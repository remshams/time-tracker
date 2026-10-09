use chrono::{TimeDelta, Utc};
use tracker_domain::WorklogTimes;

use crate::app::AppState;
use crate::application_request::{ApplicationOutcome, ApplicationRequest};
use crate::screens::WorklogHistoryMode;
use crate::support::errors::correction_error_text;
use crate::support::timestamps::{
    OUTSIDE_EDITABLE_RANGE, adjusted_correction_timestamp, correction_timestamp,
    resolve_correction_timestamp,
};

/// The correction field that receives input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrectionField {
    Start,
    End,
}

/// Editable timestamps and the immutable snapshot used for stale detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorrectionDraft {
    pub(crate) id: tracker_domain::WorklogId,
    pub(crate) expected: WorklogTimes,
    pub(crate) start: crate::support::timestamps::TimestampInput,
    pub(crate) end: Option<crate::support::timestamps::TimestampInput>,
    pub(crate) focused: CorrectionField,
    pub(crate) original_start: chrono::DateTime<chrono::Utc>,
    pub(crate) original_end: Option<chrono::DateTime<chrono::Utc>>,
}

impl CorrectionDraft {
    pub(crate) fn new(
        id: tracker_domain::WorklogId,
        expected: WorklogTimes,
        start: String,
        end: Option<String>,
    ) -> Self {
        Self {
            id,
            original_start: expected.start(),
            original_end: expected.end(),
            expected,
            start: crate::support::timestamps::TimestampInput::new(start),
            end: end.map(crate::support::timestamps::TimestampInput::new),
            focused: CorrectionField::Start,
        }
    }

    pub fn start(&self) -> &crate::support::timestamps::TimestampInput {
        &self.start
    }
    pub fn end(&self) -> Option<&crate::support::timestamps::TimestampInput> {
        self.end.as_ref()
    }
    pub fn focused(&self) -> CorrectionField {
        self.focused
    }

    pub(crate) fn original(&self, field: CorrectionField) -> chrono::DateTime<chrono::Utc> {
        match field {
            CorrectionField::Start => self.original_start,
            CorrectionField::End => self
                .original_end
                .expect("completed corrections have an end"),
        }
    }

    pub(crate) fn focused_input(&self) -> &crate::support::timestamps::TimestampInput {
        match self.focused {
            CorrectionField::Start => &self.start,
            CorrectionField::End => self
                .end
                .as_ref()
                .expect("only completed corrections can focus the end"),
        }
    }

    pub(crate) fn focused_mut(&mut self) -> &mut crate::support::timestamps::TimestampInput {
        match self.focused {
            CorrectionField::Start => &mut self.start,
            CorrectionField::End => self
                .end
                .as_mut()
                .expect("only completed corrections can focus the end"),
        }
    }

    pub(crate) fn switch_field(&mut self) {
        if self.end.is_some() {
            self.focused = match self.focused {
                CorrectionField::Start => CorrectionField::End,
                CorrectionField::End => CorrectionField::Start,
            };
        }
    }
}

impl AppState {
    pub(super) fn open_correction(&mut self) {
        let Some(worklog) = self.selected_history_worklog() else {
            return;
        };
        let timezone = self.shell().timezone();
        let Some(start) = correction_timestamp(worklog.start(), &timezone) else {
            self.shell_mut().error(OUTSIDE_EDITABLE_RANGE);
            return;
        };
        let end = match worklog
            .end()
            .map(|end| correction_timestamp(end, &timezone))
        {
            Some(Some(end)) => Some(end),
            Some(None) => {
                self.shell_mut().error(OUTSIDE_EDITABLE_RANGE);
                return;
            }
            None => None,
        };
        self.history_state_mut()
            .expect("history is open")
            .open_correction(CorrectionDraft::new(
                worklog.id(),
                worklog.times(),
                start,
                end,
            ));
        self.shell_mut().info("Edit the worklog timestamps");
    }

    pub(super) fn switch_correction_field(&mut self) {
        self.edit_correction(CorrectionDraft::switch_field);
    }

    pub(super) fn move_correction_cursor_left(&mut self) {
        self.edit_correction(|draft| draft.focused_mut().move_left());
    }

    pub(super) fn move_correction_cursor_right(&mut self) {
        self.edit_correction(|draft| draft.focused_mut().move_right());
    }

    pub(super) fn delete_correction_character(&mut self) {
        self.edit_correction(|draft| draft.focused_mut().delete());
    }

    fn edit_correction(&mut self, edit: impl FnOnce(&mut CorrectionDraft)) {
        if let Some(draft) = self
            .history_state_mut()
            .and_then(|state| state.correction_mut())
        {
            edit(draft);
        }
    }

    pub(super) fn insert_correction_character(&mut self, character: char) {
        self.edit_correction(|draft| draft.focused_mut().insert(character));
    }

    pub(super) fn backspace_correction_character(&mut self) {
        self.edit_correction(|draft| draft.focused_mut().backspace());
    }

    pub(super) fn adjust_correction(&mut self, delta: TimeDelta) {
        let Some(draft) = self.history_state().and_then(|state| state.correction()) else {
            return;
        };
        let timezone = self.shell().timezone();
        match adjusted_correction_timestamp(
            draft.focused_input(),
            delta,
            &timezone,
            draft.original(draft.focused()),
        ) {
            Ok((text, instant)) => {
                self.edit_correction(|draft| {
                    draft.focused_mut().replace_with_adjustment(text, instant)
                });
                self.shell_mut().info("Adjusted timestamp");
            }
            Err(message) => self.shell_mut().error(message),
        }
    }

    pub(super) fn confirm_correction(&mut self) {
        let Some(WorklogHistoryMode::Correction(draft)) =
            self.history_state().map(|state| state.mode().clone())
        else {
            return;
        };
        let timezone = self.shell().timezone();
        let start = match resolve_correction_timestamp(
            draft.start(),
            &timezone,
            draft.original(CorrectionField::Start),
        ) {
            Ok(start) => start,
            Err(message) => {
                self.shell_mut().error(format!("Start: {message}"));
                return;
            }
        };
        let end = match draft.end() {
            Some(input) => match resolve_correction_timestamp(
                input,
                &timezone,
                draft.original(CorrectionField::End),
            ) {
                Ok(end) => Some(end),
                Err(message) => {
                    self.shell_mut().error(format!("End: {message}"));
                    return;
                }
            },
            None => None,
        };
        let occurred_at = Utc::now();
        let replacement = WorklogTimes::new(start, end);
        let task_id = self
            .history()
            .expect("a correction belongs to an open history")
            .task_id();
        let session = self.history_state().expect("history is open").session_id();
        self.enqueue(
            ApplicationRequest::CorrectWorklog {
                id: draft.id,
                expected: draft.expected,
                replacement,
                occurred_at,
            },
            move |state, completed| {
                let current = state.history_session_matches(session, task_id)
                    && matches!(state.history_state().map(|state| state.mode()),
                    Some(WorklogHistoryMode::Correction(current)) if current.id == draft.id);
                let ApplicationOutcome::Worklog(result) = completed.outcome else {
                    unreachable!()
                };
                if !current {
                    return;
                }
                match result {
                    Ok(worklog) => {
                        state
                            .history_state_mut()
                            .expect("history is open")
                            .close_mode();
                        state.enqueue(
                            ApplicationRequest::WorklogsForTask {
                                task_id,
                                after: None,
                            },
                            move |state, completed| {
                                let current = state.history_session_matches(session, task_id)
                                    && state.history_is_normal();
                                let ApplicationOutcome::WorklogPage(result) = completed.outcome
                                else {
                                    unreachable!()
                                };

                                if !current {
                                    return;
                                }
                                match result {
                                    Ok(page) => {
                                        state.replace_history_with_newest_page(
                                            task_id,
                                            Some(worklog.id()),
                                            page,
                                        );
                                        state.reload_tasks();
                                        state.shell_mut().info("Corrected worklog");
                                    }
                                    Err(_) => {
                                        state.mark_history_unavailable();
                                        state
                                            .shell_mut()
                                            .error("Correction saved, but history refresh failed");
                                    }
                                }
                            },
                        );
                    }
                    Err(error) => state.shell_mut().error(correction_error_text(&error)),
                }
            },
        );
    }
}

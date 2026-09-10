use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use tracker_application::TrackerApplicationService;
use tracker_domain::{WorklogId, WorklogTimes};

use crate::app::{App, Status};
use crate::command::Command;
use crate::screens::{ScreenState, WorklogHistoryMode};
use crate::support::errors::correction_error_text;
use crate::support::timestamps::{
    OUTSIDE_EDITABLE_RANGE, TimestampInput, adjusted_correction_timestamp, correction_timestamp,
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
    pub(crate) id: WorklogId,
    pub(crate) expected: WorklogTimes,
    pub(crate) start: TimestampInput,
    pub(crate) end: Option<TimestampInput>,
    pub(crate) focused: CorrectionField,
    pub(crate) original_start: DateTime<Utc>,
    pub(crate) original_end: Option<DateTime<Utc>>,
}

impl CorrectionDraft {
    pub(crate) fn new(
        id: WorklogId,
        expected: WorklogTimes,
        start: String,
        end: Option<String>,
    ) -> Self {
        Self {
            id,
            original_start: expected.start(),
            original_end: expected.end(),
            expected,
            start: TimestampInput::new(start),
            end: end.map(TimestampInput::new),
            focused: CorrectionField::Start,
        }
    }

    pub fn start(&self) -> &TimestampInput {
        &self.start
    }

    pub fn end(&self) -> Option<&TimestampInput> {
        self.end.as_ref()
    }

    pub fn focused(&self) -> CorrectionField {
        self.focused
    }

    pub(crate) fn original(&self, field: CorrectionField) -> DateTime<Utc> {
        match field {
            CorrectionField::Start => self.original_start,
            CorrectionField::End => self
                .original_end
                .expect("completed corrections have an end"),
        }
    }

    pub(crate) fn focused_input(&self) -> &TimestampInput {
        match self.focused {
            CorrectionField::Start => &self.start,
            CorrectionField::End => self
                .end
                .as_ref()
                .expect("only completed corrections can focus the end"),
        }
    }

    pub(crate) fn focused_mut(&mut self) -> &mut TimestampInput {
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

impl<S: TrackerApplicationService> App<S> {
    pub(super) fn handle_correction_command(&mut self, command: Command) {
        match command {
            Command::OpenCorrection => self.open_correction(),
            Command::SwitchCorrectionField => self.edit_correction(CorrectionDraft::switch_field),
            Command::MoveCursorLeft => {
                self.edit_correction(|draft| draft.focused_mut().move_left());
            }
            Command::MoveCursorRight => {
                self.edit_correction(|draft| draft.focused_mut().move_right());
            }
            Command::Delete => self.edit_correction(|draft| draft.focused_mut().delete()),
            Command::AdjustForwardFiveMinutes => self.adjust_correction(TimeDelta::minutes(5)),
            Command::AdjustBackwardFiveMinutes => self.adjust_correction(TimeDelta::minutes(-5)),
            Command::AdjustForwardOneHour => self.adjust_correction(TimeDelta::hours(1)),
            Command::AdjustBackwardOneHour => self.adjust_correction(TimeDelta::hours(-1)),
            _ => {}
        }
    }

    pub fn correction(&self) -> Option<&CorrectionDraft> {
        let ScreenState::WorklogHistory(state) = &self.screen else {
            return None;
        };
        match state.mode() {
            WorklogHistoryMode::Correction(draft) => Some(draft),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn correction_mut_for_tests(&mut self) -> &mut CorrectionDraft {
        let ScreenState::WorklogHistory(state) = &mut self.screen else {
            panic!("correction is not open");
        };
        match &mut state.mode {
            WorklogHistoryMode::Correction(draft) => draft,
            _ => panic!("correction is not open"),
        }
    }

    fn open_correction(&mut self) {
        let timezone = self.timezone;
        match self.frozen_offset {
            Some(offset) => self.open_correction_in(&offset),
            None => self.open_correction_in(&timezone),
        }
    }

    pub(crate) fn open_correction_in<Tz>(&mut self, timezone: &Tz)
    where
        Tz: TimeZone,
    {
        if !self.history_is_normal() {
            return;
        }
        let Some(worklog) = self
            .history()
            .filter(|history| history.is_available())
            .and_then(|history| {
                history
                    .selected_index()
                    .map(|index| &history.worklogs[index])
            })
            .cloned()
        else {
            return;
        };
        let Some(start) = correction_timestamp(worklog.start(), timezone) else {
            self.status = Status::Error(OUTSIDE_EDITABLE_RANGE.to_owned());
            return;
        };
        let end = match worklog.end().map(|end| correction_timestamp(end, timezone)) {
            Some(Some(end)) => Some(end),
            Some(None) => {
                self.status = Status::Error(OUTSIDE_EDITABLE_RANGE.to_owned());
                return;
            }
            None => None,
        };
        if let ScreenState::WorklogHistory(state) = &mut self.screen {
            state.mode = WorklogHistoryMode::Correction(CorrectionDraft::new(
                worklog.id(),
                worklog.times(),
                start,
                end,
            ));
        }
        self.status = Status::Info("Edit the worklog timestamps".to_owned());
    }

    fn edit_correction(&mut self, edit: impl FnOnce(&mut CorrectionDraft)) {
        if let ScreenState::WorklogHistory(state) = &mut self.screen
            && let WorklogHistoryMode::Correction(draft) = &mut state.mode
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

    fn adjust_correction(&mut self, delta: TimeDelta) {
        let timezone = self.timezone;
        match self.frozen_offset {
            Some(offset) => self.adjust_correction_in(delta, &offset),
            None => self.adjust_correction_in(delta, &timezone),
        }
    }

    pub(crate) fn adjust_correction_in<Tz>(&mut self, delta: TimeDelta, timezone: &Tz)
    where
        Tz: TimeZone,
    {
        let Some(draft) = self.correction() else {
            return;
        };
        let adjusted = adjusted_correction_timestamp(
            draft.focused_input(),
            delta,
            timezone,
            draft.original(draft.focused),
        );
        match adjusted {
            Ok((text, instant)) => {
                self.edit_correction(|draft| {
                    draft.focused_mut().replace_with_adjustment(text, instant);
                });
                self.status = Status::Info("Adjusted timestamp".to_owned());
            }
            Err(message) => self.status = Status::Error(message.to_owned()),
        }
    }

    pub(super) fn confirm_correction(&mut self) {
        let timezone = self.timezone;
        match self.frozen_offset {
            Some(offset) => self.confirm_correction_in(&offset),
            None => self.confirm_correction_in(&timezone),
        }
    }

    pub(crate) fn confirm_correction_in<Tz>(&mut self, timezone: &Tz)
    where
        Tz: TimeZone,
    {
        let WorklogHistoryMode::Correction(draft) = self.history_state().mode().clone() else {
            return;
        };
        let start = match resolve_correction_timestamp(&draft.start, timezone, draft.original_start)
        {
            Ok(start) => start,
            Err(message) => {
                self.status = Status::Error(format!("Start: {message}"));
                return;
            }
        };
        let end = match draft.end.as_ref() {
            Some(input) => {
                let original = draft
                    .original_end
                    .expect("completed corrections have an end");
                match resolve_correction_timestamp(input, timezone, original) {
                    Ok(end) => Some(end),
                    Err(message) => {
                        self.status = Status::Error(format!("End: {message}"));
                        return;
                    }
                }
            }
            None => None,
        };
        let replacement = WorklogTimes::new(start, end);
        match self
            .application
            .correct_worklog(draft.id, draft.expected, replacement, Utc::now())
        {
            Ok(worklog) => {
                let task_id = self
                    .history()
                    .map(|history| history.task_id)
                    .expect("a correction belongs to an open history");
                let corrected_id = worklog.id();
                if let ScreenState::WorklogHistory(state) = &mut self.screen {
                    state.mode = WorklogHistoryMode::Normal;
                }
                match self.application.worklogs_for_task(task_id, None) {
                    Ok(page) => {
                        self.replace_history_with_newest_page(task_id, Some(corrected_id), page);
                        self.sync_tasks_from_application();
                        self.sync_tracking_after_history_reload();
                        self.status = Status::Info("Corrected worklog".to_owned());
                    }
                    Err(_) => {
                        self.mark_history_unavailable();
                        self.sync_from_application(false);
                        self.status = Status::Error(
                            "Correction saved, but history refresh failed".to_owned(),
                        );
                    }
                }
            }
            Err(error) => {
                self.sync_from_application(false);
                self.status = Status::Error(correction_error_text(&error));
            }
        }
    }
}

use tracker_application::ApplicationFailureCategory;

use crate::app::AppState;
use crate::application_request::{ApplicationOutcome, ApplicationRequest};
use crate::screens::WorklogHistoryMode;
use crate::support::errors::application_error_text;

use super::MoveFocus;
use super::move_worklog::MoveDraft;

impl AppState {
    pub(super) fn open_move(&mut self) {
        let Some(worklog) = self.selected_history_worklog() else {
            return;
        };
        let tasks = self.catalog().move_task_snapshot();
        self.history_state_mut()
            .expect("history is open")
            .open_move(MoveDraft::new(worklog, tasks));
        self.shell_mut().info("Choose a destination task");
    }

    fn edit_move(&mut self, edit: impl FnOnce(&mut MoveDraft)) {
        if let Some(draft) = self
            .history_state_mut()
            .and_then(|state| state.move_draft_mut())
        {
            edit(draft);
        }
    }

    pub(super) fn toggle_move_focus(&mut self) {
        self.edit_move(MoveDraft::toggle_focus);
    }

    pub(super) fn move_destination_up(&mut self) {
        self.edit_move(MoveDraft::move_up);
    }

    pub(super) fn move_destination_down(&mut self) {
        self.edit_move(MoveDraft::move_down);
    }

    pub(super) fn insert_move_query(&mut self, character: char) {
        self.edit_move(|draft| {
            if draft.focus() == MoveFocus::Search {
                draft.insert(character);
            }
        });
    }

    pub(super) fn backspace_move_query(&mut self) {
        self.edit_move(|draft| {
            if draft.focus() == MoveFocus::Search {
                draft.backspace();
            }
        });
    }

    pub(super) fn confirm_move(&mut self) {
        let Some((destination_task_id, source_task_id, source_id, source_times, destination_name)) =
            self.history_state().and_then(|state| match state.mode() {
                WorklogHistoryMode::Move(draft) => {
                    let destination_task_id = draft.selected_task_id()?;
                    let source = draft.worklog();
                    let destination_name = draft
                        .results()
                        .find(|candidate| candidate.id == destination_task_id)?
                        .name
                        .to_owned();
                    Some((
                        destination_task_id,
                        source.task_id(),
                        source.id(),
                        source.times(),
                        destination_name,
                    ))
                }
                _ => None,
            })
        else {
            return;
        };
        let preferred_selection = self.history().and_then(|history| {
            let index = history
                .worklogs()
                .iter()
                .position(|worklog| worklog.id() == source_id)?;
            history
                .worklogs()
                .get(index + 1)
                .or_else(|| {
                    index
                        .checked_sub(1)
                        .and_then(|index| history.worklogs().get(index))
                })
                .map(|worklog| worklog.id())
        });
        let session = self.history_state().expect("history is open").session_id();
        self.enqueue(
            ApplicationRequest::MoveWorklog {
                id: source_id,
                expected_source_task_id: source_task_id,
                expected: source_times,
                destination_task_id,
            },
            move |state, completed| {
                let current = state.history_session_matches(session, source_task_id)
                    && matches!(state.history_state().map(|state| state.mode()),
                    Some(WorklogHistoryMode::Move(draft)) if draft.worklog().id() == source_id);
                let ApplicationOutcome::Worklog(result) = completed.outcome else {
                    unreachable!()
                };
                if !current {
                    return;
                }
                match result {
                    Ok(_) => {
                        state
                            .history_state_mut()
                            .expect("history is open")
                            .close_mode();
                        state.enqueue(
                            ApplicationRequest::WorklogsForTask {
                                task_id: source_task_id,
                                after: None,
                            },
                            move |state, completed| {
                                let current = state
                                    .history_session_matches(session, source_task_id)
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
                                            source_task_id,
                                            preferred_selection,
                                            page,
                                        );
                                        state.shell_mut().info(format!(
                                            "Moved worklog to \"{destination_name}\""
                                        ));
                                    }
                                    Err(_) => {
                                        state.mark_history_unavailable();
                                        state
                                            .shell_mut()
                                            .error("Move saved, but history refresh failed");
                                    }
                                }
                            },
                        );
                    }
                    Err(error) => {
                        let failure = error.failure();
                        if failure.recovery_failed() {
                            state.shell_mut().error(application_error_text(&error));
                            return;
                        }
                        let stale = matches!(
                            failure.category(),
                            ApplicationFailureCategory::WorklogChanged
                                | ApplicationFailureCategory::WorklogNotFound
                        );
                        let destination_unavailable = state
                            .catalog()
                            .task(destination_task_id)
                            .is_none_or(|task| task.is_archived());
                        if stale || destination_unavailable {
                            state
                                .history_state_mut()
                                .expect("history is open")
                                .close_mode();
                            state.reload_newest_history_after_change(source_task_id);
                        } else {
                            state.shell_mut().error(application_error_text(&error));
                        }
                    }
                }
            },
        );
    }
}

use tracker_application::{ApplicationFailureCategory, TrackerApplicationService};

use crate::app::App;
use crate::screens::{TaskView, WorklogHistoryMode};
use crate::support::errors::application_error_text;

use super::MoveFocus;
use super::move_worklog::{MoveCandidate, MoveDraft};

impl<S: TrackerApplicationService> App<S> {
    pub(super) fn open_move(&mut self) {
        let Some(worklog) = self.selected_history_worklog() else {
            return;
        };
        let candidates = self
            .catalog()
            .tasks(TaskView::Active)
            .iter()
            .filter(|task| task.id() != worklog.task_id())
            .map(|task| MoveCandidate::new(task.id(), task.name().to_string()))
            .collect();
        self.history_state_mut()
            .expect("history is open")
            .open_move(MoveDraft::new(worklog, candidates));
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
                        .find(|candidate| candidate.id() == destination_task_id)?
                        .name()
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
        match self.application_mut().move_worklog(
            source_id,
            source_task_id,
            source_times,
            destination_task_id,
        ) {
            Ok(_) => {
                self.history_state_mut()
                    .expect("history is open")
                    .close_mode();
                match self
                    .application_mut()
                    .worklogs_for_task(source_task_id, None)
                {
                    Ok(page) => {
                        self.replace_history_with_newest_page(
                            source_task_id,
                            preferred_selection,
                            page,
                        );
                        self.reload_tasks();
                        self.sync_tracking_after_history_reload();
                        self.shell_mut()
                            .info(format!("Moved worklog to \"{destination_name}\""));
                    }
                    Err(_) => {
                        self.mark_history_unavailable();
                        self.reload_tasks();
                        self.sync_tracking_after_history_reload();
                        self.shell_mut()
                            .error("Move saved, but history refresh failed");
                    }
                }
            }
            Err(error) => {
                let failure = error.failure();
                self.sync_from_application(false);
                if failure.recovery_failed() {
                    self.shell_mut().error(application_error_text(&error));
                    return;
                }
                let stale = matches!(
                    failure.category(),
                    ApplicationFailureCategory::WorklogChanged
                        | ApplicationFailureCategory::WorklogNotFound
                );
                let destination_unavailable = self
                    .catalog()
                    .task(destination_task_id)
                    .is_none_or(|task| task.is_archived());
                if stale || destination_unavailable {
                    self.history_state_mut()
                        .expect("history is open")
                        .close_mode();
                    self.reload_newest_history_after_change(source_task_id);
                } else {
                    self.shell_mut().error(application_error_text(&error));
                }
            }
        }
    }
}

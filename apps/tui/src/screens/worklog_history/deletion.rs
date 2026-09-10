use tracker_application::{ApplicationError, TrackerApplicationService};
use tracker_domain::{Worklog, WorklogId};

use crate::app::{App, Status};
use crate::screens::{ScreenState, WorklogHistoryMode};
use crate::support::errors::{
    ACTIVE_WORKLOG_DELETE_MESSAGE, application_error_text, deletion_conflict,
    matches_worklog_active, matches_worklog_not_found,
};

impl<S: TrackerApplicationService> App<S> {
    pub(super) fn open_deletion(&mut self) {
        if !self.history_is_normal() {
            return;
        }
        let Some(worklog) = self
            .history()
            .filter(|history| history.is_available())
            .and_then(|history| {
                history
                    .selected_index()
                    .map(|index| history.worklogs[index].clone())
            })
        else {
            return;
        };
        if worklog.is_active() {
            self.status = Status::Error(ACTIVE_WORKLOG_DELETE_MESSAGE.to_owned());
            return;
        }
        if let ScreenState::WorklogHistory(state) = &mut self.screen {
            state.mode = WorklogHistoryMode::ConfirmDeletion { worklog };
        }
    }

    #[cfg(test)]
    pub fn deletion(&self) -> Option<&Worklog> {
        let ScreenState::WorklogHistory(state) = &self.screen else {
            return None;
        };
        match state.mode() {
            WorklogHistoryMode::ConfirmDeletion { worklog } => Some(worklog),
            _ => None,
        }
    }

    pub(super) fn confirm_deletion(&mut self) {
        let WorklogHistoryMode::ConfirmDeletion { worklog } = self.history_state().mode().clone()
        else {
            return;
        };
        let target_id = worklog.id();
        let task_id = worklog.task_id();
        let result = self
            .application
            .delete_completed_worklog(target_id, task_id, worklog.times());
        match result {
            Ok(_) => {
                self.remove_deleted_worklog(target_id);
                self.sync_tasks_from_application();
                if let ScreenState::WorklogHistory(state) = &mut self.screen {
                    state.mode = WorklogHistoryMode::Normal;
                }
                self.status = Status::Info("Deleted worklog".to_owned());
            }
            Err(error) if deletion_conflict(&error) => {
                if let ScreenState::WorklogHistory(state) = &mut self.screen {
                    state.mode = WorklogHistoryMode::Normal;
                }
                self.reload_newest_history_after_deletion(task_id, target_id, &error);
            }
            Err(error) => {
                self.sync_from_application(false);
                self.status = Status::Error(application_error_text(&error));
            }
        }
    }

    fn remove_deleted_worklog(&mut self, deleted_id: WorklogId) {
        let Some(history) = self.history_mut() else {
            return;
        };
        let Some(index) = history
            .worklogs
            .iter()
            .position(|worklog| worklog.id() == deleted_id)
        else {
            history.selected = None;
            return;
        };
        history.worklogs.remove(index);
        history.selected = history
            .worklogs
            .get(index.min(history.worklogs.len().saturating_sub(1)))
            .map(Worklog::id);
    }

    fn reload_newest_history_after_deletion(
        &mut self,
        task_id: tracker_domain::TaskId,
        target_id: WorklogId,
        error: &ApplicationError,
    ) {
        match self.application.worklogs_for_task(task_id, None) {
            Ok(page) => {
                let target_is_present = page
                    .worklogs
                    .iter()
                    .any(|worklog| worklog.id() == target_id);
                self.replace_history_with_newest_page(task_id, Some(target_id), page);
                self.sync_from_application(false);
                self.status = if matches_worklog_active(error) {
                    Status::Error(ACTIVE_WORKLOG_DELETE_MESSAGE.to_owned())
                } else if matches_worklog_not_found(error) {
                    if target_is_present {
                        Status::Error(
                            "Worklog was not found. Press d to confirm deletion again.".to_owned(),
                        )
                    } else {
                        Status::Error("Worklog was not found. History was refreshed.".to_owned())
                    }
                } else if target_is_present {
                    Status::Error("Worklog changed. Press d to confirm deletion again.".to_owned())
                } else {
                    Status::Error("Worklog changed. History was refreshed.".to_owned())
                };
            }
            Err(refresh_error) => {
                self.mark_history_unavailable();
                self.sync_from_application(false);
                self.status = if matches_worklog_active(error) {
                    Status::Error(ACTIVE_WORKLOG_DELETE_MESSAGE.to_owned())
                } else {
                    Status::Error(format!(
                        "Worklog changed, but history refresh failed: {}",
                        application_error_text(&refresh_error)
                    ))
                };
            }
        }
    }
}

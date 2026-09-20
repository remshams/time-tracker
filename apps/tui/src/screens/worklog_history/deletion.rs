use tracker_application::{ApplicationError, TrackerApplicationService};
use tracker_domain::{TaskId, WorklogId};

use crate::app::App;
use crate::screens::WorklogHistoryMode;
use crate::support::errors::{
    ACTIVE_WORKLOG_DELETE_MESSAGE, application_error_text, deletion_conflict,
    matches_worklog_active, matches_worklog_not_found,
};

impl<S: TrackerApplicationService> App<S> {
    pub(super) fn open_deletion(&mut self) {
        let Some(worklog) = self.selected_history_worklog() else {
            return;
        };
        if worklog.is_active() {
            self.shell_mut().error(ACTIVE_WORKLOG_DELETE_MESSAGE);
            return;
        }
        self.history_state_mut()
            .expect("history is open")
            .open_deletion(worklog);
    }

    pub(super) fn confirm_deletion(&mut self) {
        let Some(WorklogHistoryMode::ConfirmDeletion { worklog }) =
            self.history_state().map(|state| state.mode().clone())
        else {
            return;
        };
        let target_id = worklog.id();
        let task_id = worklog.task_id();
        match self
            .application_mut()
            .delete_completed_worklog(target_id, task_id, worklog.times())
        {
            Ok(_) => {
                self.history_mut()
                    .expect("history is open")
                    .remove(target_id);
                self.reload_tasks();
                self.history_state_mut()
                    .expect("history is open")
                    .close_mode();
                self.shell_mut().info("Deleted worklog");
            }
            Err(error) if deletion_conflict(&error) => {
                self.history_state_mut()
                    .expect("history is open")
                    .close_mode();
                self.reload_newest_history_after_deletion(task_id, target_id, &error);
            }
            Err(error) => {
                self.sync_from_application(false);
                self.shell_mut().error(application_error_text(&error));
            }
        }
    }

    fn reload_newest_history_after_deletion(
        &mut self,
        task_id: TaskId,
        target_id: WorklogId,
        error: &ApplicationError,
    ) {
        match self.application_mut().worklogs_for_task(task_id, None) {
            Ok(page) => {
                let target_is_present = page
                    .worklogs
                    .iter()
                    .any(|worklog| worklog.id() == target_id);
                self.replace_history_with_newest_page(task_id, Some(target_id), page);
                self.sync_from_application(false);
                if matches_worklog_active(error) {
                    self.shell_mut().error(ACTIVE_WORKLOG_DELETE_MESSAGE);
                } else if matches_worklog_not_found(error) {
                    self.shell_mut().error(if target_is_present {
                        "Worklog was not found. Press d to confirm deletion again."
                    } else {
                        "Worklog was not found. History was refreshed."
                    });
                } else if target_is_present {
                    self.shell_mut()
                        .error("Worklog changed. Press d to confirm deletion again.");
                } else {
                    self.shell_mut()
                        .error("Worklog changed. History was refreshed.");
                }
            }
            Err(refresh_error) => {
                self.mark_history_unavailable();
                self.sync_from_application(false);
                if matches_worklog_active(error) {
                    self.shell_mut().error(ACTIVE_WORKLOG_DELETE_MESSAGE);
                } else {
                    self.shell_mut().error(format!(
                        "Worklog changed, but history refresh failed: {}",
                        application_error_text(&refresh_error)
                    ));
                }
            }
        }
    }
}

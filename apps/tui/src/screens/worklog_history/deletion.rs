use tracker_domain::{TaskId, WorklogId};

use crate::app::AppState;
use crate::application_request::{ApplicationOutcome, ApplicationRequest};
use crate::screens::WorklogHistoryMode;
use crate::support::errors::{
    ACTIVE_WORKLOG_DELETE_MESSAGE, application_error_text, deletion_conflict,
    matches_worklog_active, matches_worklog_not_found,
};

impl AppState {
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
        let session = self.history_state().expect("history is open").session_id();
        self.enqueue(ApplicationRequest::DeleteCompletedWorklog {
            id: target_id, expected_task_id: task_id, expected: worklog.times(),
        }, move |state, completed| {
            let current = state.history_session_matches(session, task_id)
                && matches!(state.history_state().map(|state| state.mode()),
                    Some(WorklogHistoryMode::ConfirmDeletion { worklog }) if worklog.id() == target_id);
            let ApplicationOutcome::Worklog(result) = completed.outcome else { unreachable!() };
            state.sync_from_snapshot(completed.snapshot.items, completed.snapshot.tracking, false);
            if !current { return; }
            match result {
                Ok(_) => {
                    state.history_mut().expect("history is open").remove(target_id);
                    state.reload_tasks();
                    state.history_state_mut().expect("history is open").close_mode();
                    state.shell_mut().info("Deleted worklog");
                }
                Err(error) if deletion_conflict(&error) => {
                    state.history_state_mut().expect("history is open").close_mode();
                    state.reload_newest_history_after_deletion(session, task_id, target_id, error);
                }
                Err(error) => state.shell_mut().error(application_error_text(&error)),
            }
        });
    }

    fn reload_newest_history_after_deletion(
        &mut self,
        session: u64,
        task_id: TaskId,
        target_id: WorklogId,
        error: tracker_application::ApplicationError,
    ) {
        self.enqueue(
            ApplicationRequest::WorklogsForTask {
                task_id,
                after: None,
            },
            move |state, completed| {
                let current =
                    state.history_session_matches(session, task_id) && state.history_is_normal();
                let ApplicationOutcome::WorklogPage(result) = completed.outcome else {
                    unreachable!()
                };
                state.sync_from_snapshot(
                    completed.snapshot.items,
                    completed.snapshot.tracking,
                    false,
                );
                if !current {
                    return;
                }
                match result {
                    Ok(page) => {
                        let target_is_present = page
                            .worklogs
                            .iter()
                            .any(|worklog| worklog.id() == target_id);
                        state.replace_history_with_newest_page(task_id, Some(target_id), page);
                        if matches_worklog_active(&error) {
                            state.shell_mut().error(ACTIVE_WORKLOG_DELETE_MESSAGE);
                        } else if matches_worklog_not_found(&error) {
                            state.shell_mut().error(if target_is_present {
                                "Worklog was not found. Press d to confirm deletion again."
                            } else {
                                "Worklog was not found. History was refreshed."
                            });
                        } else if target_is_present {
                            state
                                .shell_mut()
                                .error("Worklog changed. Press d to confirm deletion again.");
                        } else {
                            state
                                .shell_mut()
                                .error("Worklog changed. History was refreshed.");
                        }
                    }
                    Err(refresh_error) => {
                        state.mark_history_unavailable();
                        if matches_worklog_active(&error) {
                            state.shell_mut().error(ACTIVE_WORKLOG_DELETE_MESSAGE);
                        } else {
                            state.shell_mut().error(format!(
                                "Worklog changed, but history refresh failed: {}",
                                application_error_text(&refresh_error)
                            ));
                        }
                    }
                }
            },
        );
    }
}

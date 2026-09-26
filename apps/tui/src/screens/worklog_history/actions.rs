use chrono::TimeDelta;
use tracker_application::{ApplicationFailureCategory, TrackerApplicationService, WorklogPage};
use tracker_domain::{TaskId, WorklogId};

use crate::app::App;
use crate::screens::worklog_history::active_worklog_for_task;
use crate::screens::{History, Screen, WorklogHistoryCommand, WorklogHistoryMode};
use crate::support::errors::application_error_text;

impl<S: TrackerApplicationService> App<S> {
    pub(crate) fn handle_worklog_history_command(&mut self, command: WorklogHistoryCommand) {
        match command {
            WorklogHistoryCommand::MoveUp => self.move_history_up(),
            WorklogHistoryCommand::MoveDown => self.move_history_down(),
            WorklogHistoryCommand::First
            | WorklogHistoryCommand::Last
            | WorklogHistoryCommand::PageUp
            | WorklogHistoryCommand::PageDown => self.jump_history(command),
            WorklogHistoryCommand::GPrefix => self
                .history_state_mut()
                .expect("history is open")
                .set_g_prefix(true),
            WorklogHistoryCommand::OpenCorrection => self.open_correction(),
            WorklogHistoryCommand::OpenDeletion => self.open_deletion(),
            WorklogHistoryCommand::OpenMove => self.open_move(),
            WorklogHistoryCommand::SwitchCorrectionField => self.switch_correction_field(),
            WorklogHistoryCommand::MoveCursorLeft => self.move_correction_cursor_left(),
            WorklogHistoryCommand::MoveCursorRight => self.move_correction_cursor_right(),
            WorklogHistoryCommand::ToggleMoveFocus => self.toggle_move_focus(),
            WorklogHistoryCommand::MoveDestinationUp => self.move_destination_up(),
            WorklogHistoryCommand::MoveDestinationDown => self.move_destination_down(),
            WorklogHistoryCommand::Delete => self.delete_correction_character(),
            WorklogHistoryCommand::AdjustForwardFiveMinutes => {
                self.adjust_correction(TimeDelta::minutes(5));
            }
            WorklogHistoryCommand::AdjustBackwardFiveMinutes => {
                self.adjust_correction(TimeDelta::minutes(-5));
            }
            WorklogHistoryCommand::AdjustForwardOneHour => {
                self.adjust_correction(TimeDelta::hours(1));
            }
            WorklogHistoryCommand::AdjustBackwardOneHour => {
                self.adjust_correction(TimeDelta::hours(-1));
            }
            WorklogHistoryCommand::LoadOlderWorklogs => self.load_older_worklogs(),
            WorklogHistoryCommand::RefreshWorklogs => self.refresh_worklogs(),
            WorklogHistoryCommand::BackToTaskList => self.back_to_task_list(),
            WorklogHistoryCommand::Confirm => self.confirm_history_mode(),
            WorklogHistoryCommand::Cancel => self.cancel_history_mode(),
            WorklogHistoryCommand::Insert(character) => {
                self.insert_correction_character(character);
            }
            WorklogHistoryCommand::Backspace => self.backspace_correction_character(),
            WorklogHistoryCommand::InsertMoveQuery(character) => {
                self.insert_move_query(character);
            }
            WorklogHistoryCommand::BackspaceMoveQuery => self.backspace_move_query(),
        }
        if command != WorklogHistoryCommand::GPrefix
            && let Some(history) = self.history_state_mut()
        {
            history.set_g_prefix(false);
        }
    }

    fn jump_history(&mut self, command: WorklogHistoryCommand) {
        if !self.history_is_normal() {
            return;
        }
        let history = self.history().expect("history is open");
        if !history.is_available() || history.worklogs().is_empty() {
            return;
        }
        let len = history.worklogs().len();
        let index = history.selected_index().unwrap_or(0);
        let target = match command {
            WorklogHistoryCommand::First => 0,
            WorklogHistoryCommand::Last => len - 1,
            WorklogHistoryCommand::PageUp => index.saturating_sub(10),
            WorklogHistoryCommand::PageDown => (index + 10).min(len - 1),
            _ => return,
        };
        self.history_mut()
            .expect("history is open")
            .select_index(target);
    }

    pub(crate) fn open_history(&mut self) {
        if self.shell().screen() != Screen::TaskList
            || !matches!(
                self.shell().task_list().mode(),
                crate::screens::task_list::TaskListMode::Normal
            )
        {
            return;
        }
        let list = self.shell().task_list();
        let Some(task) = list
            .selection()
            .and_then(|id| {
                self.catalog()
                    .tasks(list.view())
                    .iter()
                    .find(|task| task.id() == id)
            })
            .cloned()
        else {
            return;
        };
        let result = self.application_mut().worklogs_for_task(task.id(), None);
        self.sync_from_application(false);
        match result {
            Ok(page) => {
                let baseline = active_worklog_for_task(&page.snapshot.active_worklog, task.id());
                self.shell_mut().open_history(History::new(
                    task.id(),
                    page.worklogs,
                    page.next_cursor,
                    baseline,
                ));
                self.shell_mut()
                    .info(format!("History of \"{}\"", task.name()));
            }
            Err(error) => self.shell_mut().error(application_error_text(&error)),
        }
    }

    pub(crate) fn back_to_task_list(&mut self) {
        if self.history_is_normal() {
            self.shell_mut().back_to_task_list();
            self.refresh_reports_now();
        }
    }

    fn confirm_history_mode(&mut self) {
        match self.history_state().map(|state| state.mode()) {
            Some(WorklogHistoryMode::ConfirmDeletion { .. }) => self.confirm_deletion(),
            Some(WorklogHistoryMode::Correction(_)) => self.confirm_correction(),
            Some(WorklogHistoryMode::Move(_)) => self.confirm_move(),
            Some(WorklogHistoryMode::Normal) | None => {}
        }
    }

    pub(super) fn history_state(&self) -> Option<&crate::screens::WorklogHistoryState> {
        self.shell().history()
    }

    pub(super) fn history_state_mut(&mut self) -> Option<&mut crate::screens::WorklogHistoryState> {
        self.shell_mut().history_mut()
    }

    pub(super) fn history(&self) -> Option<&History> {
        self.history_state()
            .map(crate::screens::WorklogHistoryState::history)
    }

    pub(super) fn history_mut(&mut self) -> Option<&mut History> {
        self.history_state_mut()
            .map(crate::screens::WorklogHistoryState::history_mut)
    }

    pub(super) fn history_is_normal(&self) -> bool {
        self.history_state()
            .is_some_and(crate::screens::WorklogHistoryState::is_normal)
    }

    pub(super) fn selected_history_worklog(&self) -> Option<tracker_domain::Worklog> {
        if !self.history_is_normal() {
            return None;
        }
        self.history()
            .filter(|history| history.is_available())
            .and_then(History::selected_worklog)
            .cloned()
    }

    fn move_history_up(&mut self) {
        if self.history_is_normal() {
            self.history_mut().expect("history is open").move_up();
        }
    }

    fn move_history_down(&mut self) {
        if self.history_is_normal() {
            self.history_mut().expect("history is open").move_down();
        }
    }

    fn cancel_history_mode(&mut self) {
        let Some(state) = self.history_state_mut() else {
            return;
        };
        let message = match state.mode() {
            WorklogHistoryMode::Correction(_) => Some("Correction cancelled"),
            WorklogHistoryMode::ConfirmDeletion { .. } => Some("Deletion cancelled"),
            WorklogHistoryMode::Move(_) => Some("Move cancelled"),
            WorklogHistoryMode::Normal => None,
        };
        if let Some(message) = message {
            state.close_mode();
            self.shell_mut().info(message);
        }
    }

    fn load_older_worklogs(&mut self) {
        if !self.history_is_normal() {
            return;
        }
        let Some(task_id) = self
            .history()
            .filter(|history| history.is_available())
            .map(History::task_id)
        else {
            return;
        };
        let Some(cursor) = self.history().and_then(History::next_cursor) else {
            self.shell_mut().info("No older worklogs");
            return;
        };
        let baseline = self.history().and_then(History::active_worklog_baseline);
        match self
            .application_mut()
            .worklogs_for_task(task_id, Some(&cursor))
        {
            Ok(page) => {
                let active = active_worklog_for_task(&page.snapshot.active_worklog, task_id);
                if active != baseline {
                    self.reload_newest_history_after_change(task_id);
                    return;
                }
                let loaded = page.worklogs.len();
                self.history_mut()
                    .expect("history is open")
                    .append(page.worklogs, page.next_cursor);
                self.sync_from_application(false);
                if loaded == 0 {
                    self.shell_mut().info("No older worklogs");
                } else {
                    self.shell_mut()
                        .info(format!("Loaded {loaded} older worklogs"));
                }
            }
            Err(error)
                if error.failure().category()
                    == ApplicationFailureCategory::WorklogHistoryChanged =>
            {
                self.reload_newest_history_after_change(task_id);
            }
            Err(error) => {
                self.sync_from_application(false);
                self.shell_mut().error(application_error_text(&error));
            }
        }
    }

    fn refresh_worklogs(&mut self) {
        if !self.history_is_normal() {
            return;
        }
        let Some(task_id) = self.history().map(History::task_id) else {
            return;
        };
        let keep = self.history().and_then(History::selected_id);
        let result = self.application_mut().worklogs_for_task(task_id, None);
        self.sync_from_application(false);
        match result {
            Ok(page) => {
                self.replace_history_with_newest_page(task_id, keep, page);
                self.shell_mut().info("Refreshed");
            }
            Err(error) => self.shell_mut().error(application_error_text(&error)),
        }
    }

    pub(super) fn reload_newest_history_after_change(&mut self, task_id: TaskId) {
        let keep = self.history().and_then(History::selected_id);
        match self.application_mut().worklogs_for_task(task_id, None) {
            Ok(page) => {
                self.replace_history_with_newest_page(task_id, keep, page);
                self.sync_from_application(false);
                self.shell_mut().info("History changed and was refreshed");
            }
            Err(error) => {
                self.mark_history_unavailable();
                self.sync_from_application(false);
                self.shell_mut().error(format!(
                    "History changed, but refresh failed: {}",
                    application_error_text(&error)
                ));
            }
        }
    }

    pub(super) fn replace_history_with_newest_page(
        &mut self,
        task_id: TaskId,
        keep: Option<WorklogId>,
        page: WorklogPage,
    ) {
        let baseline = active_worklog_for_task(&page.snapshot.active_worklog, task_id);
        let Some(history) = self.history_mut() else {
            return;
        };
        history.replace(task_id, page.worklogs, page.next_cursor, baseline, keep);
    }

    pub(super) fn mark_history_unavailable(&mut self) {
        if let Some(history) = self.history_mut() {
            history.mark_unavailable();
        }
    }

    pub(crate) fn sync_tracking_after_history_reload(&mut self) {
        let tracking = self.application_mut().current_tracking().clone();
        self.tracking_mut().sync_after_history_reload(tracking);
    }
}

#[cfg(test)]
mod navigation_tests {
    use crate::command::Command;
    use crate::screens::task_list::TaskListCommand;
    use crate::screens::worklog_history::WorklogHistoryCommand;
    use crate::test_support::app_with;

    #[test]
    fn paging_an_empty_history_keeps_it_unselected() {
        let mut app = app_with(&["empty task"]);
        app.handle(Command::TaskList(TaskListCommand::OpenHistory));
        app.handle(Command::WorklogHistory(WorklogHistoryCommand::Last));
        assert_eq!(
            app.shell().history().unwrap().history().selected_index(),
            None
        );
    }
}

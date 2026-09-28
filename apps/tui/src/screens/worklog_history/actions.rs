use chrono::TimeDelta;
use tracker_application::{ApplicationFailureCategory, WorklogPage};
use tracker_domain::{TaskId, WorklogId};

use crate::app::AppState;
use crate::application_request::{ApplicationOutcome, ApplicationRequest};
use crate::screens::worklog_history::active_worklog_for_task;
use crate::screens::{History, Screen, WorklogHistoryCommand, WorklogHistoryMode};
use crate::support::errors::application_error_text;

impl AppState {
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
        let task_id = task.id();
        let screen_generation = self.shell().screen_generation();
        let view_generation = self.shell().task_list().view_generation();
        self.enqueue(
            ApplicationRequest::WorklogsForTask {
                task_id,
                after: None,
            },
            move |state, completed| {
                let still_selected = state.shell().screen() == Screen::TaskList
                    && state.shell().screen_generation() == screen_generation
                    && state.shell().task_list().view_generation() == view_generation
                    && matches!(
                        state.shell().task_list().mode(),
                        crate::screens::task_list::TaskListMode::Normal
                    )
                    && state.shell().task_list().selection() == Some(task_id);
                let ApplicationOutcome::WorklogPage(result) = completed.outcome else {
                    unreachable!()
                };
                state.sync_from_snapshot(
                    completed.snapshot.items,
                    completed.snapshot.tracking,
                    false,
                );
                if !still_selected {
                    return;
                }
                match result {
                    Ok(page) => {
                        let baseline =
                            active_worklog_for_task(&page.snapshot.active_worklog, task_id);
                        state.shell_mut().open_history(History::new(
                            task_id,
                            page.worklogs,
                            page.next_cursor,
                            baseline,
                        ));
                        state
                            .shell_mut()
                            .info(format!("History of \"{}\"", task.name()));
                    }
                    Err(error) => state.shell_mut().error(application_error_text(&error)),
                }
            },
        );
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
        let session = self.history_state().expect("history is open").session_id();
        self.enqueue(
            ApplicationRequest::WorklogsForTask {
                task_id,
                after: Some(cursor),
            },
            move |state, completed| {
                let current = state.history_session_matches(session, task_id)
                    && state.history_is_normal()
                    && state.history().and_then(History::next_cursor) == Some(cursor);
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
                        let active =
                            active_worklog_for_task(&page.snapshot.active_worklog, task_id);
                        if active != baseline {
                            state.reload_newest_history_after_change(task_id);
                            return;
                        }
                        let loaded = page.worklogs.len();
                        state
                            .history_mut()
                            .expect("history is open")
                            .append(page.worklogs, page.next_cursor);
                        if loaded == 0 {
                            state.shell_mut().info("No older worklogs");
                        } else {
                            state
                                .shell_mut()
                                .info(format!("Loaded {loaded} older worklogs"));
                        }
                    }
                    Err(error)
                        if error.failure().category()
                            == ApplicationFailureCategory::WorklogHistoryChanged =>
                    {
                        state.reload_newest_history_after_change(task_id);
                    }
                    Err(error) => state.shell_mut().error(application_error_text(&error)),
                }
            },
        );
    }

    fn refresh_worklogs(&mut self) {
        if !self.history_is_normal() {
            return;
        }
        let Some(task_id) = self.history().map(History::task_id) else {
            return;
        };
        let keep = self.history().and_then(History::selected_id);
        let session = self.history_state().expect("history is open").session_id();
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
                        state.replace_history_with_newest_page(task_id, keep, page);
                        state.shell_mut().info("Refreshed");
                    }
                    Err(error) => state.shell_mut().error(application_error_text(&error)),
                }
            },
        );
    }

    pub(super) fn reload_newest_history_after_change(&mut self, task_id: TaskId) {
        let keep = self.history().and_then(History::selected_id);
        let Some(session) = self.history_state().map(|state| state.session_id()) else {
            return;
        };
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
                        state.replace_history_with_newest_page(task_id, keep, page);
                        state.shell_mut().info("History changed and was refreshed");
                    }
                    Err(error) => {
                        state.mark_history_unavailable();
                        state.shell_mut().error(format!(
                            "History changed, but refresh failed: {}",
                            application_error_text(&error)
                        ));
                    }
                }
            },
        );
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

    pub(super) fn history_session_matches(&self, session: u64, task_id: TaskId) -> bool {
        self.history_state().is_some_and(|state| {
            state.session_id() == session && state.history().task_id() == task_id
        })
    }

    pub(crate) fn sync_tracking_after_history_reload(
        &mut self,
        tracking: tracker_domain::TrackingState,
    ) {
        self.tracking_mut().sync_after_history_reload(tracking);
    }
}

#[cfg(test)]
mod navigation_tests {
    use tracker_application::{TaskListItem, WorklogPage, WorklogPageSnapshot};
    use tracker_domain::TrackingState;

    use crate::app::AppState;
    use crate::application_request::{ApplicationOutcome, ApplicationSnapshot, CompletedRequest};
    use crate::command::Command;
    use crate::screens::task_list::TaskListCommand;
    use crate::screens::worklog_history::WorklogHistoryCommand;
    use crate::screens::{Screen, TaskView};
    use crate::test_support::{app_with, at, task};

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

    #[test]
    fn delayed_history_does_not_reopen_after_leaving_and_returning_to_the_task_list() {
        let selected = task(1, "alpha");
        let items = vec![TaskListItem {
            task: selected.clone(),
            latest_work_start: None,
        }];
        let mut state = AppState::load_from_snapshot(items.clone(), TrackingState::Idle);
        state.handle_command(Command::TaskList(TaskListCommand::OpenHistory));
        let effect = state.take_effect().expect("history request");
        let request = effect.request.clone();

        state.shell_mut().open_reports(at(100));
        state
            .shell_mut()
            .leave_reports(TaskView::Active, Some(selected.id()));
        assert_eq!(state.shell().task_list().selection(), Some(selected.id()));
        state.complete_effect(
            effect,
            CompletedRequest {
                request,
                outcome: ApplicationOutcome::WorklogPage(Ok(WorklogPage {
                    worklogs: Vec::new(),
                    snapshot: WorklogPageSnapshot {
                        requested_task_latest_work_start: None,
                        active_worklog: None,
                        active_task_latest_work_start: None,
                    },
                    next_cursor: None,
                })),
                snapshot: ApplicationSnapshot {
                    items,
                    tracking: TrackingState::Idle,
                },
            },
        );
        assert_eq!(state.shell().screen(), Screen::TaskList);
    }

    #[test]
    fn delayed_history_does_not_open_after_switching_task_views_twice() {
        let selected = task(1, "alpha");
        let items = vec![TaskListItem {
            task: selected.clone(),
            latest_work_start: None,
        }];
        let mut state = AppState::load_from_snapshot(items.clone(), TrackingState::Idle);
        state.handle_command(Command::TaskList(TaskListCommand::OpenHistory));
        let effect = state.take_effect().expect("history request");
        let request = effect.request.clone();

        state.handle_command(Command::TaskList(TaskListCommand::ShowArchivedTasks));
        state.handle_command(Command::TaskList(TaskListCommand::ShowActiveTasks));
        assert_eq!(state.shell().task_list().selection(), Some(selected.id()));
        state.complete_effect(
            effect,
            CompletedRequest {
                request,
                outcome: ApplicationOutcome::WorklogPage(Ok(WorklogPage {
                    worklogs: Vec::new(),
                    snapshot: WorklogPageSnapshot {
                        requested_task_latest_work_start: None,
                        active_worklog: None,
                        active_task_latest_work_start: None,
                    },
                    next_cursor: None,
                })),
                snapshot: ApplicationSnapshot {
                    items,
                    tracking: TrackingState::Idle,
                },
            },
        );
        assert_eq!(state.shell().screen(), Screen::TaskList);
    }
}

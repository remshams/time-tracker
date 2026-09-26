use tracker_application::{ApplicationFailureCategory, TrackerApplicationService};

use crate::app::App;
use crate::screens::worklog_history::move_worklog::{MoveCandidate, MoveDraft};
use crate::screens::{Screen, TaskView};
use crate::support::errors::application_error_text;

use super::{AllWorklogsCommand as C, AllWorklogsFocus, AllWorklogsState};

impl<S: TrackerApplicationService> App<S> {
    pub(crate) fn open_all_worklogs(&mut self) {
        if !matches!(self.shell().screen(), Screen::TaskList | Screen::Reports) {
            return;
        }
        match self.application_mut().all_worklogs(None) {
            Ok(page) => {
                self.shell_mut()
                    .open_all_worklogs(AllWorklogsState::new(page));
                self.sync_from_application(false);
            }
            Err(error) => self.shell_mut().error(application_error_text(&error)),
        }
    }

    pub(crate) fn handle_all_worklogs_command(&mut self, command: C) {
        match command {
            C::ShowActive => {
                let first = self
                    .catalog()
                    .tasks(TaskView::Active)
                    .first()
                    .map(|task| task.id());
                self.shell_mut().leave_all_worklogs_for_tasks();
                self.shell_mut()
                    .task_list_mut()
                    .show(TaskView::Active, first);
                self.reload_tasks();
            }
            C::ShowReports => {
                self.shell_mut()
                    .leave_all_worklogs_for_reports(chrono::Utc::now());
                self.refresh_reports_now();
            }
            C::FocusRows => self.global_mut().focus = AllWorklogsFocus::Rows,
            C::FocusTabs => self.global_mut().focus = AllWorklogsFocus::Tabs,
            C::MoveUp => self.global_mut().move_selection(false),
            C::MoveDown => self.global_mut().move_selection(true),
            C::First | C::Last | C::PageUp | C::PageDown => self.jump_global(command),
            C::GPrefix => self.global_mut().g_prefix = true,
            C::LoadOlder => self.load_older_global(),
            C::Refresh => self.refresh_global(),
            C::OpenMove => self.open_global_move(),
            C::ToggleMoveFocus => self.edit_global_move(MoveDraft::toggle_focus),
            C::MoveDestinationUp => self.edit_global_move(MoveDraft::move_up),
            C::MoveDestinationDown => self.edit_global_move(MoveDraft::move_down),
            C::InsertMoveQuery(character) => self.edit_global_move(|draft| {
                if draft.focus() == crate::screens::worklog_history::MoveFocus::Search {
                    draft.insert(character);
                }
            }),
            C::BackspaceMoveQuery => self.edit_global_move(|draft| {
                if draft.focus() == crate::screens::worklog_history::MoveFocus::Search {
                    draft.backspace();
                }
            }),
            C::ConfirmMove => self.confirm_global_move(),
            C::CancelMove => {
                self.global_mut().move_draft = None;
                self.shell_mut().info("Move cancelled");
            }
        }
        if command != C::GPrefix {
            if let Some(state) = self.shell_mut().all_worklogs_mut() {
                state.g_prefix = false;
            }
        }
    }

    fn global_mut(&mut self) -> &mut AllWorklogsState {
        self.shell_mut()
            .all_worklogs_mut()
            .expect("worklogs are open")
    }

    fn jump_global(&mut self, command: C) {
        let Some(state) = self.shell().all_worklogs() else {
            return;
        };
        if !state.available || state.worklogs().is_empty() {
            return;
        }
        let index = state.selected_index().unwrap_or(0);
        let last = state.worklogs().len() - 1;
        let target = match command {
            C::First => 0,
            C::Last => last,
            C::PageUp => index.saturating_sub(10),
            C::PageDown => (index + 10).min(last),
            _ => return,
        };
        self.global_mut().select_index(target);
    }

    fn load_older_global(&mut self) {
        if self
            .shell()
            .all_worklogs()
            .is_some_and(|state| state.pagination_invalidated)
        {
            self.shell_mut().info("Refresh to load older worklogs");
            return;
        }
        let Some(cursor) = self
            .shell()
            .all_worklogs()
            .and_then(AllWorklogsState::next_cursor)
        else {
            self.shell_mut().info("No older worklogs");
            return;
        };
        match self.application_mut().all_worklogs(Some(&cursor)) {
            Ok(page) => {
                let count = page.worklogs.len();
                self.global_mut().append(page);
                self.sync_from_application(false);
                self.shell_mut().info(if count == 0 {
                    "No older worklogs".to_owned()
                } else {
                    format!("Loaded {count} older worklogs")
                });
            }
            Err(error)
                if error.failure().category()
                    == ApplicationFailureCategory::WorklogHistoryChanged =>
            {
                self.reload_global_after_change();
            }
            Err(error) => {
                self.sync_from_application(false);
                self.shell_mut().error(application_error_text(&error));
            }
        }
    }

    fn refresh_global(&mut self) {
        let keep = self
            .shell()
            .all_worklogs()
            .and_then(AllWorklogsState::selected_id);
        match self.application_mut().all_worklogs(None) {
            Ok(page) => {
                self.global_mut().replace(page, keep);
                self.sync_from_application(false);
                self.shell_mut().info("Refreshed");
            }
            Err(error) => self.shell_mut().error(application_error_text(&error)),
        }
    }

    fn reload_global_after_change(&mut self) {
        let keep = self
            .shell()
            .all_worklogs()
            .and_then(AllWorklogsState::selected_id);
        match self.application_mut().all_worklogs(None) {
            Ok(page) => {
                self.global_mut().replace(page, keep);
                self.sync_from_application(false);
                self.shell_mut().info("Worklogs changed and were refreshed");
            }
            Err(error) => {
                self.global_mut().mark_unavailable();
                self.sync_from_application(false);
                self.shell_mut().error(format!(
                    "Worklogs changed, but refresh failed: {}",
                    application_error_text(&error)
                ));
            }
        }
    }

    fn open_global_move(&mut self) {
        let Some(worklog) = self
            .shell()
            .all_worklogs()
            .and_then(AllWorklogsState::selected_worklog)
            .cloned()
        else {
            return;
        };
        let candidates = self
            .catalog()
            .tasks(TaskView::Active)
            .iter()
            .filter(|task| task.id() != worklog.task_id())
            .map(|task| {
                MoveCandidate::new(
                    task.id(),
                    task.name().to_string(),
                    self.catalog().search_rank(task.id()),
                )
            })
            .collect();
        self.global_mut().move_draft = Some(MoveDraft::new(worklog, candidates));
        self.shell_mut().info("Choose a destination task");
    }

    fn edit_global_move(&mut self, edit: impl FnOnce(&mut MoveDraft)) {
        if let Some(draft) = self.global_mut().move_draft.as_mut() {
            edit(draft);
        }
    }

    fn confirm_global_move(&mut self) {
        let Some((source, destination_task_id, destination_name)) = self
            .shell()
            .all_worklogs()
            .and_then(|state| state.move_draft.as_ref())
            .and_then(|draft| {
                let destination = draft.selected_task_id()?;
                let name = draft
                    .results()
                    .find(|candidate| candidate.id() == destination)?
                    .name()
                    .to_owned();
                Some((draft.worklog().clone(), destination, name))
            })
        else {
            return;
        };
        match self.application_mut().move_worklog(
            source.id(),
            source.task_id(),
            source.times(),
            destination_task_id,
        ) {
            Ok(moved) => {
                self.global_mut().move_draft = None;
                self.global_mut().apply_move(moved);
                self.reload_tasks();
                self.sync_tracking_after_history_reload();
                self.shell_mut()
                    .info(format!("Moved worklog to \"{destination_name}\""));
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
                    self.global_mut().move_draft = None;
                    self.reload_global_after_change();
                } else {
                    self.shell_mut().error(application_error_text(&error));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use tracker_domain::{ActiveWorklog, TrackingState, Worklog, WorklogId};

    use crate::command::Command;
    use crate::screens::{Screen, TaskListCommand};
    use crate::test_support::{TestService, app_in_timezone, app_with_test_clock, at, task};

    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn worklog(task_id: tracker_domain::TaskId, start: i64) -> Worklog {
        Worklog::new(
            WorklogId::generate(),
            task_id,
            at(start),
            Some(at(start + 1)),
        )
        .unwrap()
    }

    #[test]
    fn tabs_wrap_and_preserve_the_saved_task_selection() {
        let alpha = task(1, "alpha");
        let beta = task(2, "beta");
        let service = TestService::with_tasks(vec![alpha, beta.clone()]);
        let mut app = app_in_timezone(service, chrono_tz::UTC);
        app.handle(Command::TaskList(TaskListCommand::MoveDown));
        let saved = app.shell().task_list().selection();
        app.handle(Command::TaskList(TaskListCommand::ShowAllWorklogs));
        assert_eq!(app.shell().screen(), Screen::AllWorklogs);
        assert_eq!(app.app_view().status(), &crate::app::Status::Empty);
        assert_eq!(
            app.command_for(key(KeyCode::Tab)),
            Some(Command::AllWorklogs(C::ShowActive))
        );
        assert_eq!(
            app.command_for(key(KeyCode::BackTab)),
            Some(Command::AllWorklogs(C::ShowReports))
        );
        app.handle(Command::AllWorklogs(C::ShowActive));
        assert_eq!(app.shell().screen(), Screen::TaskList);
        assert_eq!(app.shell().task_list().selection(), saved);
    }

    #[test]
    fn older_pages_and_vim_navigation_keep_stable_selection() {
        let source = task(1, "source");
        let mut service = TestService::with_tasks(vec![source.clone()]);
        service.authoritative_worklogs = (0..51).map(|start| worklog(source.id(), start)).collect();
        let mut app = app_in_timezone(service, chrono_tz::UTC);
        app.handle(Command::TaskList(TaskListCommand::ShowAllWorklogs));
        assert_eq!(app.shell().all_worklogs().unwrap().worklogs().len(), 50);
        app.handle(Command::AllWorklogs(C::FocusRows));
        app.handle(Command::AllWorklogs(C::LoadOlder));
        assert_eq!(app.shell().all_worklogs().unwrap().worklogs().len(), 51);
        app.handle(Command::AllWorklogs(C::Last));
        assert_eq!(
            app.shell().all_worklogs().unwrap().selected_index(),
            Some(50)
        );
        assert_eq!(
            app.command_for(key(KeyCode::Char('g'))),
            Some(Command::AllWorklogs(C::GPrefix))
        );
        app.handle(Command::AllWorklogs(C::GPrefix));
        app.handle(Command::AllWorklogs(C::First));
        assert_eq!(
            app.shell().all_worklogs().unwrap().selected_index(),
            Some(0)
        );
    }

    #[test]
    fn page_motions_move_ten_rows_and_empty_pages_ignore_jumps() {
        let source = task(1, "source");
        let mut service = TestService::with_tasks(vec![source.clone()]);
        service.authoritative_worklogs = (0..25).map(|start| worklog(source.id(), start)).collect();
        let mut app = app_in_timezone(service, chrono_tz::UTC);
        app.handle(Command::TaskList(TaskListCommand::ShowAllWorklogs));
        app.handle(Command::AllWorklogs(C::FocusRows));
        app.handle(Command::AllWorklogs(C::PageDown));
        assert_eq!(
            app.shell().all_worklogs().unwrap().selected_index(),
            Some(10)
        );
        app.handle(Command::AllWorklogs(C::PageDown));
        assert_eq!(
            app.shell().all_worklogs().unwrap().selected_index(),
            Some(20)
        );
        app.handle(Command::AllWorklogs(C::PageUp));
        assert_eq!(
            app.shell().all_worklogs().unwrap().selected_index(),
            Some(10)
        );
        app.handle(Command::AllWorklogs(C::Last));
        assert_eq!(
            app.shell().all_worklogs().unwrap().selected_index(),
            Some(24)
        );
        app.handle(Command::AllWorklogs(C::First));
        assert_eq!(
            app.shell().all_worklogs().unwrap().selected_index(),
            Some(0)
        );

        let selected = app.shell().all_worklogs().unwrap().selected_id();
        app.shell_mut().all_worklogs_mut().unwrap().available = false;
        app.handle(Command::AllWorklogs(C::PageDown));
        assert_eq!(app.shell().all_worklogs().unwrap().selected_id(), selected);

        let mut empty = app_in_timezone(TestService::with_tasks(vec![]), chrono_tz::UTC);
        empty.handle(Command::TaskList(TaskListCommand::ShowAllWorklogs));
        empty.handle(Command::AllWorklogs(C::PageDown));
        assert_eq!(empty.shell().all_worklogs().unwrap().selected_id(), None);
    }

    #[test]
    fn g_prefix_ends_after_the_next_row_command() {
        let source = task(1, "source");
        let mut service = TestService::with_tasks(vec![source.clone()]);
        service.authoritative_worklogs = (0..3).map(|start| worklog(source.id(), start)).collect();
        let mut app = app_in_timezone(service, chrono_tz::UTC);
        app.handle(Command::TaskList(TaskListCommand::ShowAllWorklogs));
        app.handle(Command::AllWorklogs(C::FocusRows));
        app.handle(Command::AllWorklogs(C::GPrefix));
        assert!(app.shell().all_worklogs().unwrap().g_prefix);
        app.handle(Command::AllWorklogs(C::MoveDown));
        assert!(!app.shell().all_worklogs().unwrap().g_prefix);
        assert_eq!(
            app.shell().all_worklogs().unwrap().selected_index(),
            Some(1)
        );
    }

    #[test]
    fn older_page_refreshes_after_history_changes_and_other_errors_leave_rows_visible() {
        let source = task(1, "source");
        let mut service = TestService::with_tasks(vec![source.clone()]);
        service.authoritative_worklogs = (0..51).map(|start| worklog(source.id(), start)).collect();
        let spy = service.spy();
        let mut app = app_in_timezone(service, chrono_tz::UTC);
        app.handle(Command::TaskList(TaskListCommand::ShowAllWorklogs));
        spy.set_global_worklog_error(
            tracker_application::ApplicationError::worklog_history_changed(source.id()),
        );
        app.handle(Command::AllWorklogs(C::LoadOlder));
        assert_eq!(spy.global_worklog_reads(), 3);
        assert_eq!(
            crate::test_support::text(app.app_view().status()),
            "Worklogs changed and were refreshed"
        );
        assert_eq!(app.shell().all_worklogs().unwrap().worklogs().len(), 50);

        spy.set_global_worklog_error(TestService::failure());
        app.handle(Command::AllWorklogs(C::LoadOlder));
        assert_eq!(spy.global_worklog_reads(), 4);
        assert!(matches!(
            app.app_view().status(),
            crate::app::Status::Error(_)
        ));
        assert_eq!(app.shell().all_worklogs().unwrap().worklogs().len(), 50);
    }

    #[test]
    fn backspace_changes_the_move_query_only_when_search_has_focus() {
        let source = task(1, "source");
        let destination = task(2, "destination");
        let mut service = TestService::with_tasks(vec![source.clone(), destination]);
        service.authoritative_worklogs = vec![worklog(source.id(), 10)];
        let mut app = app_in_timezone(service, chrono_tz::UTC);
        app.handle(Command::TaskList(TaskListCommand::ShowAllWorklogs));
        app.handle(Command::AllWorklogs(C::OpenMove));
        app.handle(Command::AllWorklogs(C::InsertMoveQuery('d')));
        app.handle(Command::AllWorklogs(C::InsertMoveQuery('e')));
        app.handle(Command::AllWorklogs(C::ToggleMoveFocus));
        app.handle(Command::AllWorklogs(C::BackspaceMoveQuery));
        assert_eq!(
            app.shell()
                .all_worklogs()
                .unwrap()
                .move_draft
                .as_ref()
                .unwrap()
                .query(),
            "de"
        );
        app.handle(Command::AllWorklogs(C::ToggleMoveFocus));
        app.handle(Command::AllWorklogs(C::BackspaceMoveQuery));
        assert_eq!(
            app.shell()
                .all_worklogs()
                .unwrap()
                .move_draft
                .as_ref()
                .unwrap()
                .query(),
            "d"
        );
    }

    #[test]
    fn moving_an_archived_source_keeps_the_row_selected_and_changes_its_task() {
        let source = task(1, "archived source");
        let destination = task(2, "destination");
        let mut archived = source.clone();
        archived.archive(at(100));
        let entry = worklog(source.id(), 10);
        let mut service = TestService::with_tasks(vec![archived, destination.clone()]);
        service.authoritative_worklogs = vec![entry.clone()];
        let spy = service.spy();
        let mut app = app_in_timezone(service, chrono_tz::UTC);
        app.handle(Command::TaskList(TaskListCommand::ShowAllWorklogs));
        app.handle(Command::AllWorklogs(C::FocusRows));
        app.handle(Command::AllWorklogs(C::OpenMove));
        assert_eq!(
            app.shell()
                .all_worklogs()
                .unwrap()
                .move_draft
                .as_ref()
                .unwrap()
                .selected_task_id(),
            Some(destination.id())
        );
        assert_eq!(
            app.command_for(key(KeyCode::Tab)),
            Some(Command::AllWorklogs(C::ToggleMoveFocus))
        );
        app.handle(Command::AllWorklogs(C::ConfirmMove));
        let state = app.shell().all_worklogs().unwrap();
        assert!(state.move_draft.is_none());
        assert_eq!(state.selected_id(), Some(entry.id()));
        assert_eq!(
            state.selected_worklog().unwrap().task_id(),
            destination.id()
        );
        assert_eq!(spy.move_calls().len(), 1);
    }

    #[test]
    fn the_move_dialog_cancel_returns_to_the_same_rows_and_focus() {
        let source = task(1, "source");
        let destination = task(2, "destination");
        let mut service = TestService::with_tasks(vec![source.clone(), destination]);
        let entry = worklog(source.id(), 10);
        service.authoritative_worklogs = vec![entry.clone()];
        let mut app = app_in_timezone(service, chrono_tz::UTC);
        app.handle(Command::TaskList(TaskListCommand::ShowAllWorklogs));
        app.handle(Command::AllWorklogs(C::FocusRows));
        app.handle(Command::AllWorklogs(C::OpenMove));
        app.handle(Command::AllWorklogs(C::CancelMove));
        let state = app.shell().all_worklogs().unwrap();
        assert_eq!(state.focus, AllWorklogsFocus::Rows);
        assert_eq!(state.selected_id(), Some(entry.id()));
        assert!(state.move_draft.is_none());
    }

    #[test]
    fn moving_a_running_worklog_keeps_the_monotonic_elapsed_time() {
        let source = task(1, "source");
        let destination = task(2, "destination");
        let entry = Worklog::begin(WorklogId::generate(), source.id(), at(100));
        let mut service = TestService::with_tasks(vec![source, destination.clone()]);
        service.authoritative_worklogs = vec![entry.clone()];
        service.tracking = TrackingState::Running {
            worklog: ActiveWorklog::begin(entry.id(), entry.task_id(), entry.start()),
        };
        let (mut app, clock) = app_with_test_clock(service, chrono_tz::UTC, at(100));
        app.handle(Command::TaskList(TaskListCommand::ShowAllWorklogs));
        app.handle(Command::AllWorklogs(C::FocusRows));
        app.handle(Command::AllWorklogs(C::OpenMove));
        clock.advance_monotonic(Duration::from_secs(600));
        clock.set_wall_clock(at(10_000));

        app.handle(Command::AllWorklogs(C::ConfirmMove));

        assert_eq!(app.app_view().elapsed(), Some(Duration::from_secs(600)));
        assert_eq!(app.app_view().active_task_name(), Some("destination"));
        assert_eq!(
            app.shell().all_worklogs().unwrap().selected_id(),
            Some(entry.id())
        );
    }

    #[test]
    fn moving_a_loaded_older_worklog_preserves_selection_without_using_a_stale_cursor() {
        let source = task(1, "source");
        let destination = task(2, "destination");
        let mut service = TestService::with_tasks(vec![source.clone(), destination.clone()]);
        service.authoritative_worklogs =
            (0..151).map(|start| worklog(source.id(), start)).collect();
        let spy = service.spy();
        let mut app = app_in_timezone(service, chrono_tz::UTC);
        app.handle(Command::TaskList(TaskListCommand::ShowAllWorklogs));
        app.handle(Command::AllWorklogs(C::FocusRows));
        for _ in 0..2 {
            app.handle(Command::AllWorklogs(C::LoadOlder));
        }
        assert_eq!(app.shell().all_worklogs().unwrap().worklogs().len(), 150);
        app.handle(Command::AllWorklogs(C::Last));
        let selected = app.shell().all_worklogs().unwrap().selected_id();
        app.handle(Command::AllWorklogs(C::OpenMove));
        let before = spy.global_worklog_reads();

        app.handle(Command::AllWorklogs(C::ConfirmMove));

        let state = app.shell().all_worklogs().unwrap();
        assert_eq!(spy.global_worklog_reads(), before);
        assert_eq!(state.worklogs().len(), 150);
        assert_eq!(state.selected_id(), selected);
        assert_eq!(
            state.selected_worklog().unwrap().task_id(),
            destination.id()
        );
        assert!(state.next_cursor().is_none());
        assert!(state.pagination_invalidated);
        app.handle(Command::AllWorklogs(C::LoadOlder));
        assert_eq!(spy.global_worklog_reads(), before);
        assert_eq!(app.shell().all_worklogs().unwrap().worklogs().len(), 150);
        assert_eq!(
            crate::test_support::text(app.app_view().status()),
            "Refresh to load older worklogs"
        );
        app.handle(Command::AllWorklogs(C::Refresh));
        assert_eq!(spy.global_worklog_reads(), before + 1);
        assert_eq!(app.shell().all_worklogs().unwrap().worklogs().len(), 50);
        assert!(app.shell().all_worklogs().unwrap().next_cursor().is_some());
    }
}

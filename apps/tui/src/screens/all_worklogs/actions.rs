use tracker_application::ApplicationFailureCategory;

use crate::app::AppState;
use crate::application_request::{ApplicationOutcome, ApplicationRequest};
use crate::screens::worklog_history::move_worklog::{MoveCandidate, MoveDraft};
use crate::screens::{Screen, TaskView};
use crate::support::errors::application_error_text;

use super::{AllWorklogsCommand as C, AllWorklogsFocus, AllWorklogsState};

impl AppState {
    pub(crate) fn open_all_worklogs(&mut self) {
        if !matches!(self.shell().screen(), Screen::TaskList | Screen::Reports) {
            return;
        }
        let source_screen = self.shell().screen();
        let screen_generation = self.shell().screen_generation();
        let task_view_generation =
            (source_screen == Screen::TaskList).then(|| self.shell().task_list().view_generation());
        let source_report_context = self
            .shell()
            .report()
            .map(|report| (report.session_id, report.period_generation));
        self.enqueue(
            ApplicationRequest::AllWorklogs { after: None },
            move |app, completed| {
                if app.shell().screen() != source_screen
                    || app.shell().screen_generation() != screen_generation
                    || (source_screen == Screen::TaskList
                        && Some(app.shell().task_list().view_generation()) != task_view_generation)
                    || (source_screen == Screen::Reports
                        && app
                            .shell()
                            .report()
                            .map(|report| (report.session_id, report.period_generation))
                            != source_report_context)
                {
                    return;
                }
                let ApplicationOutcome::GlobalWorklogPage(result) = completed.outcome else {
                    unreachable!("global history request returns a page")
                };
                match result {
                    Ok(page) => {
                        app.shell_mut()
                            .open_all_worklogs(AllWorklogsState::new(page));
                        app.sync_from_snapshot(
                            completed.snapshot.items,
                            completed.snapshot.tracking,
                            false,
                        );
                    }
                    Err(error) => app.shell_mut().error(application_error_text(&error)),
                }
            },
        );
    }

    pub(crate) fn handle_all_worklogs_command(&mut self, command: C) {
        match command {
            C::ShowArchived => {
                let first = self
                    .catalog()
                    .tasks(TaskView::Archived)
                    .first()
                    .map(|task| task.id());
                self.shell_mut().leave_all_worklogs_for_tasks();
                self.shell_mut()
                    .task_list_mut()
                    .show(TaskView::Archived, first);
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
                let state = self.global_mut();
                state.move_draft_generation = state.move_draft_generation.wrapping_add(1);
                state.move_draft = None;
                self.shell_mut().info("Move cancelled");
            }
        }
        if command != C::GPrefix
            && let Some(state) = self.shell_mut().all_worklogs_mut()
        {
            state.g_prefix = false;
        }
    }

    fn global_mut(&mut self) -> &mut AllWorklogsState {
        self.shell_mut()
            .all_worklogs_mut()
            .expect("worklogs are open")
    }

    fn global_session_matches(&self, session: u64) -> bool {
        self.shell().screen() == Screen::AllWorklogs
            && self
                .shell()
                .all_worklogs()
                .is_some_and(|state| state.session_id() == session)
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
        let state = self.shell().all_worklogs().expect("worklogs are open");
        if state.loading_cursor == Some(cursor) {
            return;
        }
        let session = state.session_id();
        let page_generation = state.page_generation;
        if self.enqueue(
            ApplicationRequest::AllWorklogs {
                after: Some(cursor),
            },
            move |app, completed| {
                let current = app.global_session_matches(session);
                let ApplicationOutcome::GlobalWorklogPage(result) = completed.outcome else {
                    unreachable!("global history request returns a page")
                };
                app.sync_from_snapshot(
                    completed.snapshot.items,
                    completed.snapshot.tracking,
                    false,
                );
                if !current {
                    return;
                }
                let current_page = app.shell().all_worklogs().is_some_and(|state| {
                    state.page_generation == page_generation && state.next_cursor() == Some(cursor)
                });
                if app.global_mut().loading_cursor == Some(cursor) {
                    app.global_mut().loading_cursor = None;
                }
                if !current_page {
                    return;
                }
                match result {
                    Ok(page) => {
                        let count = page.worklogs.len();
                        app.global_mut().append(page);
                        app.shell_mut().info(if count == 0 {
                            "No older worklogs".to_owned()
                        } else {
                            format!("Loaded {count} older worklogs")
                        });
                    }
                    Err(error)
                        if error.failure().category()
                            == ApplicationFailureCategory::WorklogHistoryChanged =>
                    {
                        app.reload_global_after_change();
                    }
                    Err(error) => {
                        app.shell_mut().error(application_error_text(&error));
                    }
                }
            },
        ) {
            self.global_mut().loading_cursor = Some(cursor);
        }
    }

    fn refresh_global(&mut self) {
        let keep = self
            .shell()
            .all_worklogs()
            .and_then(AllWorklogsState::selected_id);
        let session = self.global_mut().session_id();
        self.enqueue(
            ApplicationRequest::AllWorklogs { after: None },
            move |app, completed| {
                let current = app.global_session_matches(session);
                let ApplicationOutcome::GlobalWorklogPage(result) = completed.outcome else {
                    unreachable!("global history request returns a page")
                };
                if !current {
                    return;
                }
                match result {
                    Ok(page) => {
                        app.global_mut().replace(page, keep);
                        app.sync_from_snapshot(
                            completed.snapshot.items,
                            completed.snapshot.tracking,
                            false,
                        );
                        app.shell_mut().info("Refreshed");
                    }
                    Err(error) => app.shell_mut().error(application_error_text(&error)),
                }
            },
        );
    }

    fn reload_global_after_change(&mut self) {
        let keep = self
            .shell()
            .all_worklogs()
            .and_then(AllWorklogsState::selected_id);
        let session = self.global_mut().session_id();
        self.enqueue(
            ApplicationRequest::AllWorklogs { after: None },
            move |app, completed| {
                let current = app.global_session_matches(session);
                let ApplicationOutcome::GlobalWorklogPage(result) = completed.outcome else {
                    unreachable!("global history request returns a page")
                };
                app.sync_from_snapshot(
                    completed.snapshot.items,
                    completed.snapshot.tracking,
                    false,
                );
                if !current {
                    return;
                }
                match result {
                    Ok(page) => {
                        app.global_mut().replace(page, keep);
                        app.shell_mut().info("Worklogs changed and were refreshed");
                    }
                    Err(error) => {
                        app.global_mut().mark_unavailable();
                        app.shell_mut().error(format!(
                            "Worklogs changed, but refresh failed: {}",
                            application_error_text(&error)
                        ));
                    }
                }
            },
        );
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
        let state = self.global_mut();
        state.move_draft_generation = state.move_draft_generation.wrapping_add(1);
        state.move_draft = Some(MoveDraft::new(worklog, candidates));
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
        let session = self.global_mut().session_id();
        let draft_generation = self.global_mut().move_draft_generation;
        self.enqueue(
            ApplicationRequest::MoveWorklog {
                id: source.id(),
                expected_source_task_id: source.task_id(),
                expected: source.times(),
                destination_task_id,
            },
            move |app, completed| {
                let current = app.global_session_matches(session);
                let ApplicationOutcome::Worklog(result) = completed.outcome else {
                    unreachable!("move request returns a worklog")
                };
                if result.is_ok() {
                    app.replace_items(completed.snapshot.items);
                    app.sync_tracking_after_history_reload(completed.snapshot.tracking);
                } else {
                    app.sync_from_snapshot(
                        completed.snapshot.items,
                        completed.snapshot.tracking,
                        false,
                    );
                }
                if !current {
                    return;
                }
                let same_draft = app.global_mut().move_draft_generation == draft_generation;
                match result {
                    Ok(moved) => {
                        if same_draft {
                            app.global_mut().move_draft = None;
                        }
                        app.global_mut().apply_move(moved);
                        app.shell_mut()
                            .info(format!("Moved worklog to \"{destination_name}\""));
                    }
                    Err(error) => {
                        let failure = error.failure();
                        if failure.recovery_failed() {
                            app.shell_mut().error(application_error_text(&error));
                            return;
                        }
                        let stale = matches!(
                            failure.category(),
                            ApplicationFailureCategory::WorklogChanged
                                | ApplicationFailureCategory::WorklogNotFound
                        );
                        let destination_unavailable = app
                            .catalog()
                            .task(destination_task_id)
                            .is_none_or(|task| task.is_archived());
                        if stale || destination_unavailable {
                            if same_draft {
                                app.global_mut().move_draft = None;
                            }
                            app.reload_global_after_change();
                        } else {
                            app.shell_mut().error(application_error_text(&error));
                        }
                    }
                }
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use tracker_application::{
        GlobalWorklogCursor, GlobalWorklogPage, TaskListItem, TrackerSnapshot,
    };
    use tracker_domain::{ActiveWorklog, TrackingState, Worklog, WorklogId};

    use crate::app::AppState;
    use crate::application_request::{ApplicationOutcome, ApplicationSnapshot, CompletedRequest};
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

    fn page(worklogs: Vec<Worklog>) -> GlobalWorklogPage {
        GlobalWorklogPage {
            worklogs,
            snapshot: TrackerSnapshot {
                task_items: Vec::new(),
                active_worklog: None,
            },
            next_cursor: None,
        }
    }

    fn page_with_cursor(worklogs: Vec<Worklog>, cursor: GlobalWorklogCursor) -> GlobalWorklogPage {
        let mut page = page(worklogs);
        page.next_cursor = Some(cursor);
        page
    }

    fn complete_global(
        state: &mut AppState,
        effect: crate::app::AppEffect,
        page: GlobalWorklogPage,
    ) {
        let request = effect.request.clone();
        state.complete_effect(
            effect,
            CompletedRequest {
                request,
                outcome: ApplicationOutcome::GlobalWorklogPage(Ok(page)),
                snapshot: ApplicationSnapshot {
                    items: Vec::new(),
                    tracking: TrackingState::Idle,
                },
            },
        );
    }

    #[test]
    fn repeated_load_older_queues_one_cursor_and_appends_one_page() {
        let mut state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
        let task_id = tracker_domain::TaskId::generate();
        let first = worklog(task_id, 1);
        let cursor = GlobalWorklogCursor {
            start: first.start(),
            id: first.id(),
            revision: 1,
        };
        state
            .shell_mut()
            .open_all_worklogs(AllWorklogsState::new(page_with_cursor(vec![first], cursor)));
        state.load_older_global();
        let effect = state.take_effect().expect("older page request");
        state.load_older_global();
        complete_global(&mut state, effect, page(vec![worklog(task_id, 2)]));
        assert_eq!(state.shell().all_worklogs().unwrap().worklogs().len(), 2);
        assert_eq!(state.shell().all_worklogs().unwrap().loading_cursor, None);
        assert!(state.take_effect().is_none());
    }

    #[test]
    fn replaced_page_with_the_same_cursor_discards_a_pending_older_page() {
        let mut state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
        let task_id = tracker_domain::TaskId::generate();
        let first = worklog(task_id, 1);
        let cursor = GlobalWorklogCursor {
            start: first.start(),
            id: first.id(),
            revision: 1,
        };
        state
            .shell_mut()
            .open_all_worklogs(AllWorklogsState::new(page_with_cursor(vec![first], cursor)));
        state.load_older_global();
        let effect = state.take_effect().expect("older page request");
        let replacement = worklog(task_id, 2);
        state
            .global_mut()
            .replace(page_with_cursor(vec![replacement.clone()], cursor), None);
        complete_global(&mut state, effect, page(vec![worklog(task_id, 3)]));
        let rows = state.shell().all_worklogs().unwrap().worklogs();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id(), replacement.id());
    }

    #[test]
    fn older_page_queued_before_refresh_result_is_ignored() {
        let mut state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
        let task_id = tracker_domain::TaskId::generate();
        let first = worklog(task_id, 1);
        let cursor = GlobalWorklogCursor {
            start: first.start(),
            id: first.id(),
            revision: 1,
        };
        state
            .shell_mut()
            .open_all_worklogs(AllWorklogsState::new(page_with_cursor(vec![first], cursor)));
        state.refresh_global();
        state.load_older_global();
        let refresh = state.take_effect().expect("refresh request");
        let refreshed = worklog(task_id, 2);
        complete_global(&mut state, refresh, page(vec![refreshed.clone()]));
        let older = state.take_effect().expect("queued older page request");
        complete_global(&mut state, older, page(vec![worklog(task_id, 3)]));
        let rows = state.shell().all_worklogs().unwrap().worklogs();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id(), refreshed.id());
    }

    #[test]
    fn completed_move_keeps_a_newer_move_dialog_open() {
        let source = task(1, "source");
        let destination = task(2, "destination");
        let destination_id = destination.id();
        let items = vec![source.clone(), destination.clone()]
            .into_iter()
            .map(|task| TaskListItem {
                task,
                latest_work_start: None,
            })
            .collect();
        let mut state = AppState::load_from_snapshot(items, TrackingState::Idle);
        let entry = worklog(source.id(), 1);
        state
            .shell_mut()
            .open_all_worklogs(AllWorklogsState::new(page(vec![entry.clone()])));
        state.open_global_move();
        state.confirm_global_move();
        let effect = state.take_effect().expect("move request");
        state.handle_all_worklogs_command(C::CancelMove);
        state.open_global_move();
        let newer_generation = state.shell().all_worklogs().unwrap().move_draft_generation;
        let request = effect.request.clone();
        state.complete_effect(
            effect,
            CompletedRequest {
                request,
                outcome: ApplicationOutcome::Worklog(Ok(entry.moved_to(destination_id).unwrap())),
                snapshot: ApplicationSnapshot {
                    items: vec![source, destination]
                        .into_iter()
                        .map(|task| TaskListItem {
                            task,
                            latest_work_start: None,
                        })
                        .collect(),
                    tracking: TrackingState::Idle,
                },
            },
        );
        let state = state.shell().all_worklogs().unwrap();
        assert_eq!(state.move_draft_generation, newer_generation);
        assert!(state.move_draft.is_some());
        assert_eq!(state.selected_worklog().unwrap().task_id(), destination_id);
    }

    #[test]
    fn old_refresh_does_not_replace_a_new_global_session() {
        let mut state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
        let task_id = tracker_domain::TaskId::generate();
        state
            .shell_mut()
            .open_all_worklogs(AllWorklogsState::new(page(vec![worklog(task_id, 1)])));
        state.refresh_global();
        let effect = state.take_effect().expect("refresh request");
        let current = worklog(task_id, 2);
        state.shell_mut().leave_all_worklogs_for_tasks();
        state
            .shell_mut()
            .open_all_worklogs(AllWorklogsState::new(page(vec![current.clone()])));
        state.complete_effect(
            effect,
            CompletedRequest {
                request: ApplicationRequest::AllWorklogs { after: None },
                outcome: ApplicationOutcome::GlobalWorklogPage(Ok(page(vec![worklog(task_id, 3)]))),
                snapshot: ApplicationSnapshot {
                    items: Vec::new(),
                    tracking: TrackingState::Idle,
                },
            },
        );
        assert_eq!(
            state.shell().all_worklogs().unwrap().worklogs()[0].id(),
            current.id()
        );
    }

    #[test]
    fn pending_open_does_not_reopen_after_leaving_and_returning_to_tasks() {
        let mut state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
        state.open_all_worklogs();
        let effect = state.take_effect().expect("global open request");
        let original_view_generation = state.shell().task_list().view_generation();
        let original_screen_generation = state.shell().screen_generation();
        state.shell_mut().open_reports(chrono::Utc::now());
        state.shell_mut().leave_reports(TaskView::Active, None);
        assert_eq!(
            state.shell().task_list().view_generation(),
            original_view_generation
        );
        assert_ne!(
            state.shell().screen_generation(),
            original_screen_generation
        );
        complete_global(&mut state, effect, page(Vec::new()));
        assert_eq!(state.shell().screen(), Screen::TaskList);
    }

    #[test]
    fn pending_open_does_not_reopen_after_switching_task_views() {
        let mut state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
        state.open_all_worklogs();
        let effect = state.take_effect().expect("global open request");
        let original_screen_generation = state.shell().screen_generation();
        let original_view_generation = state.shell().task_list().view_generation();
        state
            .shell_mut()
            .task_list_mut()
            .show(TaskView::Archived, None);
        state
            .shell_mut()
            .task_list_mut()
            .show(TaskView::Active, None);
        assert_eq!(
            state.shell().screen_generation(),
            original_screen_generation
        );
        assert_ne!(
            state.shell().task_list().view_generation(),
            original_view_generation
        );
        complete_global(&mut state, effect, page(Vec::new()));
        assert_eq!(state.shell().screen(), Screen::TaskList);
    }

    #[test]
    fn pending_open_from_reports_does_not_override_a_new_period() {
        let mut state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
        state.shell_mut().open_reports(chrono::Utc::now());
        state.open_all_worklogs();
        let effect = state.take_effect().expect("global open request");
        let screen_generation = state.shell().screen_generation();
        let report_session = state.shell().report().unwrap().session_id;
        assert!(state.shell_mut().report_mut().unwrap().step(-1));
        assert_eq!(state.shell().screen_generation(), screen_generation);
        assert_eq!(state.shell().report().unwrap().session_id, report_session);
        complete_global(&mut state, effect, page(Vec::new()));
        assert_eq!(state.shell().screen(), Screen::Reports);
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
            Some(Command::AllWorklogs(C::ShowReports))
        );
        assert_eq!(
            app.command_for(key(KeyCode::BackTab)),
            Some(Command::AllWorklogs(C::ShowArchived))
        );
        app.handle(Command::AllWorklogs(C::ShowReports));
        assert_eq!(app.shell().screen(), Screen::Reports);
        app.handle(Command::Reports(crate::screens::ReportCommand::ShowActive));
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

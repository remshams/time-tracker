//! Global TUI presentation controller.

mod shell_state;
mod task_catalog;
mod tracking_session;
mod view;

use chrono_tz::Tz;
use crossterm::event::KeyEvent;
use tracker_application::{TaskOrdering, TrackerApplicationService};

use crate::command::Command;
use crate::screens::Screen;
use crate::screens::task_list::TaskListState;
use crate::support::timestamps::startup_timezone;

use self::shell_state::ShellState;
use self::task_catalog::TaskCatalog;
#[cfg(test)]
pub(crate) use self::tracking_session::TestClock;
use self::tracking_session::TrackingSession;
pub(crate) use self::view::AppView;

/// The most recent message shown in the status line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Info(String),
    Error(String),
}

/// Coordinates service calls and the presentation state owners.
pub struct App<S: TrackerApplicationService> {
    application: S,
    catalog: TaskCatalog,
    tracking: TrackingSession,
    shell: ShellState,
}

impl<S: TrackerApplicationService> App<S> {
    /// Builds presentation state from an already loaded application service.
    pub fn load(application: S) -> Self {
        let (timezone, timezone_status) = startup_timezone();
        Self::load_with_timezone(application, timezone, timezone_status)
    }

    fn load_with_timezone(application: S, timezone: Tz, timezone_status: Option<&str>) -> Self {
        let catalog = TaskCatalog::new(application.tasks(TaskOrdering::default()));
        let tracking = TrackingSession::new(application.current_tracking().clone());
        Self::assemble(application, catalog, tracking, timezone, timezone_status)
    }

    fn assemble(
        application: S,
        catalog: TaskCatalog,
        tracking: TrackingSession,
        timezone: Tz,
        timezone_status: Option<&str>,
    ) -> Self {
        let task_list = TaskListState::new(
            catalog
                .tasks(crate::screens::TaskView::Active)
                .first()
                .map(|task| task.id()),
        );
        let status = match timezone_status {
            Some(message) => Status::Error(message.to_owned()),
            None if tracking.elapsed().is_some() => {
                Status::Info("Recovered the previous active timer".to_owned())
            }
            None => Status::Info("Ready".to_owned()),
        };
        Self {
            application,
            catalog,
            tracking,
            shell: ShellState::new(status, task_list, timezone),
        }
    }

    #[cfg(test)]
    pub(crate) fn load_in_timezone(application: S, timezone: Tz) -> Self {
        Self::load_with_timezone(application, timezone, None)
    }

    #[cfg(test)]
    pub(crate) fn load_with_test_clock(
        application: S,
        timezone: Tz,
        wall_clock: chrono::DateTime<chrono::Utc>,
    ) -> (Self, TestClock) {
        let catalog = TaskCatalog::new(application.tasks(TaskOrdering::default()));
        let (tracking, clock) =
            TrackingSession::with_test_clock(application.current_tracking().clone(), wall_clock);
        (
            Self::assemble(application, catalog, tracking, timezone, None),
            clock,
        )
    }

    pub(crate) fn handle(&mut self, command: Command) {
        match (self.shell.screen(), command) {
            (_, Command::Quit) => self.shell.quit(),
            (Screen::TaskList, Command::TaskList(command)) => {
                self.handle_task_list_command(command);
            }
            (Screen::WorklogHistory, Command::WorklogHistory(command)) => {
                self.handle_worklog_history_command(command);
            }
            (Screen::Reports, Command::Reports(command)) => self.handle_report_command(command),
            (Screen::TaskList, Command::WorklogHistory(_))
            | (Screen::TaskList, Command::Reports(_))
            | (Screen::Reports, Command::TaskList(_))
            | (Screen::Reports, Command::WorklogHistory(_))
            | (Screen::WorklogHistory, Command::Reports(_))
            | (Screen::WorklogHistory, Command::TaskList(_)) => {}
        }
    }

    pub fn is_running(&self) -> bool {
        self.shell.is_running()
    }

    pub(crate) fn expire_copy_confirmation(&mut self) {
        self.shell.expire_copy_confirmation();
    }

    pub(crate) fn app_view(&self) -> AppView<'_> {
        AppView::new(&self.catalog, &self.tracking, &self.shell)
    }

    pub(crate) fn command_for(&self, key: KeyEvent) -> Option<Command> {
        crate::screens::map_key(self.shell.input_state(), key)
    }

    pub(crate) fn application_mut(&mut self) -> &mut S {
        &mut self.application
    }

    pub(crate) fn catalog(&self) -> &TaskCatalog {
        &self.catalog
    }

    pub(crate) fn catalog_mut(&mut self) -> &mut TaskCatalog {
        &mut self.catalog
    }

    pub(crate) fn tracking(&self) -> &TrackingSession {
        &self.tracking
    }

    pub(crate) fn tracking_mut(&mut self) -> &mut TrackingSession {
        &mut self.tracking
    }

    pub(crate) fn shell(&self) -> &ShellState {
        &self.shell
    }

    pub(crate) fn shell_mut(&mut self) -> &mut ShellState {
        &mut self.shell
    }

    pub(crate) fn reload_tasks(&mut self) {
        let view = self.shell.task_list().view();
        let selected = self.shell.task_list().selection();
        let items = self.application.tasks(self.catalog.ordering());
        let resolved = self.catalog.reload(items, view, selected);
        let query = self.shell.task_list().search_query();
        let visible = self.catalog.visible_tasks(view, query);
        let resolved = resolved
            .filter(|id| visible.iter().any(|task| task.id() == *id))
            .or_else(|| visible.first().map(|task| task.id()));
        self.shell.task_list_mut().set_selection(resolved);
    }

    pub(crate) fn sync_from_application(&mut self, fresh_active: bool) {
        self.reload_tasks();
        self.tracking
            .sync(self.application.current_tracking().clone(), fresh_active);
    }
}

use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use tracker_application::{ApplicationFailureCategory, TrackerApplicationService, WorklogPage};
use tracker_domain::{TaskId, TrackingState, Worklog, WorklogId};

use crate::app::{App, Status};
use crate::command::Command;
use crate::screens::{Mode, Screen, ScreenState};
use crate::support::clock::ElapsedClock;
use crate::support::errors::application_error_text;

use super::{History, HistoryAvailability, WorklogHistoryMode, active_worklog_for_task};

impl<S: TrackerApplicationService> App<S> {
    pub(crate) fn handle_worklog_history_command(&mut self, command: Command) {
        match command {
            Command::MoveUp => self.move_history_up(),
            Command::MoveDown => self.move_history_down(),
            Command::LoadOlderWorklogs => self.load_older_worklogs(),
            Command::RefreshWorklogs => self.refresh_worklogs(),
            Command::OpenCorrection
            | Command::SwitchCorrectionField
            | Command::MoveCursorLeft
            | Command::MoveCursorRight
            | Command::Delete
            | Command::AdjustForwardFiveMinutes
            | Command::AdjustBackwardFiveMinutes
            | Command::AdjustForwardOneHour
            | Command::AdjustBackwardOneHour => self.handle_correction_command(command),
            Command::OpenDeletion => self.open_deletion(),
            Command::Insert(character) => self.insert_correction_character(character),
            Command::Backspace => self.backspace_correction_character(),
            Command::Confirm => match self.mode() {
                Mode::ConfirmDeletion { .. } => self.confirm_deletion(),
                Mode::Correction(_) => self.confirm_correction(),
                _ => {}
            },
            Command::Cancel => self.cancel_history_mode(),
            _ => {}
        }
    }

    pub fn history(&self) -> Option<&History> {
        match &self.screen {
            ScreenState::WorklogHistory(state) => Some(state.history()),
            ScreenState::TaskList(_) => None,
        }
    }

    pub(super) fn history_mut(&mut self) -> Option<&mut History> {
        match &mut self.screen {
            ScreenState::WorklogHistory(state) => Some(&mut state.history),
            ScreenState::TaskList(_) => None,
        }
    }

    pub fn history_selected_index(&self) -> Option<usize> {
        self.history()?.selected_index()
    }

    pub fn history_task_name(&self) -> Option<&str> {
        self.task_name_for(self.history()?.task_id)
    }

    pub fn local_time(&self, at: DateTime<Utc>) -> String {
        match self.frozen_offset {
            Some(offset) => crate::ui::local_time(at, &offset),
            None => crate::ui::local_time(at, &self.timezone),
        }
    }

    pub fn history_row_duration(&self, worklog: &Worklog) -> Duration {
        let Some(end) = worklog.end() else {
            return if self.active_worklog_id() == Some(worklog.id()) {
                self.clock
                    .as_ref()
                    .map_or(Duration::ZERO, ElapsedClock::elapsed)
            } else {
                Duration::ZERO
            };
        };
        (end - worklog.start()).to_std().unwrap_or(Duration::ZERO)
    }

    fn move_history_up(&mut self) {
        if self.mode() != Mode::Normal {
            return;
        }
        let Some(history) = self.history_mut() else {
            return;
        };
        if !history.is_available() {
            return;
        }
        let index = match history.selected_index() {
            None => history.worklogs.len().checked_sub(1),
            Some(0) => Some(0),
            Some(index) => Some(index - 1),
        };
        history.selected = index
            .and_then(|index| history.worklogs.get(index))
            .map(Worklog::id);
    }

    fn move_history_down(&mut self) {
        if self.mode() != Mode::Normal {
            return;
        }
        let Some(history) = self.history_mut() else {
            return;
        };
        if !history.is_available() {
            return;
        }
        if history.worklogs.is_empty() {
            history.selected = None;
            return;
        }
        let last = history.worklogs.len() - 1;
        let index = match history.selected_index() {
            None => 0,
            Some(index) => index.saturating_add(1).min(last),
        };
        history.selected = Some(history.worklogs[index].id());
    }

    fn cancel_history_mode(&mut self) {
        let ScreenState::WorklogHistory(state) = &mut self.screen else {
            return;
        };
        match state.mode {
            WorklogHistoryMode::Correction(_) => {
                state.mode = WorklogHistoryMode::Normal;
                self.status = Status::Info("Correction cancelled".to_owned());
            }
            WorklogHistoryMode::ConfirmDeletion { .. } => {
                state.mode = WorklogHistoryMode::Normal;
                self.status = Status::Info("Deletion cancelled".to_owned());
            }
            WorklogHistoryMode::Normal => {}
        }
    }

    fn load_older_worklogs(&mut self) {
        if self.mode() != Mode::Normal {
            return;
        }
        let Some(task_id) = self
            .history()
            .filter(|history| history.is_available())
            .map(|history| history.task_id)
        else {
            return;
        };
        let Some(cursor) = self.history().and_then(|history| history.next_cursor) else {
            self.status = Status::Info("No older worklogs".to_owned());
            return;
        };
        let baseline = self
            .history()
            .and_then(|history| history.active_worklog_baseline);
        match self.application.worklogs_for_task(task_id, Some(&cursor)) {
            Ok(page) => {
                let active = active_worklog_for_task(&page.snapshot.active_worklog, task_id);
                if active != baseline {
                    self.reload_newest_history_after_change(task_id);
                    return;
                }
                let loaded = page.worklogs.len();
                let next_cursor = page.next_cursor;
                if let Some(history) = self.history_mut() {
                    let select_first = history.worklogs.is_empty();
                    history.worklogs.extend(page.worklogs);
                    history.next_cursor = next_cursor;
                    if select_first {
                        history.selected = history.worklogs.first().map(Worklog::id);
                    }
                }
                self.sync_from_application(false);
                self.status = if loaded == 0 {
                    Status::Info("No older worklogs".to_owned())
                } else {
                    Status::Info(format!("Loaded {loaded} older worklogs"))
                };
            }
            Err(error)
                if error.failure().category()
                    == ApplicationFailureCategory::WorklogHistoryChanged =>
            {
                self.reload_newest_history_after_change(task_id);
            }
            Err(error) => {
                self.sync_from_application(false);
                self.status = Status::Error(application_error_text(&error));
            }
        }
    }

    fn refresh_worklogs(&mut self) {
        if self.mode() != Mode::Normal || self.screen() != Screen::WorklogHistory {
            return;
        }
        let Some(task_id) = self.history().map(|history| history.task_id) else {
            return;
        };
        let keep = self.history().and_then(|history| history.selected);
        let result = self.application.worklogs_for_task(task_id, None);
        self.sync_from_application(false);
        match result {
            Ok(page) => {
                self.replace_history_with_newest_page(task_id, keep, page);
                self.status = Status::Info("Refreshed".to_owned());
            }
            Err(error) => self.status = Status::Error(application_error_text(&error)),
        }
    }

    pub(super) fn reload_newest_history_after_change(&mut self, task_id: TaskId) {
        let keep = self.history().and_then(|history| history.selected);
        match self.application.worklogs_for_task(task_id, None) {
            Ok(page) => {
                self.replace_history_with_newest_page(task_id, keep, page);
                self.sync_from_application(false);
                self.status = Status::Info("History changed and was refreshed".to_owned());
            }
            Err(error) => {
                self.mark_history_unavailable();
                self.sync_from_application(false);
                self.status = Status::Error(format!(
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
        let selected = keep
            .filter(|id| page.worklogs.iter().any(|worklog| worklog.id() == *id))
            .or_else(|| page.worklogs.first().map(Worklog::id));
        let baseline = active_worklog_for_task(&page.snapshot.active_worklog, task_id);
        let Some(history) = self.history_mut() else {
            return;
        };
        *history = History {
            task_id,
            availability: HistoryAvailability::Available,
            worklogs: page.worklogs,
            next_cursor: page.next_cursor,
            active_worklog_baseline: baseline,
            selected,
        };
    }

    pub(super) fn mark_history_unavailable(&mut self) {
        let Some(history) = self.history_mut() else {
            return;
        };
        history.availability = HistoryAvailability::Unavailable;
        history.worklogs.clear();
        history.next_cursor = None;
    }

    pub(super) fn sync_tracking_after_history_reload(&mut self) {
        self.sync_tracking_after_history_reload_at(Utc::now(), Instant::now());
    }

    pub(crate) fn sync_tracking_after_history_reload_at(
        &mut self,
        wall_clock: DateTime<Utc>,
        monotonic_clock: Instant,
    ) {
        let tracking = self.application.current_tracking().clone();
        match (&self.tracking, &tracking) {
            (
                TrackingState::Running { worklog: current },
                TrackingState::Running {
                    worklog: final_worklog,
                },
            ) if current.id() == final_worklog.id() && current.start() == final_worklog.start() => {
            }
            (_, TrackingState::Idle) => self.clock = None,
            (_, TrackingState::Running { worklog }) => {
                self.clock = Some(ElapsedClock::at_anchor(
                    ElapsedClock::base_since(worklog.start(), wall_clock),
                    monotonic_clock,
                ));
            }
        }
        self.tracking = tracking;
    }
}

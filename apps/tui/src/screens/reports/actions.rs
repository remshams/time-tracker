use chrono::{DateTime, TimeDelta, Utc};

use crate::app::AppState;
use crate::application_request::{ApplicationOutcome, ApplicationRequest};
use crate::screens::reports::{ReportFocus, ReportMode, ReportPreset};
use crate::screens::{History, ReportCommand, Screen, TaskView};
use crate::support::errors::application_error_text;

impl AppState {
    pub(crate) fn open_reports(&mut self) {
        if self.shell().screen() != Screen::TaskList {
            return;
        }
        let now = report_now();
        self.shell_mut().open_reports(now);
        self.refresh_reports_at(now);
    }

    pub(crate) fn refresh_reports(&mut self) {
        self.refresh_reports_if_needed(report_now());
    }

    pub(crate) fn refresh_reports_if_needed(&mut self, now: DateTime<Utc>) {
        if self.shell().screen() != Screen::Reports {
            return;
        }
        let timezone = self.shell().timezone();
        let rollover = self.shell().report().is_some_and(|report| {
            report.follow_calendar
                && report.calendar_today != now.with_timezone(&timezone).date_naive()
        });
        let live = self
            .shell()
            .report()
            .and_then(|report| report.range(timezone))
            .is_some_and(|(start, end)| start <= now && now < end);
        if rollover || live {
            self.tick_reports_at(now);
        }
    }

    pub(crate) fn tick_reports_at(&mut self, now: DateTime<Utc>) {
        if self
            .shell()
            .report()
            .and_then(|report| report.last_refresh_second)
            != Some(now.timestamp())
        {
            self.refresh_reports_at(now);
        }
    }

    pub(crate) fn refresh_reports_now(&mut self) {
        if self.shell().screen() == Screen::Reports {
            self.refresh_reports_at(report_now());
        }
    }

    fn refresh_reports_at(&mut self, now: DateTime<Utc>) {
        let timezone = self.shell().timezone();
        let today = now.with_timezone(&timezone).date_naive();
        let report = self.shell_mut().report_mut().expect("report is open");
        report.follow_calendar_date(today);
        let Some(state) = self.shell().report() else {
            return;
        };
        let session_id = state.session_id;
        let period_generation = state.period_generation;
        let Some((start, end)) = state.range(timezone) else {
            let report = self.shell_mut().report_mut().expect("report is open");
            report.clear_totals();
            report.report_error = true;
            self.shell_mut()
                .error("Report dates are outside the supported range");
            return;
        };
        self.shell_mut()
            .report_mut()
            .expect("report is open")
            .last_refresh_second = Some(now.timestamp());
        self.enqueue(
            ApplicationRequest::ReportTotals { start, end, now },
            move |app, completed| {
                if app.shell().screen() != Screen::Reports {
                    return;
                }
                let current_period = app.shell().report().is_some_and(|report| {
                    report.session_id == session_id
                        && report.period_generation == period_generation
                        && report.range(app.shell().timezone()) == Some((start, end))
                });
                if !current_period {
                    app.refresh_reports_now();
                    return;
                }
                let ApplicationOutcome::ReportTotals(result) = completed.outcome else {
                    unreachable!("report request returns totals")
                };
                match result {
                    Ok(totals) => {
                        let had_error = app.shell().report().expect("report is open").report_error;
                        app.shell_mut()
                            .report_mut()
                            .expect("report is open")
                            .set_totals(totals);
                        app.sync_from_snapshot(
                            completed.snapshot.items,
                            completed.snapshot.tracking,
                            false,
                        );
                        if had_error {
                            app.shell_mut().info("Report ready");
                        }
                    }
                    Err(error) => {
                        let report = app.shell_mut().report_mut().expect("report is open");
                        report.clear_totals();
                        report.report_error = true;
                        app.shell_mut().error(application_error_text(&error));
                    }
                }
            },
        );
    }

    pub(crate) fn handle_report_command(&mut self, command: ReportCommand) {
        if self.shell().screen() != Screen::Reports {
            return;
        }
        match command {
            ReportCommand::ShowActive
            | ReportCommand::ShowAllWorklogs
            | ReportCommand::MoveUp
            | ReportCommand::MoveDown
            | ReportCommand::First
            | ReportCommand::Last
            | ReportCommand::PageUp
            | ReportCommand::PageDown
            | ReportCommand::PreviousPeriod
            | ReportCommand::NextPeriod
            | ReportCommand::Refresh
            | ReportCommand::FocusPresets
            | ReportCommand::FocusRows
            | ReportCommand::FocusTabs
            | ReportCommand::PresetPrevious
            | ReportCommand::PresetNext
            | ReportCommand::ChoosePreset => self.handle_report_navigation_command(command),
            ReportCommand::OpenHistory
            | ReportCommand::CopyName
            | ReportCommand::CopyExact
            | ReportCommand::CopyRounded
            | ReportCommand::GPrefix => self.handle_report_row_command(command),
            ReportCommand::SwitchField
            | ReportCommand::ShiftCustomDate(..)
            | ReportCommand::Insert(..)
            | ReportCommand::Backspace
            | ReportCommand::ConfirmCustom
            | ReportCommand::Cancel => self.handle_report_custom_command(command),
        }
        if command != ReportCommand::GPrefix
            && let Some(report) = self.shell_mut().report_mut()
        {
            report.g_prefix = false;
        }
    }

    fn handle_report_navigation_command(&mut self, command: ReportCommand) {
        match command {
            ReportCommand::ShowActive => self.leave_reports(TaskView::Active),
            ReportCommand::ShowAllWorklogs => self.open_all_worklogs(),
            ReportCommand::MoveUp
            | ReportCommand::MoveDown
            | ReportCommand::First
            | ReportCommand::Last
            | ReportCommand::PageUp
            | ReportCommand::PageDown => self.move_report(command),
            ReportCommand::PreviousPeriod | ReportCommand::NextPeriod => {
                self.step_report_period(command)
            }
            ReportCommand::Refresh => self.refresh_reports_now(),
            ReportCommand::FocusPresets
            | ReportCommand::FocusRows
            | ReportCommand::FocusTabs
            | ReportCommand::PresetPrevious
            | ReportCommand::PresetNext => self.navigate_report(command),
            ReportCommand::ChoosePreset => {
                let preset =
                    ReportPreset::ALL[self.shell().report().expect("report is open").preset_cursor];
                self.choose_report_preset(preset);
            }
            _ => unreachable!("only commands in this dispatch group reach this handler"),
        }
    }

    fn step_report_period(&mut self, command: ReportCommand) {
        let direction = if command == ReportCommand::PreviousPeriod {
            -1
        } else {
            1
        };
        if self
            .shell_mut()
            .report_mut()
            .expect("report is open")
            .step(direction)
        {
            self.refresh_reports_now();
        }
    }

    fn handle_report_row_command(&mut self, command: ReportCommand) {
        match command {
            ReportCommand::OpenHistory => self.open_report_history(),
            ReportCommand::CopyName | ReportCommand::CopyExact | ReportCommand::CopyRounded => {
                self.copy_report_value(command)
            }
            ReportCommand::GPrefix => {
                self.shell_mut()
                    .report_mut()
                    .expect("report is open")
                    .g_prefix = true
            }
            _ => unreachable!("only commands in this dispatch group reach this handler"),
        }
    }

    fn handle_report_custom_command(&mut self, command: ReportCommand) {
        match command {
            ReportCommand::SwitchField => {
                if let ReportMode::Custom { focus_to, .. } =
                    &mut self.shell_mut().report_mut().expect("report is open").mode
                {
                    *focus_to = !*focus_to;
                }
            }
            ReportCommand::ShiftCustomDate(shift) => {
                self.shell_mut()
                    .report_mut()
                    .expect("report is open")
                    .shift_custom_date(shift);
            }
            ReportCommand::Insert(character) => self.edit_custom(|field| {
                if field.len() < 10 {
                    field.push(character);
                }
            }),
            ReportCommand::Backspace => self.edit_custom(|field| {
                field.pop();
            }),
            ReportCommand::ConfirmCustom => self.confirm_custom_report_period(),
            ReportCommand::Cancel => {
                let report = self.shell_mut().report_mut().expect("report is open");
                if matches!(report.mode, ReportMode::Custom { .. }) {
                    report.mode = ReportMode::Normal;
                    report.focus = ReportFocus::Presets;
                } else {
                    report.focus = ReportFocus::TopTabs;
                }
            }
            _ => unreachable!("command is handled by an earlier dispatch group"),
        }
    }

    fn confirm_custom_report_period(&mut self) {
        let result = self
            .shell_mut()
            .report_mut()
            .expect("report is open")
            .apply_custom();
        match result {
            Ok(()) => self.refresh_reports_now(),
            Err(message) => {
                self.shell_mut()
                    .report_mut()
                    .expect("report is open")
                    .report_error = true;
                self.shell_mut().error(message);
            }
        }
    }

    fn leave_reports(&mut self, view: TaskView) {
        let first = self.catalog().tasks(view).first().map(|task| task.id());
        self.shell_mut().leave_reports(view, first);
        self.reload_tasks();
    }

    fn navigate_report(&mut self, command: ReportCommand) {
        let selected_preset = {
            let report = self.shell_mut().report_mut().expect("report is open");
            match command {
                ReportCommand::FocusPresets => report.focus = ReportFocus::Presets,
                ReportCommand::FocusRows => {
                    if report
                        .totals
                        .as_ref()
                        .is_some_and(|totals| !totals.rows.is_empty())
                    {
                        report.focus = ReportFocus::Rows;
                    }
                }
                ReportCommand::FocusTabs => report.focus = ReportFocus::TopTabs,
                ReportCommand::PresetPrevious => {
                    report.preset_cursor = (report.preset_cursor + ReportPreset::ALL.len() - 1)
                        % ReportPreset::ALL.len();
                }
                ReportCommand::PresetNext => {
                    report.preset_cursor = (report.preset_cursor + 1) % ReportPreset::ALL.len();
                }
                _ => unreachable!("only report navigation commands reach this handler"),
            }
            matches!(
                command,
                ReportCommand::PresetPrevious | ReportCommand::PresetNext
            )
            .then_some(ReportPreset::ALL[report.preset_cursor])
        };
        if let Some(preset) = selected_preset.filter(|preset| *preset != ReportPreset::Custom) {
            self.choose_report_preset(preset);
        }
    }

    fn choose_report_preset(&mut self, preset: ReportPreset) {
        let today = report_now()
            .with_timezone(&self.shell().timezone())
            .date_naive();
        if self
            .shell_mut()
            .report_mut()
            .expect("report is open")
            .choose(preset, today)
        {
            self.refresh_reports_now();
        }
    }

    fn edit_custom(&mut self, edit: impl FnOnce(&mut String)) {
        if let ReportMode::Custom { from, to, focus_to } =
            &mut self.shell_mut().report_mut().expect("report is open").mode
        {
            edit(if *focus_to { to } else { from });
        }
    }

    fn move_report(&mut self, command: ReportCommand) {
        let report = self.shell_mut().report_mut().expect("report is open");
        let len = report.totals.as_ref().map_or(0, |totals| totals.rows.len());
        if len == 0 {
            return;
        }
        let index = report.selected_index().unwrap_or(0);
        let target = match command {
            ReportCommand::MoveUp if index == 0 => {
                report.focus = ReportFocus::Presets;
                return;
            }
            ReportCommand::MoveUp => index - 1,
            ReportCommand::MoveDown => (index + 1).min(len - 1),
            ReportCommand::First => 0,
            ReportCommand::Last => len - 1,
            ReportCommand::PageUp => index.saturating_sub(10),
            ReportCommand::PageDown => (index + 10).min(len - 1),
            _ => return,
        };
        report.select_index(target);
    }

    fn open_report_history(&mut self) {
        let Some(task_id) = self.shell().report().and_then(|report| report.selected) else {
            return;
        };
        let report_context = self.shell().report().map(|report| {
            (
                report.session_id,
                report.period_generation,
                report.from,
                report.to,
            )
        });
        self.enqueue(
            ApplicationRequest::WorklogsForTask {
                task_id,
                after: None,
            },
            move |app, completed| {
                if app.shell().screen() != Screen::Reports
                    || app.shell().report().and_then(|report| report.selected) != Some(task_id)
                    || app.shell().report().map(|report| {
                        (
                            report.session_id,
                            report.period_generation,
                            report.from,
                            report.to,
                        )
                    }) != report_context
                {
                    return;
                }
                let ApplicationOutcome::WorklogPage(result) = completed.outcome else {
                    unreachable!("history request returns a page")
                };
                match result {
                    Ok(page) => {
                        let baseline = crate::screens::worklog_history::active_worklog_for_task(
                            &page.snapshot.active_worklog,
                            task_id,
                        );
                        app.shell_mut().open_report_history(History::new(
                            task_id,
                            page.worklogs,
                            page.next_cursor,
                            baseline,
                        ));
                        app.sync_from_snapshot(
                            completed.snapshot.items,
                            completed.snapshot.tracking,
                            false,
                        );
                        app.shell_mut().info("Task history");
                    }
                    Err(error) => app.shell_mut().error(application_error_text(&error)),
                }
            },
        );
    }

    fn copy_report_value(&mut self, command: ReportCommand) {
        let Some(row) = self
            .shell()
            .report()
            .and_then(|report| report.totals.as_ref())
            .and_then(|totals| {
                totals.rows.iter().find(|row| {
                    Some(row.task.id()) == self.shell().report().and_then(|report| report.selected)
                })
            })
        else {
            return;
        };
        let value = match command {
            ReportCommand::CopyName => row.task.name().to_string(),
            ReportCommand::CopyExact => format_exact(row.duration),
            ReportCommand::CopyRounded => format_rounded(row.duration),
            _ => return,
        };
        self.copy_text(&value);
    }

    pub(crate) fn copy_text(&mut self, value: &str) {
        match crate::support::clipboard::copy(value) {
            Ok(()) => self.shell_mut().copied_to_clipboard(),
            Err(error) => self.shell_mut().error(format!("Clipboard: {error}")),
        }
    }
}

pub(crate) fn format_exact(duration: TimeDelta) -> String {
    let seconds = duration.num_seconds().max(0);
    let hours = seconds / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let seconds = seconds % 60;
    let mut parts = Vec::new();
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if hours > 0 || minutes > 0 {
        parts.push(format!("{minutes}m"));
    }
    parts.push(format!("{seconds}s"));
    parts.join(" ")
}

pub(crate) fn format_rounded(duration: TimeDelta) -> String {
    let seconds = duration.num_seconds().max(0);
    let quarter_minutes = ((seconds + 450) / 900) * 15;
    let hours = quarter_minutes / 60;
    let minutes = quarter_minutes % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

fn report_now() -> DateTime<Utc> {
    #[cfg(debug_assertions)]
    if let Ok(value) = std::env::var("TT_TEST_NOW")
        && value.ends_with('Z')
        && let Ok(parsed) = DateTime::parse_from_rfc3339(&value)
    {
        return parsed.with_timezone(&Utc);
    }
    Utc::now()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, AppEffect, AppState, Status};
    use crate::application_request::{ApplicationSnapshot, CompletedRequest};
    use crate::command::Command;
    use crate::screens::TaskListCommand;
    use crate::test_support::{TestService, app_in_timezone, archived_task, at, task};
    use tracker_application::{ApplicationError, RepositoryError};
    use tracker_application::{ReportRow, ReportTotals, WorklogPage, WorklogPageSnapshot};
    use tracker_domain::TrackingState;

    fn finish_empty_report(state: &mut AppState, effect: AppEffect) {
        let completed = CompletedRequest {
            request: effect.request.clone(),
            outcome: ApplicationOutcome::ReportTotals(Ok(ReportTotals {
                rows: Vec::new(),
                total: TimeDelta::zero(),
            })),
            snapshot: ApplicationSnapshot {
                items: Vec::new(),
                tracking: TrackingState::Idle,
            },
        };
        state.complete_effect(effect, completed);
    }

    #[test]
    fn period_change_during_a_report_read_queues_the_new_period() {
        let mut state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
        state.open_reports();
        let first = state
            .shell()
            .report()
            .unwrap()
            .range(state.shell().timezone())
            .unwrap();
        let effect = state.take_effect().expect("first report request");
        state.handle_report_command(ReportCommand::PreviousPeriod);
        let latest = state
            .shell()
            .report()
            .unwrap()
            .range(state.shell().timezone())
            .unwrap();
        assert_ne!(first, latest);
        assert!(state.shell().report().unwrap().totals.is_none());
        finish_empty_report(&mut state, effect);
        assert!(state.shell().report().unwrap().totals.is_none());
        let replacement = state.take_effect().expect("new period request");
        assert!(
            matches!(replacement.request, ApplicationRequest::ReportTotals { start, end, .. } if (start, end) == latest)
        );
    }

    #[test]
    fn live_report_accepts_a_result_after_the_next_tick() {
        let mut state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
        state.open_reports();
        let first = state.take_effect().expect("first report request");
        let ApplicationRequest::ReportTotals { now, end, .. } = &first.request else {
            unreachable!("first request is a report")
        };
        let next_tick = if *now + TimeDelta::seconds(2) < *end {
            *now + TimeDelta::seconds(2)
        } else {
            *now - TimeDelta::seconds(2)
        };
        state.refresh_reports_at(next_tick);
        finish_empty_report(&mut state, first);
        assert!(state.shell().report().unwrap().totals.is_some());
        assert!(state.take_effect().is_some());
    }

    #[test]
    fn reopening_reports_during_a_read_ignores_the_old_result() {
        let mut state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
        state.open_reports();
        let effect = state.take_effect().expect("first report request");
        state.handle_report_command(ReportCommand::ShowActive);
        state.open_reports();
        finish_empty_report(&mut state, effect);
        assert!(state.shell().report().unwrap().totals.is_none());
        let replacement = state.take_effect().expect("reopened report request");
        finish_empty_report(&mut state, replacement);
        assert!(state.shell().report().unwrap().totals.is_some());
    }

    #[test]
    fn report_refresh_does_not_discard_pending_history() {
        let selected = task(1, "selected task");
        let mut state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
        state.open_reports();
        let first = state.take_effect().expect("report request");
        let request = first.request.clone();
        state.complete_effect(
            first,
            CompletedRequest {
                request,
                outcome: ApplicationOutcome::ReportTotals(Ok(ReportTotals {
                    rows: vec![ReportRow {
                        task: selected.clone(),
                        duration: TimeDelta::seconds(1),
                    }],
                    total: TimeDelta::seconds(1),
                })),
                snapshot: ApplicationSnapshot {
                    items: Vec::new(),
                    tracking: TrackingState::Idle,
                },
            },
        );
        state.handle_report_command(ReportCommand::OpenHistory);
        let history = state.take_effect().expect("history request");
        let (start, end) = state
            .shell()
            .report()
            .unwrap()
            .range(state.shell().timezone())
            .unwrap();
        state.refresh_reports_at(start + (end - start) / 2);
        let request = history.request.clone();
        state.complete_effect(
            history,
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
                    items: Vec::new(),
                    tracking: TrackingState::Idle,
                },
            },
        );
        assert_eq!(state.shell().screen(), Screen::WorklogHistory);
    }

    #[test]
    fn history_read_does_not_open_after_stepping_away_and_back() {
        let selected = task(1, "selected task");
        let mut state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
        state.open_reports();
        let first = state.take_effect().expect("report request");
        finish_empty_report(&mut state, first);
        state
            .shell_mut()
            .report_mut()
            .unwrap()
            .set_totals(ReportTotals {
                rows: vec![ReportRow {
                    task: selected,
                    duration: TimeDelta::seconds(1),
                }],
                total: TimeDelta::seconds(1),
            });
        state.handle_report_command(ReportCommand::OpenHistory);
        let history = state.take_effect().expect("history request");
        state.handle_report_command(ReportCommand::PreviousPeriod);
        state.handle_report_command(ReportCommand::NextPeriod);
        let request = history.request.clone();
        state.complete_effect(
            history,
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
                    items: Vec::new(),
                    tracking: TrackingState::Idle,
                },
            },
        );
        assert_eq!(state.shell().screen(), Screen::Reports);
    }

    #[test]
    fn history_read_does_not_open_after_selecting_another_report_row() {
        let first_task = task(1, "first task");
        let second_task = task(2, "second task");
        let mut state = AppState::load_from_snapshot(Vec::new(), TrackingState::Idle);
        state.open_reports();
        let first = state.take_effect().expect("report request");
        finish_empty_report(&mut state, first);
        state
            .shell_mut()
            .report_mut()
            .unwrap()
            .set_totals(ReportTotals {
                rows: vec![
                    ReportRow {
                        task: first_task,
                        duration: TimeDelta::seconds(1),
                    },
                    ReportRow {
                        task: second_task.clone(),
                        duration: TimeDelta::seconds(1),
                    },
                ],
                total: TimeDelta::seconds(2),
            });
        state.handle_report_command(ReportCommand::OpenHistory);
        let history = state.take_effect().expect("history request");
        state.shell_mut().report_mut().unwrap().select_index(1);
        assert_eq!(
            state.shell().report().unwrap().selected,
            Some(second_task.id())
        );
        let request = history.request.clone();
        state.complete_effect(
            history,
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
                    items: Vec::new(),
                    tracking: TrackingState::Idle,
                },
            },
        );
        assert_eq!(state.shell().screen(), Screen::Reports);
    }
    #[test]
    fn copied_durations_follow_exact_and_quarter_hour_formats() {
        assert_eq!(format_exact(TimeDelta::seconds(3_661)), "1h 1m 1s");
        assert_eq!(format_exact(TimeDelta::zero()), "0s");
        assert_eq!(format_rounded(TimeDelta::seconds(3_600)), "1h 0m");
        assert_eq!(format_rounded(TimeDelta::seconds(449)), "0m");
        assert_eq!(format_rounded(TimeDelta::seconds(450)), "15m");
    }

    #[test]
    fn entering_reports_reads_totals_and_returning_selects_a_task() {
        let selected = task(1, "selected task");
        let service = TestService::with_tasks(vec![selected.clone()]);
        let spy = service.spy();
        spy.set_report_result(Ok(ReportTotals {
            rows: vec![ReportRow {
                task: selected.clone(),
                duration: TimeDelta::minutes(15),
            }],
            total: TimeDelta::minutes(15),
        }));
        let mut app = app_in_timezone(service, chrono_tz::Europe::Berlin);
        app.handle(Command::TaskList(TaskListCommand::ShowReports));
        let report = app.shell().report().unwrap();
        assert_eq!(report.selected, Some(selected.id()));
        let (start, end) = report.range(chrono_tz::Europe::Berlin).unwrap();
        assert_eq!(spy.report_reads()[0].0, start);
        assert_eq!(spy.report_reads()[0].1, end);
        let same_second = DateTime::from_timestamp(report.last_refresh_second.unwrap(), 0).unwrap();
        app.tick_reports_at(same_second);
        assert_eq!(spy.report_reads().len(), 1);
        let original_day = app.shell().report().unwrap().from;
        app.handle(Command::Reports(ReportCommand::PreviousPeriod));
        assert_eq!(
            app.shell().report().unwrap().from,
            original_day.pred_opt().unwrap()
        );
        assert_eq!(spy.report_reads().len(), 2);
        app.handle(Command::Reports(ReportCommand::NextPeriod));
        assert_eq!(app.shell().report().unwrap().from, original_day);
        app.handle(Command::Reports(ReportCommand::ShowActive));
        assert_eq!(app.shell().task_list().view(), TaskView::Active);
        assert_eq!(app.shell().task_list().selection(), Some(selected.id()));
    }

    #[test]
    fn moving_preset_focus_applies_fixed_ranges_without_opening_custom() {
        let selected = task(1, "period task");
        let service = TestService::with_tasks(vec![selected.clone()]);
        let spy = service.spy();
        spy.set_report_result(Ok(ReportTotals {
            rows: vec![ReportRow {
                task: selected,
                duration: TimeDelta::minutes(15),
            }],
            total: TimeDelta::minutes(15),
        }));
        let mut app = app_in_timezone(service, chrono_tz::UTC);
        app.handle(Command::TaskList(TaskListCommand::ShowReports));
        let today = app.shell().report().unwrap().from;
        let initial_reads = spy.report_reads().len();
        app.handle(Command::Reports(ReportCommand::FocusPresets));
        assert_eq!(app.shell().report().unwrap().focus, ReportFocus::Presets);
        app.handle(Command::Reports(ReportCommand::PresetPrevious));
        assert_eq!(app.shell().report().unwrap().preset_cursor, 5);
        assert_eq!(app.shell().report().unwrap().preset, ReportPreset::Today);
        assert!(matches!(
            app.shell().report().unwrap().mode,
            ReportMode::Normal
        ));
        assert_eq!(spy.report_reads().len(), initial_reads);
        app.handle(Command::Reports(ReportCommand::PresetNext));
        assert_eq!(app.shell().report().unwrap().preset_cursor, 0);
        assert_eq!(spy.report_reads().len(), initial_reads + 1);
        app.handle(Command::Reports(ReportCommand::PresetNext));
        let report = app.shell().report().unwrap();
        assert_eq!(report.preset, ReportPreset::Yesterday);
        assert_eq!(report.from, today.pred_opt().unwrap());
        assert_eq!(report.focus, ReportFocus::Presets);
        assert_eq!(spy.report_reads().len(), initial_reads + 2);
        app.handle(Command::Reports(ReportCommand::PresetPrevious));
        assert_eq!(app.shell().report().unwrap().preset, ReportPreset::Today);
        assert_eq!(app.shell().report().unwrap().from, today);
        app.handle(Command::Reports(ReportCommand::PresetPrevious));
        assert_eq!(app.shell().report().unwrap().preset_cursor, 5);
        assert_eq!(app.shell().report().unwrap().preset, ReportPreset::Today);
        app.handle(Command::Reports(ReportCommand::ChoosePreset));
        assert!(matches!(
            app.shell().report().unwrap().mode,
            ReportMode::Custom { .. }
        ));
        app.handle(Command::Reports(ReportCommand::Cancel));
        app.handle(Command::Reports(ReportCommand::FocusRows));
        assert_eq!(app.shell().report().unwrap().focus, ReportFocus::Rows);
        app.handle(Command::Reports(ReportCommand::MoveUp));
        assert_eq!(app.shell().report().unwrap().focus, ReportFocus::Presets);
        app.handle(Command::Reports(ReportCommand::FocusRows));
        app.handle(Command::Reports(ReportCommand::FocusTabs));
        assert_eq!(app.shell().report().unwrap().focus, ReportFocus::TopTabs);
    }

    #[test]
    fn automatic_refresh_uses_live_half_open_range_and_calendar_following() {
        let service = TestService::with_tasks(vec![task(1, "daily task")]);
        let spy = service.spy();
        let mut app = app_in_timezone(service, chrono_tz::UTC);
        app.handle(Command::TaskList(TaskListCommand::ShowReports));
        let initial_reads = spy.report_reads().len();
        let day = chrono::NaiveDate::from_ymd_opt(2024, 5, 2).unwrap();
        let midnight = day.and_hms_opt(0, 0, 0).unwrap().and_utc();
        let report = app.shell_mut().report_mut().unwrap();
        report.from = day;
        report.to = day;
        report.calendar_today = day;
        report.follow_calendar = false;
        report.last_refresh_second = None;

        app.refresh_reports_if_needed(midnight - TimeDelta::seconds(1));
        assert_eq!(spy.report_reads().len(), initial_reads);
        app.refresh_reports_if_needed(midnight);
        assert_eq!(spy.report_reads().len(), initial_reads + 1);
        app.refresh_reports_if_needed(midnight + TimeDelta::days(1));
        assert_eq!(spy.report_reads().len(), initial_reads + 1);
        app.refresh_reports_if_needed(midnight + TimeDelta::days(2));
        assert_eq!(spy.report_reads().len(), initial_reads + 1);

        app.shell_mut().report_mut().unwrap().follow_calendar = true;
        app.refresh_reports_if_needed(midnight + TimeDelta::days(2));
        assert_eq!(spy.report_reads().len(), initial_reads + 2);
        assert_eq!(app.shell().report().unwrap().from, day + TimeDelta::days(2));
    }

    #[test]
    fn failed_report_read_clears_rows_and_a_later_success_clears_the_error() {
        let selected = task(1, "selected task");
        let service = TestService::with_tasks(vec![selected.clone()]);
        let spy = service.spy();
        let totals = ReportTotals {
            rows: vec![ReportRow {
                task: selected.clone(),
                duration: TimeDelta::minutes(15),
            }],
            total: TimeDelta::minutes(15),
        };
        spy.set_report_result(Ok(totals.clone()));
        let mut app = app_in_timezone(service, chrono_tz::UTC);
        app.handle(Command::TaskList(TaskListCommand::ShowReports));
        assert_eq!(app.shell().report().unwrap().selected, Some(selected.id()));
        spy.set_report_result(Err(ApplicationError::Repository(
            RepositoryError::TaskNotFound { id: selected.id() },
        )));
        app.refresh_reports_now();
        let report = app.shell().report().unwrap();
        assert!(report.totals.is_none());
        assert!(report.selected.is_none());
        assert!(matches!(app.shell().status(), Status::Error(_)));
        app.handle(Command::Reports(ReportCommand::CopyName));
        app.handle(Command::Reports(ReportCommand::OpenHistory));
        assert_eq!(app.shell().screen(), Screen::Reports);
        spy.set_report_result(Ok(totals));
        app.refresh_reports_now();
        assert_eq!(app.shell().report().unwrap().selected, Some(selected.id()));
        assert_eq!(
            app.shell().status(),
            &Status::Info("Report ready".to_owned())
        );
    }

    #[test]
    fn an_unrepresentable_report_range_clears_previous_rows() {
        let selected = task(1, "selected task");
        let service = TestService::with_tasks(vec![selected.clone()]);
        let spy = service.spy();
        spy.set_report_result(Ok(ReportTotals {
            rows: vec![ReportRow {
                task: selected.clone(),
                duration: TimeDelta::seconds(1),
            }],
            total: TimeDelta::seconds(1),
        }));
        let mut app = app_in_timezone(service, chrono_tz::UTC);
        app.handle(Command::TaskList(TaskListCommand::ShowReports));
        let report = app.shell_mut().report_mut().unwrap();
        report.to = chrono::NaiveDate::MAX;
        report.follow_calendar = false;
        app.refresh_reports_now();
        assert!(app.shell().report().unwrap().totals.is_none());
        assert!(app.shell().report().unwrap().selected.is_none());
        assert!(matches!(app.shell().status(), Status::Error(_)));
    }

    #[test]
    fn report_snapshot_reconciles_both_saved_task_tabs() {
        let active = task(1, "active task");
        let archived = archived_task(2, "archived task");
        let service = TestService::with_tasks(vec![active.clone(), archived.clone()]);
        let spy = service.spy();
        let mut app = app_in_timezone(service, chrono_tz::UTC);
        app.handle(Command::TaskList(TaskListCommand::ShowArchivedTasks));
        assert_eq!(app.shell().task_list().selection(), Some(archived.id()));
        app.handle(Command::TaskList(TaskListCommand::ShowReports));
        let mut newly_archived = active.clone();
        newly_archived.archive(at(200));
        let mut newly_active = archived.clone();
        newly_active.restore(at(200));
        spy.set_report_tasks(vec![newly_archived, newly_active]);
        app.refresh_reports_now();
        app.handle(Command::Reports(ReportCommand::ShowAllWorklogs));
        app.handle(Command::AllWorklogs(
            crate::screens::AllWorklogsCommand::ShowArchived,
        ));
        assert_eq!(app.shell().task_list().selection(), Some(active.id()));
        app.handle(Command::TaskList(TaskListCommand::ShowReports));
        app.handle(Command::Reports(ReportCommand::ShowActive));
        assert_eq!(app.shell().task_list().selection(), Some(archived.id()));
    }

    #[test]
    fn report_navigation_clamps_and_selects_first_last_and_pages() {
        let tasks: Vec<_> = (1..=25)
            .map(|tag| task(tag, &format!("task {tag}")))
            .collect();
        let service = TestService::with_tasks(tasks.clone());
        let spy = service.spy();
        spy.set_report_result(Ok(ReportTotals {
            rows: tasks
                .iter()
                .cloned()
                .map(|task| ReportRow {
                    task,
                    duration: TimeDelta::seconds(1),
                })
                .collect(),
            total: TimeDelta::seconds(25),
        }));
        let mut app = app_in_timezone(service, chrono_tz::UTC);
        app.handle(Command::TaskList(TaskListCommand::ShowReports));
        let selected = |app: &App<TestService>| app.shell().report().unwrap().selected_index();
        assert_eq!(selected(&app), Some(0));
        app.handle(Command::Reports(ReportCommand::MoveUp));
        assert_eq!(selected(&app), Some(0));
        app.handle(Command::Reports(ReportCommand::PageDown));
        assert_eq!(selected(&app), Some(10));
        app.handle(Command::Reports(ReportCommand::MoveDown));
        assert_eq!(selected(&app), Some(11));
        app.handle(Command::Reports(ReportCommand::Last));
        assert_eq!(selected(&app), Some(24));
        app.handle(Command::Reports(ReportCommand::MoveDown));
        assert_eq!(selected(&app), Some(24));
        app.handle(Command::Reports(ReportCommand::PageDown));
        assert_eq!(selected(&app), Some(24));
        app.handle(Command::Reports(ReportCommand::PageUp));
        assert_eq!(selected(&app), Some(14));
        app.handle(Command::Reports(ReportCommand::First));
        assert_eq!(selected(&app), Some(0));
        app.handle(Command::Reports(ReportCommand::PageUp));
        assert_eq!(selected(&app), Some(0));
        app.shell_mut().report_mut().unwrap().clear_totals();
        app.handle(Command::Reports(ReportCommand::MoveDown));
        assert_eq!(selected(&app), None);
    }

    #[test]
    fn custom_date_input_stops_at_ten_characters() {
        let service = TestService::with_tasks(vec![task(1, "date task")]);
        let mut app = app_in_timezone(service, chrono_tz::UTC);
        app.handle(Command::TaskList(TaskListCommand::ShowReports));
        app.shell_mut().report_mut().unwrap().mode = ReportMode::Custom {
            from: String::new(),
            to: String::new(),
            focus_to: false,
        };
        for character in "2024-05-021".chars() {
            app.handle(Command::Reports(ReportCommand::Insert(character)));
        }
        assert_eq!(
            app.shell().report().unwrap().mode,
            ReportMode::Custom {
                from: "2024-05-02".to_owned(),
                to: String::new(),
                focus_to: false,
            }
        );
    }

    #[test]
    fn historical_reports_wait_for_manual_refresh() {
        let service = TestService::with_tasks(vec![task(1, "old task")]);
        let spy = service.spy();
        let mut app = app_in_timezone(service, chrono_tz::UTC);
        app.handle(Command::TaskList(TaskListCommand::ShowReports));
        let initial_reads = spy.report_reads().len();
        let report = app.shell_mut().report_mut().unwrap();
        report.from = chrono::NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
        report.to = report.from;
        report.follow_calendar = false;
        app.refresh_reports_now();
        assert_eq!(spy.report_reads().len(), initial_reads + 1);
        app.refresh_reports();
        assert_eq!(spy.report_reads().len(), initial_reads + 1);
        app.handle(Command::Reports(ReportCommand::Refresh));
        assert_eq!(spy.report_reads().len(), initial_reads + 2);
    }
}

use chrono::{DateTime, TimeDelta, Utc};
use tracker_application::TrackerApplicationService;

use crate::app::App;
use crate::screens::reports::{ReportMode, ReportPreset};
use crate::screens::{History, ReportCommand, Screen, TaskView};
use crate::support::errors::application_error_text;

impl<S: TrackerApplicationService> App<S> {
    pub(crate) fn open_reports(&mut self) {
        if self.shell().screen() != Screen::TaskList {
            return;
        }
        let now = report_now();
        self.shell_mut().open_reports(now);
        self.refresh_reports_at(now);
    }

    pub(crate) fn refresh_reports(&mut self) {
        if self.shell().screen() == Screen::Reports {
            let now = report_now();
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
    }

    fn tick_reports_at(&mut self, now: DateTime<Utc>) {
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
        self.shell_mut()
            .report_mut()
            .expect("report is open")
            .follow_calendar_date(today);
        if self
            .shell()
            .report()
            .is_some_and(|report| report.loaded_dates != Some((report.from, report.to)))
        {
            self.shell_mut()
                .report_mut()
                .expect("report is open")
                .clear_totals();
        }
        let Some(state) = self.shell().report() else {
            return;
        };
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
        match self.application_mut().report_totals(start, end, now) {
            Ok(totals) => {
                let had_error = self.shell().report().expect("report is open").report_error;
                let report = self.shell_mut().report_mut().expect("report is open");
                report.set_totals(totals);
                self.sync_from_application(false);
                if had_error {
                    self.shell_mut().info("Report ready");
                }
            }
            Err(error) => {
                let report = self.shell_mut().report_mut().expect("report is open");
                report.clear_totals();
                report.report_error = true;
                self.shell_mut().error(application_error_text(&error));
            }
        }
    }

    pub(crate) fn handle_report_command(&mut self, command: ReportCommand) {
        if self.shell().screen() != Screen::Reports {
            return;
        }
        match command {
            ReportCommand::ShowActive => self.leave_reports(TaskView::Active),
            ReportCommand::ShowArchived => self.leave_reports(TaskView::Archived),
            ReportCommand::MoveUp
            | ReportCommand::MoveDown
            | ReportCommand::First
            | ReportCommand::Last
            | ReportCommand::PageUp
            | ReportCommand::PageDown => self.move_report(command),
            ReportCommand::PreviousPeriod | ReportCommand::NextPeriod => {
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
            ReportCommand::Refresh => self.refresh_reports_now(),
            ReportCommand::OpenPresets => {
                let report = self.shell_mut().report_mut().expect("report is open");
                let highlighted = report.highlighted_preset().unwrap_or(report.preset);
                let selected = ReportPreset::ALL
                    .iter()
                    .position(|preset| *preset == highlighted)
                    .unwrap_or(0);
                report.mode = ReportMode::Presets { selected };
            }
            ReportCommand::PresetUp | ReportCommand::PresetDown => {
                let ReportMode::Presets { selected } =
                    &mut self.shell_mut().report_mut().expect("report is open").mode
                else {
                    return;
                };
                *selected = if command == ReportCommand::PresetUp {
                    selected.saturating_sub(1)
                } else {
                    (*selected + 1).min(ReportPreset::ALL.len() - 1)
                };
            }
            ReportCommand::ChoosePreset => {
                let index = match self.shell().report().expect("report is open").mode {
                    ReportMode::Presets { selected } => selected,
                    _ => return,
                };
                let today = report_now()
                    .with_timezone(&self.shell().timezone())
                    .date_naive();
                if self
                    .shell_mut()
                    .report_mut()
                    .expect("report is open")
                    .choose(ReportPreset::ALL[index], today)
                {
                    self.refresh_reports_now();
                }
            }
            ReportCommand::SwitchField => {
                if let ReportMode::Custom { focus_to, .. } =
                    &mut self.shell_mut().report_mut().expect("report is open").mode
                {
                    *focus_to = !*focus_to;
                }
            }
            ReportCommand::Insert(character) => self.edit_custom(|field| {
                if field.len() < 10 {
                    field.push(character);
                }
            }),
            ReportCommand::Backspace => self.edit_custom(|field| {
                field.pop();
            }),
            ReportCommand::ConfirmCustom => {
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
            ReportCommand::Cancel => {
                self.shell_mut().report_mut().expect("report is open").mode = ReportMode::Normal
            }
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
        }
        if command != ReportCommand::GPrefix
            && let Some(report) = self.shell_mut().report_mut()
        {
            report.g_prefix = false;
        }
    }

    fn leave_reports(&mut self, view: TaskView) {
        let first = self.catalog().tasks(view).first().map(|task| task.id());
        self.shell_mut().leave_reports(view, first);
        self.reload_tasks();
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
            ReportCommand::MoveUp => index.saturating_sub(1),
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
        let result = self.application_mut().worklogs_for_task(task_id, None);
        match result {
            Ok(page) => {
                let baseline = crate::screens::worklog_history::active_worklog_for_task(
                    &page.snapshot.active_worklog,
                    task_id,
                );
                self.shell_mut().open_report_history(History::new(
                    task_id,
                    page.worklogs,
                    page.next_cursor,
                    baseline,
                ));
                self.sync_from_application(false);
                self.shell_mut().info("Task history");
            }
            Err(error) => self.shell_mut().error(application_error_text(&error)),
        }
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
            Ok(()) => self.shell_mut().info("Copied to clipboard"),
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
    if let Ok(value) = std::env::var("TT_TEST_NOW") {
        if value.ends_with('Z')
            && let Ok(parsed) = DateTime::parse_from_rfc3339(&value)
        {
            return parsed.with_timezone(&Utc);
        }
    }
    Utc::now()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Status;
    use crate::command::Command;
    use crate::screens::TaskListCommand;
    use crate::test_support::{TestService, app_in_timezone, archived_task, at, task};
    use tracker_application::{ApplicationError, RepositoryError};
    use tracker_application::{ReportRow, ReportTotals};
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
        app.handle(Command::Reports(ReportCommand::PreviousPeriod));
        assert_eq!(spy.report_reads().len(), 2);
        app.handle(Command::Reports(ReportCommand::ShowArchived));
        assert_eq!(app.shell().task_list().view(), TaskView::Archived);
        app.handle(Command::TaskList(TaskListCommand::ShowActiveTasks));
        assert_eq!(app.shell().task_list().selection(), Some(selected.id()));
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
        app.handle(Command::Reports(ReportCommand::ShowActive));
        assert_eq!(app.shell().task_list().selection(), Some(archived.id()));
        app.handle(Command::TaskList(TaskListCommand::ShowArchivedTasks));
        assert_eq!(app.shell().task_list().selection(), Some(active.id()));
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

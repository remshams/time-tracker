//! TUI presentation state and semantic command handling.

use std::time::{Duration, Instant};

use chrono::{DateTime, FixedOffset, NaiveDateTime, Offset, TimeDelta, TimeZone, Utc};
use chrono_tz::Tz;
use tracker_application::{
    ApplicationError, ClearActiveTaskOutcome, CorrectWorklogOutcome, DeleteCompletedWorklogOutcome,
    RepositoryError, SetActiveTaskOutcome, TaskOrdering, TaskOutcome, TrackerApplicationService,
    WorklogCursor,
};
use tracker_domain::{
    Task, TaskId, TaskName, TaskNameError, TrackingError, TrackingState, Worklog, WorklogId,
    WorklogTimes,
};

use crate::command::Command;

const ACTIVE_WORKLOG_DELETE_MESSAGE: &str = "Running worklogs cannot be deleted";

/// What the TUI is currently asking of the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Input {
        purpose: InputPurpose,
        buffer: String,
    },
    ConfirmArchive {
        task_id: TaskId,
        name: String,
    },
    ConfirmDeletion {
        worklog: Worklog,
    },
    Correction(CorrectionDraft),
}

/// The correction field that receives input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrectionField {
    Start,
    End,
}

/// One bounded timestamp input with a character-indexed cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimestampInput {
    text: String,
    initial_text: String,
    cursor: usize,
    adjusted_instant: Option<DateTime<Utc>>,
}

impl TimestampInput {
    const MAX_LEN: usize = 19;

    fn new(text: String) -> Self {
        let cursor = text.chars().count();
        Self {
            initial_text: text.clone(),
            text,
            cursor,
            adjusted_instant: None,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    fn insert(&mut self, character: char) {
        if self.text.chars().count() >= Self::MAX_LEN || !is_timestamp_character(character) {
            return;
        }
        let byte = byte_index(&self.text, self.cursor);
        self.text.insert(byte, character);
        self.cursor += 1;
        self.adjusted_instant = None;
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let end = byte_index(&self.text, self.cursor);
        let start = byte_index(&self.text, self.cursor - 1);
        self.text.replace_range(start..end, "");
        self.cursor -= 1;
        self.adjusted_instant = None;
    }

    fn delete(&mut self) {
        if self.cursor == self.text.chars().count() {
            return;
        }
        let start = byte_index(&self.text, self.cursor);
        let end = byte_index(&self.text, self.cursor + 1);
        self.text.replace_range(start..end, "");
        self.adjusted_instant = None;
    }

    fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn move_right(&mut self) {
        self.cursor = self.cursor.saturating_add(1).min(self.text.chars().count());
    }

    #[cfg(test)]
    fn replace(&mut self, text: String) {
        self.cursor = text.chars().count();
        self.text = text;
        self.adjusted_instant = None;
    }

    fn replace_with_adjustment(&mut self, text: String, instant: DateTime<Utc>) {
        self.cursor = text.chars().count();
        self.text = text;
        self.adjusted_instant = Some(instant);
    }

    fn is_unchanged(&self) -> bool {
        self.adjusted_instant.is_none() && self.text == self.initial_text
    }
}

fn byte_index(text: &str, character_index: usize) -> usize {
    text.char_indices()
        .nth(character_index)
        .map_or(text.len(), |(index, _)| index)
}

pub(crate) fn is_timestamp_character(character: char) -> bool {
    character.is_ascii_digit() || matches!(character, '+' | '-' | ':' | ' ')
}

/// Editable timestamps and the immutable snapshot used for stale detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorrectionDraft {
    id: WorklogId,
    expected: WorklogTimes,
    start: TimestampInput,
    end: Option<TimestampInput>,
    focused: CorrectionField,
    original_start: DateTime<Utc>,
    original_end: Option<DateTime<Utc>>,
}

impl CorrectionDraft {
    pub(crate) fn new(
        id: WorklogId,
        expected: WorklogTimes,
        start: String,
        end: Option<String>,
    ) -> Self {
        Self {
            id,
            original_start: expected.start(),
            original_end: expected.end(),
            expected,
            start: TimestampInput::new(start),
            end: end.map(TimestampInput::new),
            focused: CorrectionField::Start,
        }
    }

    pub fn start(&self) -> &TimestampInput {
        &self.start
    }

    pub fn end(&self) -> Option<&TimestampInput> {
        self.end.as_ref()
    }

    pub fn focused(&self) -> CorrectionField {
        self.focused
    }

    fn original(&self, field: CorrectionField) -> DateTime<Utc> {
        match field {
            CorrectionField::Start => self.original_start,
            CorrectionField::End => self
                .original_end
                .expect("completed corrections have an end"),
        }
    }

    fn focused_input(&self) -> &TimestampInput {
        match self.focused {
            CorrectionField::Start => &self.start,
            CorrectionField::End => self
                .end
                .as_ref()
                .expect("only completed corrections can focus the end"),
        }
    }

    fn focused_mut(&mut self) -> &mut TimestampInput {
        match self.focused {
            CorrectionField::Start => &mut self.start,
            CorrectionField::End => self
                .end
                .as_mut()
                .expect("only completed corrections can focus the end"),
        }
    }

    fn switch_field(&mut self) {
        if self.end.is_some() {
            self.focused = match self.focused {
                CorrectionField::Start => CorrectionField::End,
                CorrectionField::End => CorrectionField::Start,
            };
        }
    }
}

/// What a confirmed text input does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputPurpose {
    Add,
    Rename { task_id: TaskId },
}

/// The most recent message shown in the status line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Info(String),
    Error(String),
}

/// Which task list the interface currently shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskView {
    Active,
    Archived,
}

/// Which screen the interface currently shows.
///
/// The task list carries the task modes and views. Worklog history has its
/// own navigation and timestamp-correction mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// The active or archived task list.
    TaskList,
    /// The worklog history of one task.
    WorklogHistory,
}

/// Whether the open history holds a valid page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryAvailability {
    Available,
    Unavailable,
}

/// Worklog-history presentation state for one task.
///
/// The worklogs are the pages loaded so far, in history order, newest
/// first. `next_cursor` marks the end of the loaded range; `None` means
/// the whole history is on screen. When a saved correction cannot reload
/// its newest page, the history becomes unavailable instead of pretending
/// that an empty history is valid. The selection remembers a worklog id,
/// so it survives appends and refreshes the way the task selection
/// survives reordering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct History {
    /// The task whose history is open.
    pub task_id: TaskId,
    /// Whether `worklogs` and `next_cursor` describe a valid history page.
    pub availability: HistoryAvailability,
    /// Every worklog loaded so far, newest first.
    pub worklogs: Vec<Worklog>,
    /// The cursor the next older page continues after, or `None` at the
    /// end of the history.
    pub next_cursor: Option<WorklogCursor>,
    /// The active worklog for this task when the newest page was read.
    active_worklog_baseline: Option<(WorklogId, DateTime<Utc>)>,
    /// The selected worklog, by id.
    selected: Option<WorklogId>,
}

impl History {
    pub(crate) fn is_available(&self) -> bool {
        self.availability == HistoryAvailability::Available
    }

    /// The row of the selected worklog.
    ///
    /// The selection is remembered by id, so it follows a worklog across
    /// appends and refreshes; a worklog that is no longer loaded selects
    /// nothing.
    fn selected_index(&self) -> Option<usize> {
        let id = self.selected?;
        self.is_available()
            .then(|| self.worklogs.iter().position(|worklog| worklog.id() == id))
            .flatten()
    }
}

fn active_worklog_for_task(
    active_worklog: &Option<Worklog>,
    task_id: TaskId,
) -> Option<(WorklogId, DateTime<Utc>)> {
    active_worklog
        .as_ref()
        .filter(|worklog| worklog.task_id() == task_id)
        .map(|worklog| (worklog.id(), worklog.start()))
}

/// The event loop's two lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    Running,
    Quitting,
}

/// Derives displayed elapsed time from a monotonic clock.
#[derive(Debug, Clone)]
pub(crate) struct ElapsedClock {
    anchor: Instant,
    base: Duration,
}

impl ElapsedClock {
    pub(crate) fn since(start: DateTime<Utc>) -> Self {
        Self::anchored(Self::base_since(start, Utc::now()))
    }

    pub(crate) fn anchored(base: Duration) -> Self {
        Self::at_anchor(base, Instant::now())
    }

    fn at_anchor(base: Duration, anchor: Instant) -> Self {
        Self { anchor, base }
    }

    fn base_since(start: DateTime<Utc>, now: DateTime<Utc>) -> Duration {
        (now - start).to_std().unwrap_or(Duration::ZERO)
    }

    pub(crate) fn at(&self, since_anchor: Duration) -> Duration {
        self.base + since_anchor
    }

    pub(crate) fn elapsed(&self) -> Duration {
        self.at(self.anchor.elapsed())
    }
}

/// Converts displayed monotonic elapsed time to a client-created UTC instant.
fn tracking_timestamp(start: DateTime<Utc>, elapsed: Duration) -> DateTime<Utc> {
    let delta = TimeDelta::from_std(elapsed).unwrap_or(TimeDelta::MAX);
    start
        .checked_add_signed(delta)
        .unwrap_or(DateTime::<Utc>::MAX_UTC)
}

fn task_name_error_text(error: TaskNameError) -> String {
    match error {
        TaskNameError::Empty => "The task name must not be empty".to_owned(),
        TaskNameError::Control => "The task name must not contain control characters".to_owned(),
        TaskNameError::TooLong => format!(
            "The task name must be at most {} characters",
            TaskName::MAX_LEN
        ),
    }
}

fn next_ordering(ordering: TaskOrdering) -> TaskOrdering {
    match ordering {
        TaskOrdering::RecentlyWorked => TaskOrdering::RecentlyUpdated,
        TaskOrdering::RecentlyUpdated => TaskOrdering::RecentlyCreated,
        TaskOrdering::RecentlyCreated => TaskOrdering::RecentlyWorked,
    }
}

fn ordering_label(ordering: TaskOrdering) -> &'static str {
    match ordering {
        TaskOrdering::RecentlyWorked => "recently worked",
        TaskOrdering::RecentlyUpdated => "recently updated",
        TaskOrdering::RecentlyCreated => "recently created",
    }
}

fn repository_error_text(error: &RepositoryError) -> &'static str {
    match error {
        RepositoryError::TaskNotFound { .. } => "Task not found",
        RepositoryError::WorklogNotFound { .. } => "Worklog not found",
        RepositoryError::WorklogAlreadyStopped { .. } => "Worklog is already stopped",
        RepositoryError::WorklogChanged { .. } => "Worklog changed in another client",
        RepositoryError::WorklogIsActive { .. } => ACTIVE_WORKLOG_DELETE_MESSAGE,
        RepositoryError::WorklogHistoryChanged { .. } => {
            "Worklog history changed. Press r to refresh"
        }
        RepositoryError::SameTaskWorklogOverlap { .. } => "The worklog overlaps another worklog",
        RepositoryError::WorklogAlreadyExists { .. } => "Worklog already exists",
        RepositoryError::TaskAlreadyExists { .. } => "Task already exists",
        RepositoryError::ActiveWorklogExists => "Another worklog is active",
        RepositoryError::TaskArchived { .. } => "Task is archived",
        RepositoryError::TaskIsActive { .. } => "Task has active work",
        RepositoryError::Constraint { .. } => "Storage rejected the change",
        RepositoryError::CorruptData { .. } => "Stored data is invalid",
        RepositoryError::Backend { .. } => "Storage error",
    }
}

fn application_error_text(error: &ApplicationError) -> String {
    match error {
        ApplicationError::Domain(error) => error.to_string(),
        ApplicationError::InvalidWorklogCorrection(error) => error.to_string(),
        ApplicationError::Repository(error)
        | ApplicationError::TrackingWrite(error)
        | ApplicationError::TrackingRecovery(error)
        | ApplicationError::TaskRecovery(error) => repository_error_text(error).to_owned(),
        ApplicationError::WorklogCorrectionWrite { write } => {
            repository_error_text(write).to_owned()
        }
        ApplicationError::WorklogCorrectionRecovery { write, recovery } => format!(
            "Correction failed: {}. State recovery failed: {}.",
            repository_error_text(write),
            repository_error_text(recovery)
        ),
        ApplicationError::WorklogDeletionWrite { write } => repository_error_text(write).to_owned(),
        ApplicationError::WorklogDeletionRecovery { write, recovery } => format!(
            "Deletion failed: {}. State recovery failed: {}.",
            repository_error_text(write),
            repository_error_text(recovery)
        ),
        ApplicationError::TrackingStateChanged => {
            "Tracking state changed in another client. Refreshed state.".to_owned()
        }
    }
}

fn deletion_repository_error(error: &ApplicationError) -> Option<&RepositoryError> {
    match error {
        ApplicationError::WorklogDeletionWrite { write } => Some(write),
        _ => None,
    }
}

fn deletion_conflict(error: &ApplicationError) -> bool {
    matches!(
        deletion_repository_error(error),
        Some(
            RepositoryError::WorklogChanged { .. }
                | RepositoryError::WorklogNotFound { .. }
                | RepositoryError::WorklogIsActive { .. }
        )
    )
}

fn matches_worklog_active(error: &ApplicationError) -> bool {
    matches!(
        deletion_repository_error(error),
        Some(RepositoryError::WorklogIsActive { .. })
    )
}

fn matches_worklog_not_found(error: &ApplicationError) -> bool {
    matches!(
        deletion_repository_error(error),
        Some(RepositoryError::WorklogNotFound { .. })
    )
}

fn correction_error_text(error: &ApplicationError) -> String {
    match error {
        ApplicationError::WorklogCorrectionWrite {
            write: RepositoryError::WorklogChanged { .. } | RepositoryError::WorklogNotFound { .. },
        } => "Worklog changed. Cancel and press r to refresh.".to_owned(),
        ApplicationError::WorklogCorrectionWrite {
            write: RepositoryError::SameTaskWorklogOverlap { .. },
        } => "The corrected time overlaps another worklog".to_owned(),
        ApplicationError::WorklogCorrectionRecovery {
            write: RepositoryError::WorklogChanged { .. } | RepositoryError::WorklogNotFound { .. },
            recovery,
        } => format!(
            "Worklog changed. State recovery also failed: {}. Cancel and press r to refresh.",
            repository_error_text(recovery)
        ),
        ApplicationError::WorklogCorrectionRecovery {
            write: RepositoryError::SameTaskWorklogOverlap { .. },
            recovery,
        } => format!(
            "The corrected time overlaps another worklog. State recovery also failed: {}.",
            repository_error_text(recovery)
        ),
        _ => application_error_text(error),
    }
}

const CORRECTION_FORMAT: &str = "%Y-%m-%d %H:%M";
const OUTSIDE_EDITABLE_RANGE: &str = "Timestamp is outside editable range";

/// Resolves an IANA timezone once for the process. A valid `TZ` value wins,
/// including the zoneinfo path form used by some shells and test runners.
fn startup_timezone() -> (Tz, Option<&'static str>) {
    let environment = std::env::var("TZ").ok();
    let system = iana_time_zone::get_timezone().ok();
    resolve_timezone(environment.as_deref(), system.as_deref())
}

fn resolve_timezone(environment: Option<&str>, system: Option<&str>) -> (Tz, Option<&'static str>) {
    let from_environment = environment.and_then(parse_timezone_name);
    let from_system = system.and_then(parse_timezone_name);
    match from_environment.or(from_system) {
        Some(timezone) => (timezone, None),
        None => (
            chrono_tz::UTC,
            Some("Could not detect an IANA timezone; using UTC for this session."),
        ),
    }
}

fn parse_timezone_name(value: &str) -> Option<Tz> {
    let value = value.strip_prefix(':').unwrap_or(value);
    let value = [
        "/usr/share/zoneinfo/",
        "../usr/share/zoneinfo/",
        "/usr/share/lib/zoneinfo/",
        "/etc/zoneinfo/",
        "../etc/zoneinfo/",
        "/var/db/timezone/zoneinfo/",
        "zoneinfo/",
    ]
    .iter()
    .find_map(|prefix| value.strip_prefix(prefix))
    .unwrap_or(value);
    let value = value
        .strip_prefix("posix/")
        .or_else(|| value.strip_prefix("right/"))
        .unwrap_or(value);
    value.parse().ok()
}

fn local_naive<Tz>(at: DateTime<Utc>, timezone: &Tz) -> Option<NaiveDateTime>
where
    Tz: TimeZone,
{
    let offset = timezone.offset_from_utc_datetime(&at.naive_utc()).fix();
    at.naive_utc().checked_add_offset(offset)
}

fn correction_timestamp<Tz>(at: DateTime<Utc>, timezone: &Tz) -> Option<String>
where
    Tz: TimeZone,
{
    local_naive(at, timezone).map(|local| local.format(CORRECTION_FORMAT).to_string())
}

fn parse_correction_timestamp<Tz>(
    text: &str,
    timezone: &Tz,
    original: Option<DateTime<Utc>>,
) -> Result<DateTime<Utc>, &'static str>
where
    Tz: TimeZone,
{
    let local = NaiveDateTime::parse_from_str(text, CORRECTION_FORMAT)
        .map_err(|_| "Use YYYY-MM-DD HH:MM")?;
    if local.format(CORRECTION_FORMAT).to_string() != text {
        return Err("Use YYYY-MM-DD HH:MM");
    }
    match timezone.offset_from_local_datetime(&local) {
        chrono::LocalResult::Single(offset) => local_to_utc(local, offset.fix()),
        chrono::LocalResult::Ambiguous(first, second) => {
            let wanted =
                original.map(|at| timezone.offset_from_utc_datetime(&at.naive_utc()).fix());
            let first = first.fix();
            let second = second.fix();
            match (
                wanted.is_some_and(|offset| offset == first),
                wanted.is_some_and(|offset| offset == second),
            ) {
                (true, false) => local_to_utc(local, first),
                (false, true) => local_to_utc(local, second),
                _ => Err("Ambiguous local time"),
            }
        }
        chrono::LocalResult::None => Err("Local time does not exist"),
    }
}

fn local_to_utc(local: NaiveDateTime, offset: FixedOffset) -> Result<DateTime<Utc>, &'static str> {
    local
        .checked_sub_offset(offset)
        .map(|utc| DateTime::from_naive_utc_and_offset(utc, Utc))
        .ok_or(OUTSIDE_EDITABLE_RANGE)
}

fn resolve_correction_timestamp<Tz>(
    input: &TimestampInput,
    timezone: &Tz,
    original: DateTime<Utc>,
) -> Result<DateTime<Utc>, &'static str>
where
    Tz: TimeZone,
{
    if let Some(instant) = input.adjusted_instant {
        Ok(instant)
    } else if input.is_unchanged() {
        Ok(original)
    } else {
        parse_correction_timestamp(input.text(), timezone, Some(original))
    }
}

fn adjusted_correction_timestamp<Tz>(
    input: &TimestampInput,
    delta: TimeDelta,
    timezone: &Tz,
    original: DateTime<Utc>,
) -> Result<(String, DateTime<Utc>), &'static str>
where
    Tz: TimeZone,
{
    let timestamp = match input.adjusted_instant {
        Some(instant) => instant,
        None => parse_correction_timestamp(input.text(), timezone, Some(original))?,
    };
    let adjusted = timestamp
        .checked_add_signed(delta)
        .ok_or("Timestamp is out of range")?;
    let text = correction_timestamp(adjusted, timezone).ok_or(OUTSIDE_EDITABLE_RANGE)?;
    if parse_correction_timestamp(&text, timezone, Some(adjusted)) != Ok(adjusted) {
        return Err("Adjustment cannot be represented as a local minute");
    }
    Ok((text, adjusted))
}

/// Task-list presentation state.
///
/// The two views each remember their selected task by id across view
/// switches and refreshes, so archiving and unarchiving can restore the
/// selection to the row the user was on.
pub struct App<S: TrackerApplicationService> {
    application: S,
    tasks: Vec<Task>,
    archived_tasks: Vec<Task>,
    ordering: TaskOrdering,
    view: TaskView,
    active_selection: Option<TaskId>,
    archived_selection: Option<TaskId>,
    screen: Screen,
    history: Option<History>,
    tracking: TrackingState,
    mode: Mode,
    status: Status,
    clock: Option<ElapsedClock>,
    timezone: Tz,
    frozen_offset: Option<FixedOffset>,
    lifecycle: Lifecycle,
}

impl<S: TrackerApplicationService> App<S> {
    /// Builds presentation state from an already loaded application service.
    pub fn load(application: S) -> Self {
        let ordering = TaskOrdering::default();
        let (tasks, archived_tasks) = task_lists(&application, ordering);
        let tracking = application.current_tracking().clone();
        let clock = match &tracking {
            TrackingState::Idle => None,
            TrackingState::Running { worklog } => Some(ElapsedClock::since(worklog.start())),
        };
        let (timezone, timezone_status) = startup_timezone();
        let status = match timezone_status {
            Some(message) => Status::Error(message.to_owned()),
            None if clock.is_some() => {
                Status::Info("Recovered the previous active timer".to_owned())
            }
            None => Status::Info("Ready".to_owned()),
        };
        let active_selection = tasks.first().map(|task| task.id);
        Self {
            application,
            tasks,
            archived_tasks,
            ordering,
            view: TaskView::Active,
            active_selection,
            archived_selection: None,
            screen: Screen::TaskList,
            history: None,
            tracking,
            mode: Mode::Normal,
            status,
            clock,
            timezone,
            frozen_offset: None,
            lifecycle: Lifecycle::Running,
        }
    }

    pub fn handle(&mut self, command: Command) {
        match command {
            Command::MoveUp
            | Command::MoveDown
            | Command::ShowActiveTasks
            | Command::ShowArchivedTasks
            | Command::OpenHistory
            | Command::LoadOlderWorklogs
            | Command::RefreshWorklogs
            | Command::BackToTaskList => self.handle_navigation(command),
            Command::OpenCorrection
            | Command::OpenDeletion
            | Command::SwitchCorrectionField
            | Command::MoveCursorLeft
            | Command::MoveCursorRight
            | Command::Delete
            | Command::AdjustForwardFiveMinutes
            | Command::AdjustBackwardFiveMinutes
            | Command::AdjustForwardOneHour
            | Command::AdjustBackwardOneHour => self.handle_correction_command(command),
            Command::Insert(_) | Command::Backspace => self.handle_text_input(command),
            Command::CycleOrdering => self.cycle_ordering(),
            Command::UnarchiveSelected => self.unarchive_selected(),
            Command::ToggleTracking => self.toggle_tracking(),
            Command::OpenAdd => self.open_add(),
            Command::OpenRename => self.open_rename(),
            Command::OpenArchiveConfirm => self.open_archive_confirm(),
            Command::Confirm => self.confirm(),
            Command::Cancel => self.cancel(),
            Command::Quit => self.lifecycle = Lifecycle::Quitting,
        }
    }

    fn handle_navigation(&mut self, command: Command) {
        match command {
            Command::MoveUp => self.move_up(),
            Command::MoveDown => self.move_down(),
            Command::ShowActiveTasks => self.show_tasks(TaskView::Active),
            Command::ShowArchivedTasks => self.show_tasks(TaskView::Archived),
            Command::OpenHistory => self.open_history(),
            Command::LoadOlderWorklogs => self.load_older_worklogs(),
            Command::RefreshWorklogs => self.refresh_worklogs(),
            Command::BackToTaskList => self.back_to_task_list(),
            _ => unreachable!("navigation commands are grouped by handle"),
        }
    }

    fn handle_correction_command(&mut self, command: Command) {
        match command {
            Command::OpenCorrection => self.open_correction(),
            Command::OpenDeletion => self.open_deletion(),
            Command::SwitchCorrectionField => self.edit_correction(|draft| draft.switch_field()),
            Command::MoveCursorLeft => {
                self.edit_correction(|draft| draft.focused_mut().move_left())
            }
            Command::MoveCursorRight => {
                self.edit_correction(|draft| draft.focused_mut().move_right())
            }
            Command::Delete => self.edit_correction(|draft| draft.focused_mut().delete()),
            Command::AdjustForwardFiveMinutes => self.adjust_correction(TimeDelta::minutes(5)),
            Command::AdjustBackwardFiveMinutes => self.adjust_correction(TimeDelta::minutes(-5)),
            Command::AdjustForwardOneHour => self.adjust_correction(TimeDelta::hours(1)),
            Command::AdjustBackwardOneHour => self.adjust_correction(TimeDelta::hours(-1)),
            _ => unreachable!("correction commands are grouped by handle"),
        }
    }

    fn handle_text_input(&mut self, command: Command) {
        match (command, &mut self.mode) {
            (Command::Insert(character), Mode::Input { buffer, .. })
                if buffer.chars().count() < TaskName::MAX_LEN =>
            {
                buffer.push(character)
            }
            (Command::Insert(character), Mode::Correction(draft)) => {
                draft.focused_mut().insert(character)
            }
            (Command::Backspace, Mode::Input { buffer, .. }) => {
                buffer.pop();
            }
            (Command::Backspace, Mode::Correction(draft)) => draft.focused_mut().backspace(),
            _ => {}
        }
    }

    pub fn is_running(&self) -> bool {
        self.lifecycle == Lifecycle::Running
    }

    /// The tasks of the view that is currently shown.
    pub fn tasks(&self) -> &[Task] {
        self.tasks_in(self.view)
    }

    pub fn view(&self) -> TaskView {
        self.view
    }

    /// The session-only ordering shared by the active and archived views.
    #[cfg(test)]
    pub fn ordering(&self) -> TaskOrdering {
        self.ordering
    }

    /// The concise label for the current ordering.
    pub fn ordering_label(&self) -> &'static str {
        ordering_label(self.ordering)
    }

    /// The selected row of the currently shown view.
    ///
    /// The selection is remembered by task id, so it follows the task across
    /// refreshes; a task that is no longer in this view selects nothing.
    pub fn selected(&self) -> Option<usize> {
        let id = self.selection_id()?;
        self.tasks_in(self.view)
            .iter()
            .position(|task| task.id == id)
    }

    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    pub fn correction(&self) -> Option<&CorrectionDraft> {
        match &self.mode {
            Mode::Correction(draft) => Some(draft),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn deletion(&self) -> Option<&Worklog> {
        match &self.mode {
            Mode::ConfirmDeletion { worklog } => Some(worklog),
            _ => None,
        }
    }

    #[cfg(test)]
    fn correction_mut_for_tests(&mut self) -> &mut CorrectionDraft {
        match &mut self.mode {
            Mode::Correction(draft) => draft,
            _ => panic!("correction is not open"),
        }
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn active_task_id(&self) -> Option<TaskId> {
        match &self.tracking {
            TrackingState::Idle => None,
            TrackingState::Running { worklog } => Some(worklog.task_id()),
        }
    }

    pub fn active_task_name(&self) -> Option<&str> {
        // Query the application service, not the visible list: the timer
        // header must keep naming the active task in the archived view too.
        self.task_name_for(self.active_task_id()?)
    }

    pub fn elapsed(&self) -> Option<Duration> {
        self.clock.as_ref().map(ElapsedClock::elapsed)
    }

    /// The screen the interface currently shows.
    pub fn screen(&self) -> Screen {
        self.screen
    }

    /// The open worklog history, if the history screen is shown.
    pub fn history(&self) -> Option<&History> {
        self.history.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn set_history_next_cursor_for_tests(&mut self, cursor: WorklogCursor) {
        if let Some(history) = &mut self.history {
            history.next_cursor = Some(cursor);
        }
    }

    /// The selected row of the open history.
    ///
    /// The selection is remembered by worklog id, so it follows a worklog
    /// across appends and refreshes; a worklog that is no longer loaded
    /// selects nothing.
    pub fn history_selected_index(&self) -> Option<usize> {
        self.history.as_ref()?.selected_index()
    }

    /// The name of the task whose history is open.
    ///
    /// The name is resolved from the application service, not from the
    /// visible list, so the title survives view switches and reordering.
    pub fn history_task_name(&self) -> Option<&str> {
        self.task_name_for(self.history.as_ref()?.task_id)
    }

    /// Formats a UTC instant as a local minute timestamp.
    ///
    /// Every instant converts through the session timezone snapshot, so a history
    /// that spans a daylight-saving transition shows each worklog in the
    /// local time valid when it ran. Tests freeze one fixed offset so
    /// rendering stays deterministic.
    pub fn local_time(&self, at: DateTime<Utc>) -> String {
        match self.frozen_offset {
            Some(offset) => crate::ui::local_time(at, &offset),
            None => crate::ui::local_time(at, &self.timezone),
        }
    }

    /// The duration a history row shows.
    ///
    /// A running row shares the header's monotonic clock. History queries
    /// refresh current tracking before returning, so a running row that a
    /// second client created adopts that same clock before it renders.
    /// Completed rows derive their duration from the stored interval.
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

    fn active_worklog(&self) -> Option<&tracker_domain::ActiveWorklog> {
        match &self.tracking {
            TrackingState::Idle => None,
            TrackingState::Running { worklog } => Some(worklog),
        }
    }

    /// The identifier of the running worklog, if tracking runs.
    fn active_worklog_id(&self) -> Option<WorklogId> {
        self.active_worklog().map(|worklog| worklog.id())
    }

    fn task_name_for(&self, task_id: TaskId) -> Option<&str> {
        Some(self.application.task(task_id)?.name().as_str())
    }

    fn tasks_in(&self, view: TaskView) -> &[Task] {
        match view {
            TaskView::Active => &self.tasks,
            TaskView::Archived => &self.archived_tasks,
        }
    }

    fn selection_id(&self) -> Option<TaskId> {
        match self.view {
            TaskView::Active => self.active_selection,
            TaskView::Archived => self.archived_selection,
        }
    }

    fn set_selection_id(&mut self, id: Option<TaskId>) {
        match self.view {
            TaskView::Active => self.active_selection = id,
            TaskView::Archived => self.archived_selection = id,
        }
    }

    fn selected_task(&self) -> Option<&Task> {
        self.tasks_in(self.view).get(self.selected()?)
    }

    fn move_up(&mut self) {
        if self.mode != Mode::Normal {
            return;
        }
        if self.screen == Screen::WorklogHistory {
            self.move_history_up();
            return;
        }
        let tasks = self.tasks_in(self.view);
        let index = match self.selected() {
            None => tasks.len().checked_sub(1),
            Some(0) => Some(0),
            Some(index) => Some(index - 1),
        };
        let id = index.and_then(|index| tasks.get(index)).map(|task| task.id);
        self.set_selection_id(id);
    }

    fn move_down(&mut self) {
        if self.mode != Mode::Normal {
            return;
        }
        if self.screen == Screen::WorklogHistory {
            self.move_history_down();
            return;
        }
        let tasks = self.tasks_in(self.view);
        if tasks.is_empty() {
            self.set_selection_id(None);
            return;
        }
        let last = tasks.len() - 1;
        let index = match self.selected() {
            None => 0,
            Some(index) => index.saturating_add(1).min(last),
        };
        self.set_selection_id(Some(tasks[index].id));
    }

    /// Moves the history selection up one row, without wrapping.
    fn move_history_up(&mut self) {
        let Some(history) = &mut self.history else {
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
            .map(|worklog| worklog.id());
    }

    /// Moves the history selection down one row, without wrapping.
    fn move_history_down(&mut self) {
        let Some(history) = &mut self.history else {
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

    /// Switches to the requested task list.
    ///
    /// Only normal mode switches views, and switching to the shown view does
    /// nothing. A view visited with no remembered selection starts on its
    /// first row.
    fn show_tasks(&mut self, target: TaskView) {
        if self.mode != Mode::Normal || self.screen != Screen::TaskList || self.view == target {
            return;
        }
        self.view = target;
        if self.selection_id().is_none()
            && let Some(first) = self.tasks_in(target).first()
        {
            self.set_selection_id(Some(first.id));
        }
    }

    /// Cycles the one ordering shared by both task views.
    fn cycle_ordering(&mut self) {
        if self.mode != Mode::Normal || self.screen != Screen::TaskList {
            return;
        }
        self.ordering = next_ordering(self.ordering);
        self.sync_tasks_from_application();
        self.status = Status::Info(format!("Sorted by {}", self.ordering_label()));
    }

    /// Translates Space into desired tracking state with an explicit client
    /// timestamp. The application service owns persistence and recovery.
    ///
    /// Tracking is an active-view, normal-mode action; the guard keeps a
    /// stray command from acting in the archived view or a modal.
    fn toggle_tracking(&mut self) {
        if self.mode != Mode::Normal
            || self.screen != Screen::TaskList
            || self.view != TaskView::Active
        {
            return;
        }
        let Some(task) = self.selected_task().cloned() else {
            return;
        };
        let active = self.active_worklog().cloned();
        let was_active = active.as_ref().map(|worklog| worklog.task_id());
        let occurred_at = active.as_ref().map_or_else(Utc::now, |worklog| {
            let start = worklog.start();
            let elapsed = self
                .clock
                .as_ref()
                .map_or(Duration::ZERO, ElapsedClock::elapsed);
            tracking_timestamp(start, elapsed)
        });

        let result = if was_active == Some(task.id) {
            self.application
                .clear_active_task(active.expect("active task has a worklog").id(), occurred_at)
                .map(|outcome| match outcome {
                    ClearActiveTaskOutcome::Stopped { .. }
                    | ClearActiveTaskOutcome::AlreadyIdle => ("stopped", false),
                })
        } else {
            self.application
                .set_active_task(task.id, occurred_at)
                .map(|outcome| match outcome {
                    SetActiveTaskOutcome::Started { .. } => ("started", true),
                    SetActiveTaskOutcome::Switched { .. } => ("switched", true),
                    SetActiveTaskOutcome::AlreadyActive { .. } if was_active.is_some() => {
                        ("switched", false)
                    }
                    SetActiveTaskOutcome::AlreadyActive { .. } => ("started", false),
                })
        };

        match result {
            Ok((action, fresh_active)) => {
                self.sync_from_application(fresh_active);
                self.status = match action {
                    "started" => Status::Info(format!("Started \"{}\"", task.name())),
                    "switched" => Status::Info(format!("Switched to \"{}\"", task.name())),
                    _ => Status::Info(format!("Stopped \"{}\"", task.name())),
                };
            }
            Err(error) => {
                self.sync_from_application(false);
                self.status = Status::Error(application_error_text(&error));
            }
        }
    }

    /// Copies backend-neutral query state after an application operation.
    fn sync_from_application(&mut self, fresh_active: bool) {
        self.sync_tasks_from_application();
        self.sync_tracking_from_application(fresh_active);
    }

    /// Copies current tracking while preserving a matching monotonic clock.
    fn sync_tracking_from_application(&mut self, fresh_active: bool) {
        let tracking = self.application.current_tracking().clone();
        let unchanged = tracking == self.tracking;
        if fresh_active {
            self.clock = match &tracking {
                TrackingState::Idle => None,
                TrackingState::Running { .. } => Some(ElapsedClock::anchored(Duration::ZERO)),
            };
        } else if !unchanged {
            self.clock = match &tracking {
                TrackingState::Idle => None,
                TrackingState::Running { worklog } => Some(ElapsedClock::since(worklog.start())),
            };
        }
        self.tracking = tracking;
    }

    fn sync_tasks_from_application(&mut self) {
        let previous_index = self.selected();
        let preferred = self.selection_id();
        (self.tasks, self.archived_tasks) = task_lists(&self.application, self.ordering);
        // Prefer the remembered task, then clamp the previous row to the
        // nearest row that remains, so archiving and unarchiving keep the
        // selection on a sensible neighbor.
        let visible = self.tasks_in(self.view);
        let resolved = preferred
            .and_then(|id| visible.iter().position(|task| task.id == id))
            .or_else(|| {
                previous_index
                    .filter(|_| !visible.is_empty())
                    .map(|index| index.min(visible.len() - 1))
            });
        let id = resolved
            .and_then(|index| visible.get(index))
            .map(|task| task.id);
        self.set_selection_id(id);
    }

    fn open_add(&mut self) {
        if !self.accepts_active_actions() {
            return;
        }
        self.mode = Mode::Input {
            purpose: InputPurpose::Add,
            buffer: String::new(),
        };
    }

    fn open_rename(&mut self) {
        if !self.accepts_active_actions() {
            return;
        }
        let Some(task) = self.selected_task() else {
            return;
        };
        self.mode = Mode::Input {
            purpose: InputPurpose::Rename { task_id: task.id },
            buffer: task.name().to_string(),
        };
    }

    fn open_archive_confirm(&mut self) {
        if !self.accepts_active_actions() {
            return;
        }
        let Some(task) = self.selected_task() else {
            return;
        };
        self.mode = Mode::ConfirmArchive {
            task_id: task.id,
            name: task.name().to_string(),
        };
    }

    /// Whether add, rename, archive, and tracking may act right now.
    ///
    /// These actions belong to the active view's normal mode only. The key
    /// map already refuses to emit them elsewhere; this guard is the second
    /// line of defense in command handling.
    fn accepts_active_actions(&self) -> bool {
        self.mode == Mode::Normal
            && self.screen == Screen::TaskList
            && self.view == TaskView::Active
    }

    /// Restores the selected archived task to the active list.
    ///
    /// Only the archived view's normal mode unarchives. Success stays in the
    /// archived view, selects the restored task in the active view, and
    /// clamps the archived selection. Both outcomes resynchronize from the
    /// application, so a worklog another client started shows up in the
    /// timer header and a failed write leaves no stale tracking state
    /// behind the status line.
    fn unarchive_selected(&mut self) {
        if self.mode != Mode::Normal
            || self.screen != Screen::TaskList
            || self.view != TaskView::Archived
        {
            return;
        }
        let Some(task) = self.selected_task().cloned() else {
            return;
        };
        match self.application.unarchive_task(task.id, Utc::now()) {
            Ok(TaskOutcome::Unarchived(restored)) => {
                self.active_selection = Some(restored.id);
                self.sync_from_application(false);
                self.status = Status::Info(format!("Restored \"{}\"", restored.name()));
            }
            Ok(TaskOutcome::Created(_) | TaskOutcome::Renamed(_) | TaskOutcome::Archived(_)) => {
                unreachable!("unarchive returned another task outcome")
            }
            Err(error) => {
                self.sync_from_application(false);
                self.status = Status::Error(application_error_text(&error));
            }
        }
    }

    /// Opens the worklog history of the selected task.
    ///
    /// Enter works in both task views of normal mode. An empty list selects
    /// no task and the command does nothing. A failed initial load keeps the
    /// task list on screen and reports an application error; no history
    /// state is built.
    fn open_history(&mut self) {
        if self.mode != Mode::Normal || self.screen != Screen::TaskList {
            return;
        }
        let Some(task) = self.selected_task().cloned() else {
            return;
        };
        let result = self.application.worklogs_for_task(task.id, None);
        self.sync_from_application(false);
        match result {
            Ok(page) => {
                let selected = page.worklogs.first().map(|worklog| worklog.id());
                let active_worklog_baseline =
                    active_worklog_for_task(&page.snapshot.active_worklog, task.id);
                self.history = Some(History {
                    task_id: task.id,
                    availability: HistoryAvailability::Available,
                    worklogs: page.worklogs,
                    next_cursor: page.next_cursor,
                    active_worklog_baseline,
                    selected,
                });
                self.screen = Screen::WorklogHistory;
                self.status = Status::Info(format!("History of \"{}\"", task.name()));
            }
            Err(error) => {
                self.status = Status::Error(application_error_text(&error));
            }
        }
    }

    fn open_correction(&mut self) {
        let timezone = self.timezone;
        match self.frozen_offset {
            Some(offset) => self.open_correction_in(&offset),
            None => self.open_correction_in(&timezone),
        }
    }

    fn open_deletion(&mut self) {
        if self.mode != Mode::Normal || self.screen != Screen::WorklogHistory {
            return;
        }
        let Some(worklog) = self
            .history
            .as_ref()
            .filter(|history| history.is_available())
            .and_then(|history| {
                history
                    .selected_index()
                    .map(|index| history.worklogs[index].clone())
            })
        else {
            return;
        };
        if worklog.is_active() {
            self.status = Status::Error(ACTIVE_WORKLOG_DELETE_MESSAGE.to_owned());
            return;
        }
        self.mode = Mode::ConfirmDeletion { worklog };
    }

    fn open_correction_in<Tz>(&mut self, timezone: &Tz)
    where
        Tz: TimeZone,
    {
        if self.mode != Mode::Normal || self.screen != Screen::WorklogHistory {
            return;
        }
        let Some(worklog) = self
            .history
            .as_ref()
            .filter(|history| history.is_available())
            .and_then(|history| {
                history
                    .selected_index()
                    .map(|index| &history.worklogs[index])
            })
            .cloned()
        else {
            return;
        };
        let Some(start) = correction_timestamp(worklog.start(), timezone) else {
            self.status = Status::Error(OUTSIDE_EDITABLE_RANGE.to_owned());
            return;
        };
        let end = match worklog.end().map(|end| correction_timestamp(end, timezone)) {
            Some(Some(end)) => Some(end),
            Some(None) => {
                self.status = Status::Error(OUTSIDE_EDITABLE_RANGE.to_owned());
                return;
            }
            None => None,
        };
        self.mode = Mode::Correction(CorrectionDraft::new(
            worklog.id(),
            worklog.times(),
            start,
            end,
        ));
        self.status = Status::Info("Edit the worklog timestamps".to_owned());
    }

    fn edit_correction(&mut self, edit: impl FnOnce(&mut CorrectionDraft)) {
        if let Mode::Correction(draft) = &mut self.mode {
            edit(draft);
        }
    }

    fn adjust_correction(&mut self, delta: TimeDelta) {
        let timezone = self.timezone;
        match self.frozen_offset {
            Some(offset) => self.adjust_correction_in(delta, &offset),
            None => self.adjust_correction_in(delta, &timezone),
        }
    }

    fn adjust_correction_in<Tz>(&mut self, delta: TimeDelta, timezone: &Tz)
    where
        Tz: TimeZone,
    {
        let Some(draft) = self.correction() else {
            return;
        };
        let adjusted = adjusted_correction_timestamp(
            draft.focused_input(),
            delta,
            timezone,
            draft.original(draft.focused),
        );
        match adjusted {
            Ok((text, instant)) => {
                self.edit_correction(|draft| {
                    draft.focused_mut().replace_with_adjustment(text, instant)
                });
                self.status = Status::Info("Adjusted timestamp".to_owned());
            }
            Err(message) => self.status = Status::Error(message.to_owned()),
        }
    }

    /// Appends the next bounded older page to the open history.
    ///
    /// The command does nothing once the loaded range reaches the end of
    /// the history. A failed load keeps every displayed worklog and the
    /// cursor state untouched and reports the application error, so `o`
    /// can simply be pressed again.
    fn load_older_worklogs(&mut self) {
        if self.mode != Mode::Normal || self.screen != Screen::WorklogHistory {
            return;
        }
        let Some(task_id) = self
            .history
            .as_ref()
            .filter(|history| history.is_available())
            .map(|history| history.task_id)
        else {
            return;
        };
        let Some(cursor) = self
            .history
            .as_ref()
            .and_then(|history| history.next_cursor)
        else {
            self.status = Status::Info("No older worklogs".to_owned());
            return;
        };
        let active_worklog_baseline = self
            .history
            .as_ref()
            .and_then(|history| history.active_worklog_baseline);
        match self.application.worklogs_for_task(task_id, Some(&cursor)) {
            Ok(page) => {
                let active_worklog =
                    active_worklog_for_task(&page.snapshot.active_worklog, task_id);
                if active_worklog != active_worklog_baseline {
                    self.reload_newest_history_after_change(task_id);
                    return;
                }
                let loaded = page.worklogs.len();
                let next_cursor = page.next_cursor;
                if let Some(history) = &mut self.history {
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
            Err(ApplicationError::Repository(RepositoryError::WorklogHistoryChanged {
                ..
            })) => {
                self.reload_newest_history_after_change(task_id);
            }
            Err(error) => {
                self.sync_from_application(false);
                self.status = Status::Error(application_error_text(&error));
            }
        }
    }

    /// Reloads the open history from its newest page.
    ///
    /// The reload discards the loaded older pages and the cursor. The
    /// selection follows the same worklog id when that worklog is still on
    /// the newest page, and otherwise starts on the newest row. A failed
    /// reload preserves a valid history. A reload after an invalidated
    /// continuation clears obsolete rows and marks history unavailable.
    fn refresh_worklogs(&mut self) {
        if self.mode != Mode::Normal || self.screen != Screen::WorklogHistory {
            return;
        }
        let Some(task_id) = self.history.as_ref().map(|history| history.task_id) else {
            return;
        };
        let keep = self.history.as_ref().and_then(|history| history.selected);
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

    fn reload_newest_history_after_change(&mut self, task_id: TaskId) {
        let keep = self.history.as_ref().and_then(|history| history.selected);
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

    fn replace_history_with_newest_page(
        &mut self,
        task_id: TaskId,
        keep: Option<WorklogId>,
        page: tracker_application::WorklogPage,
    ) {
        let selected = keep
            .filter(|id| page.worklogs.iter().any(|worklog| worklog.id() == *id))
            .or_else(|| page.worklogs.first().map(Worklog::id));
        let active_worklog_baseline =
            active_worklog_for_task(&page.snapshot.active_worklog, task_id);
        self.history = Some(History {
            task_id,
            availability: HistoryAvailability::Available,
            worklogs: page.worklogs,
            next_cursor: page.next_cursor,
            active_worklog_baseline,
            selected,
        });
    }

    fn mark_history_unavailable(&mut self) {
        let Some(history) = &mut self.history else {
            return;
        };
        history.availability = HistoryAvailability::Unavailable;
        history.worklogs.clear();
        history.next_cursor = None;
    }

    /// Leaves the history and returns to the task list.
    ///
    /// The task-list selection was never touched, so Escape lands on the
    /// same selected task, and the history state is discarded: opening a
    /// history always loads its newest page.
    fn back_to_task_list(&mut self) {
        if self.mode != Mode::Normal || self.screen != Screen::WorklogHistory {
            return;
        }
        self.screen = Screen::TaskList;
        self.history = None;
    }

    fn confirm(&mut self) {
        match &self.mode {
            Mode::Input { .. } => self.confirm_input(),
            Mode::ConfirmArchive { .. } => self.confirm_archive(),
            Mode::ConfirmDeletion { .. } => self.confirm_deletion(),
            Mode::Correction(_) => self.confirm_correction(),
            Mode::Normal => {}
        }
    }

    fn cancel(&mut self) {
        if matches!(self.mode, Mode::Correction(_)) {
            self.mode = Mode::Normal;
            self.status = Status::Info("Correction cancelled".to_owned());
        } else if matches!(self.mode, Mode::ConfirmDeletion { .. }) {
            self.mode = Mode::Normal;
            self.status = Status::Info("Deletion cancelled".to_owned());
        } else {
            self.mode = Mode::Normal;
        }
    }

    fn confirm_deletion(&mut self) {
        let Mode::ConfirmDeletion { worklog } = self.mode.clone() else {
            return;
        };
        let target_id = worklog.id();
        let task_id = worklog.task_id();
        let result = self
            .application
            .delete_completed_worklog(target_id, task_id, worklog.times());
        match result {
            Ok(DeleteCompletedWorklogOutcome::Deleted { .. }) => {
                self.remove_deleted_worklog(target_id);
                self.sync_tasks_from_application();
                self.mode = Mode::Normal;
                self.status = Status::Info("Deleted worklog".to_owned());
            }
            Err(error) if deletion_conflict(&error) => {
                self.mode = Mode::Normal;
                self.reload_newest_history_after_deletion(task_id, target_id, &error);
            }
            Err(error) => {
                self.sync_from_application(false);
                self.status = Status::Error(application_error_text(&error));
            }
        }
    }

    fn remove_deleted_worklog(&mut self, deleted_id: WorklogId) {
        let Some(history) = &mut self.history else {
            return;
        };
        let Some(index) = history
            .worklogs
            .iter()
            .position(|worklog| worklog.id() == deleted_id)
        else {
            history.selected = None;
            return;
        };
        history.worklogs.remove(index);
        history.selected = history
            .worklogs
            .get(index.min(history.worklogs.len().saturating_sub(1)))
            .map(Worklog::id);
    }

    fn reload_newest_history_after_deletion(
        &mut self,
        task_id: TaskId,
        target_id: WorklogId,
        error: &ApplicationError,
    ) {
        let result = self.application.worklogs_for_task(task_id, None);
        match result {
            Ok(page) => {
                let target_is_present = page
                    .worklogs
                    .iter()
                    .any(|worklog| worklog.id() == target_id);
                self.replace_history_with_newest_page(task_id, Some(target_id), page);
                self.sync_from_application(false);
                self.status = if matches_worklog_active(error) {
                    Status::Error(ACTIVE_WORKLOG_DELETE_MESSAGE.to_owned())
                } else if matches_worklog_not_found(error) {
                    if target_is_present {
                        Status::Error(
                            "Worklog was not found. Press d to confirm deletion again.".to_owned(),
                        )
                    } else {
                        Status::Error("Worklog was not found. History was refreshed.".to_owned())
                    }
                } else if target_is_present {
                    Status::Error("Worklog changed. Press d to confirm deletion again.".to_owned())
                } else {
                    Status::Error("Worklog changed. History was refreshed.".to_owned())
                };
            }
            Err(refresh_error) => {
                self.mark_history_unavailable();
                self.sync_from_application(false);
                self.status = if matches_worklog_active(error) {
                    Status::Error(ACTIVE_WORKLOG_DELETE_MESSAGE.to_owned())
                } else {
                    Status::Error(format!(
                        "Worklog changed, but history refresh failed: {}",
                        application_error_text(&refresh_error)
                    ))
                };
            }
        }
    }

    fn confirm_correction(&mut self) {
        let timezone = self.timezone;
        match self.frozen_offset {
            Some(offset) => self.confirm_correction_in(&offset),
            None => self.confirm_correction_in(&timezone),
        }
    }

    fn confirm_correction_in<Tz>(&mut self, timezone: &Tz)
    where
        Tz: TimeZone,
    {
        let Mode::Correction(draft) = self.mode.clone() else {
            return;
        };
        let start = match resolve_correction_timestamp(&draft.start, timezone, draft.original_start)
        {
            Ok(start) => start,
            Err(message) => {
                self.status = Status::Error(format!("Start: {message}"));
                return;
            }
        };
        let end = match draft.end.as_ref() {
            Some(input) => {
                let original = draft
                    .original_end
                    .expect("completed corrections have an end");
                match resolve_correction_timestamp(input, timezone, original) {
                    Ok(end) => Some(end),
                    Err(message) => {
                        self.status = Status::Error(format!("End: {message}"));
                        return;
                    }
                }
            }
            None => None,
        };
        let occurred_at = Utc::now();
        let replacement = WorklogTimes::new(start, end);
        match self
            .application
            .correct_worklog(draft.id, draft.expected, replacement, occurred_at)
        {
            Ok(CorrectWorklogOutcome::Corrected { worklog }) => {
                let task_id = self
                    .history
                    .as_ref()
                    .map(|history| history.task_id)
                    .expect("a correction belongs to an open history");
                let corrected_id = worklog.id();
                self.mode = Mode::Normal;
                match self.application.worklogs_for_task(task_id, None) {
                    Ok(page) => {
                        self.replace_history_with_newest_page(task_id, Some(corrected_id), page);
                        self.sync_tasks_from_application();
                        self.sync_tracking_after_history_reload();
                        self.status = Status::Info("Corrected worklog".to_owned());
                    }
                    Err(_) => {
                        self.mark_history_unavailable();
                        self.sync_from_application(false);
                        self.status = Status::Error(
                            "Correction saved, but history refresh failed".to_owned(),
                        );
                    }
                }
            }
            Err(error) => {
                self.sync_from_application(false);
                self.status = Status::Error(correction_error_text(&error));
            }
        }
    }

    fn sync_tracking_after_history_reload(&mut self) {
        self.sync_tracking_after_history_reload_at(Utc::now(), Instant::now());
    }

    fn sync_tracking_after_history_reload_at(
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

    fn confirm_input(&mut self) {
        let Mode::Input { purpose, buffer } = self.mode.clone() else {
            return;
        };
        let name = match TaskName::new(&buffer) {
            Ok(name) => name,
            Err(error) => {
                self.status = Status::Error(task_name_error_text(error));
                return;
            }
        };
        let occurred_at = Utc::now();
        let result = match purpose {
            InputPurpose::Add => self.application.create_task(name, occurred_at),
            InputPurpose::Rename { task_id } => {
                self.application.rename_task(task_id, name, occurred_at)
            }
        };
        match result {
            Ok(TaskOutcome::Created(task)) => {
                self.sync_tasks_from_application();
                self.set_selection_id(Some(task.id));
                self.mode = Mode::Normal;
                self.status = Status::Info(format!("Added \"{}\"", task.name()));
            }
            Ok(TaskOutcome::Renamed(task)) => {
                self.sync_tasks_from_application();
                self.mode = Mode::Normal;
                self.status = Status::Info(format!("Renamed to \"{}\"", task.name()));
            }
            Ok(TaskOutcome::Archived(_)) => unreachable!("input cannot archive a task"),
            Ok(TaskOutcome::Unarchived(_)) => unreachable!("input cannot unarchive a task"),
            Err(error) => self.status = Status::Error(application_error_text(&error)),
        }
    }

    fn confirm_archive(&mut self) {
        let Mode::ConfirmArchive { task_id, .. } = self.mode.clone() else {
            return;
        };
        match self.application.archive_task(task_id, Utc::now()) {
            Ok(TaskOutcome::Archived(task)) => {
                self.sync_from_application(false);
                // The archived view will select the newly archived task when
                // opened.
                self.archived_selection = Some(task.id);
                self.mode = Mode::Normal;
                self.status = Status::Info(format!("Archived \"{}\"", task.name()));
            }
            Err(ApplicationError::Domain(TrackingError::TaskIsActive { .. })) => {
                self.sync_from_application(false);
                self.mode = Mode::Normal;
                self.status = Status::Error("The active task cannot be archived".to_owned());
            }
            Err(error) => {
                self.sync_from_application(false);
                self.status = Status::Error(application_error_text(&error));
            }
            Ok(TaskOutcome::Created(_) | TaskOutcome::Renamed(_) | TaskOutcome::Unarchived(_)) => {
                unreachable!("archive returned another task outcome")
            }
        }
    }

    #[cfg(test)]
    fn set_timezone_for_tests(&mut self, timezone: Tz) {
        self.timezone = timezone;
        self.frozen_offset = None;
    }

    #[cfg(test)]
    pub(crate) fn freeze_elapsed_for_tests(&mut self, base: Duration) {
        let anchor = Instant::now()
            .checked_add(Duration::from_secs(86_400))
            .expect("a test clock can advance one day");
        self.clock = Some(ElapsedClock::at_anchor(base, anchor));
    }

    /// Freezes one fixed display offset for deterministic rendering.
    ///
    /// Tests replace the session timezone with one fixed offset, so rendered
    /// times never depend on the host timezone or a daylight-saving rule.
    #[cfg(test)]
    pub(crate) fn freeze_offset_for_tests(&mut self, offset: FixedOffset) {
        self.frozen_offset = Some(offset);
    }
}

/// Builds both views from one ordered application read.
fn task_lists<S: TrackerApplicationService>(
    application: &S,
    ordering: TaskOrdering,
) -> (Vec<Task>, Vec<Task>) {
    application
        .tasks(ordering)
        .into_iter()
        .map(|item| item.task)
        .partition(|task| !task.is_archived())
}

#[cfg(test)]
mod tests {
    use chrono::{MappedLocalTime, NaiveDate, NaiveDateTime, TimeDelta};
    use std::cell::Cell;
    use tracker_application::{
        CorrectWorklogOutcome, RepositoryError, TaskListItem, TaskOperations, TaskOutcome,
        TaskQueries, TrackerApplication, TrackingOperations, WorklogCursor, WorklogOperations,
        WorklogPage, WorklogPageSnapshot, WorklogQueries,
    };
    use tracker_domain::{
        ActiveWorklog, Task, TaskId, TaskName, Worklog, WorklogCorrectionError, WorklogId,
        WorklogTimes,
    };
    use tracker_storage::SqliteRepository;

    use super::*;

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).unwrap()
    }

    fn app_with(names: &[&str]) -> App<TrackerApplication<SqliteRepository>> {
        let repository = SqliteRepository::open_in_memory().unwrap();
        for name in names {
            repository
                .create_task(Task::create(
                    TaskId::generate(),
                    TaskName::new(name).unwrap(),
                    at(100),
                ))
                .unwrap();
        }
        App::load(TrackerApplication::load(repository).unwrap())
    }

    fn text(status: &Status) -> &str {
        match status {
            Status::Info(text) | Status::Error(text) => text,
        }
    }

    struct TestService {
        tasks: Vec<Task>,
        tracking: TrackingState,
        fail_create: bool,
        fail_rename: bool,
        fail_archive: bool,
        fail_unarchive: bool,
        archive_activates: Option<DateTime<Utc>>,
        unarchive_activates: Option<DateTime<Utc>>,
        set_returns_already_active: bool,
        set_timestamp: Option<DateTime<Utc>>,
        latest_work_starts: Vec<(TaskId, DateTime<Utc>)>,
        worklog_pages: Vec<Result<WorklogPage, ApplicationError>>,
        worklog_reads: Cell<usize>,
        correction_error: Option<ApplicationError>,
        correction_calls: Vec<(WorklogId, WorklogTimes, WorklogTimes, DateTime<Utc>)>,
        deletion_error: Option<ApplicationError>,
        deletion_calls: Vec<(WorklogId, TaskId, WorklogTimes)>,
        authoritative_worklogs: Vec<Worklog>,
        clear_calls: Vec<(WorklogId, DateTime<Utc>)>,
    }

    impl TestService {
        fn with_tasks(tasks: Vec<Task>) -> Self {
            Self {
                tasks,
                tracking: TrackingState::Idle,
                fail_create: false,
                fail_rename: false,
                fail_archive: false,
                fail_unarchive: false,
                archive_activates: None,
                unarchive_activates: None,
                set_returns_already_active: false,
                set_timestamp: None,
                latest_work_starts: Vec::new(),
                worklog_pages: Vec::new(),
                worklog_reads: Cell::new(0),
                correction_error: None,
                correction_calls: Vec::new(),
                deletion_error: None,
                deletion_calls: Vec::new(),
                authoritative_worklogs: Vec::new(),
                clear_calls: Vec::new(),
            }
        }

        fn failure() -> ApplicationError {
            ApplicationError::Repository(RepositoryError::Backend {
                message: "write failed".to_owned(),
            })
        }
    }

    impl TaskQueries for TestService {
        fn tasks(&self, ordering: TaskOrdering) -> Vec<TaskListItem> {
            let mut items = self
                .tasks
                .iter()
                .cloned()
                .map(|task| TaskListItem {
                    latest_work_start: self
                        .latest_work_starts
                        .iter()
                        .find(|(id, _)| *id == task.id)
                        .map(|(_, start)| *start),
                    task,
                })
                .collect::<Vec<_>>();
            ordering.sort_items(&mut items);
            items
        }

        fn task(&self, id: TaskId) -> Option<&Task> {
            self.tasks.iter().find(|task| task.id == id)
        }
    }

    impl TaskOperations for TestService {
        fn create_task(
            &mut self,
            name: TaskName,
            occurred_at: DateTime<Utc>,
        ) -> Result<TaskOutcome, ApplicationError> {
            if self.fail_create {
                return Err(Self::failure());
            }
            let task = Task::create(
                TaskId::from_uuid(uuid::Uuid::from_u128(2)),
                name,
                occurred_at,
            );
            self.tasks.push(task.clone());
            Ok(TaskOutcome::Created(task))
        }

        fn rename_task(
            &mut self,
            id: TaskId,
            name: TaskName,
            occurred_at: DateTime<Utc>,
        ) -> Result<TaskOutcome, ApplicationError> {
            if self.fail_rename {
                return Err(Self::failure());
            }
            let task = self
                .tasks
                .iter_mut()
                .find(|task| task.id == id)
                .expect("test task exists");
            task.rename(name, occurred_at);
            Ok(TaskOutcome::Renamed(task.clone()))
        }

        fn archive_task(
            &mut self,
            id: TaskId,
            occurred_at: DateTime<Utc>,
        ) -> Result<TaskOutcome, ApplicationError> {
            if self.fail_archive {
                return Err(Self::failure());
            }
            let task = self
                .tasks
                .iter_mut()
                .find(|task| task.id == id)
                .expect("test task exists");
            task.archive(occurred_at);
            let archived = task.clone();
            if let Some(start) = self.archive_activates {
                let other = self
                    .tasks
                    .iter()
                    .find(|task| task.id != id && !task.is_archived())
                    .expect("the test service needs another active task");
                self.tracking = TrackingState::Running {
                    worklog: ActiveWorklog::begin(WorklogId::generate(), other.id, start),
                };
            }
            Ok(TaskOutcome::Archived(archived))
        }

        fn unarchive_task(
            &mut self,
            id: TaskId,
            occurred_at: DateTime<Utc>,
        ) -> Result<TaskOutcome, ApplicationError> {
            if self.fail_unarchive {
                return Err(Self::failure());
            }
            let task = self
                .tasks
                .iter_mut()
                .find(|task| task.id == id)
                .expect("test task exists");
            task.restore(occurred_at);
            // Stands in for a second client that restored the task and
            // started tracking it before this unarchive ran.
            if let Some(start) = self.unarchive_activates {
                self.tracking = TrackingState::Running {
                    worklog: ActiveWorklog::begin(
                        WorklogId::from_uuid(uuid::Uuid::from_u128(20)),
                        task.id,
                        start,
                    ),
                };
            }
            Ok(TaskOutcome::Unarchived(task.clone()))
        }
    }

    impl TrackingOperations for TestService {
        fn current_tracking(&self) -> &TrackingState {
            &self.tracking
        }

        fn set_active_task(
            &mut self,
            task_id: TaskId,
            occurred_at: DateTime<Utc>,
        ) -> Result<SetActiveTaskOutcome, ApplicationError> {
            let started_at = self.set_timestamp.unwrap_or(occurred_at);
            if let Some((_, latest)) = self
                .latest_work_starts
                .iter_mut()
                .find(|(id, _)| *id == task_id)
            {
                *latest = (*latest).max(started_at);
            } else {
                self.latest_work_starts.push((task_id, started_at));
            }
            let worklog = Worklog::begin(
                WorklogId::from_uuid(uuid::Uuid::from_u128(99)),
                task_id,
                started_at,
            );
            let active = ActiveWorklog::begin(worklog.id(), task_id, started_at);
            self.tracking = TrackingState::Running {
                worklog: active.clone(),
            };
            if self.set_returns_already_active {
                Ok(SetActiveTaskOutcome::AlreadyActive { worklog: active })
            } else {
                Ok(SetActiveTaskOutcome::Started { worklog })
            }
        }

        fn clear_active_task(
            &mut self,
            expected_active: WorklogId,
            occurred_at: DateTime<Utc>,
        ) -> Result<ClearActiveTaskOutcome, ApplicationError> {
            self.clear_calls.push((expected_active, occurred_at));
            self.tracking = TrackingState::Idle;
            Ok(ClearActiveTaskOutcome::AlreadyIdle)
        }
    }

    impl WorklogOperations for TestService {
        fn correct_worklog(
            &mut self,
            id: WorklogId,
            expected: WorklogTimes,
            replacement: WorklogTimes,
            occurred_at: DateTime<Utc>,
        ) -> Result<CorrectWorklogOutcome, ApplicationError> {
            self.correction_calls
                .push((id, expected, replacement, occurred_at));
            if let Some(error) = self.correction_error.clone() {
                return Err(error);
            }
            let original = self
                .worklog_pages
                .iter()
                .filter_map(|page| page.as_ref().ok())
                .flat_map(|page| &page.worklogs)
                .find(|worklog| worklog.id() == id)
                .cloned()
                .expect("the corrected worklog exists in a queued page");
            let corrected = original.corrected(replacement, occurred_at)?;
            if corrected.is_active() {
                self.tracking = TrackingState::Running {
                    worklog: ActiveWorklog::begin(
                        corrected.id(),
                        corrected.task_id(),
                        corrected.start(),
                    ),
                };
            }
            Ok(CorrectWorklogOutcome::Corrected { worklog: corrected })
        }

        fn delete_completed_worklog(
            &mut self,
            id: WorklogId,
            task_id: TaskId,
            expected: WorklogTimes,
        ) -> Result<DeleteCompletedWorklogOutcome, ApplicationError> {
            self.deletion_calls.push((id, task_id, expected));
            let index = self
                .authoritative_worklogs
                .iter()
                .position(|worklog| worklog.id() == id)
                .ok_or(ApplicationError::WorklogDeletionWrite {
                    write: RepositoryError::WorklogNotFound { id },
                })?;
            let stored = &self.authoritative_worklogs[index];
            if stored.is_active() {
                return Err(ApplicationError::WorklogDeletionWrite {
                    write: RepositoryError::WorklogIsActive { id },
                });
            }
            if stored.task_id() != task_id || stored.times() != expected {
                return Err(ApplicationError::WorklogDeletionWrite {
                    write: RepositoryError::WorklogChanged { id },
                });
            }
            if let Some(error) = self.deletion_error.clone() {
                return Err(error);
            }
            let worklog = self.authoritative_worklogs.remove(index);
            let latest = self
                .authoritative_worklogs
                .iter()
                .filter(|candidate| candidate.task_id() == task_id)
                .map(Worklog::start)
                .max();
            if let Some(position) = self
                .latest_work_starts
                .iter()
                .position(|(candidate, _)| *candidate == task_id)
            {
                if let Some(latest) = latest {
                    self.latest_work_starts[position].1 = latest;
                } else {
                    self.latest_work_starts.remove(position);
                }
            } else if let Some(latest) = latest {
                self.latest_work_starts.push((task_id, latest));
            }
            Ok(DeleteCompletedWorklogOutcome::Deleted { worklog })
        }
    }

    impl WorklogQueries for TestService {
        fn worklogs_for_task(
            &mut self,
            _task_id: TaskId,
            _after: Option<&WorklogCursor>,
        ) -> Result<WorklogPage, ApplicationError> {
            // Each read consumes the next queued page, so one service can
            // answer an initial load, several older pages, and failures.
            let read = self.worklog_reads.get();
            self.worklog_reads.set(read + 1);
            let result = self.worklog_pages.get(read).cloned().unwrap_or_else(|| {
                Ok(WorklogPage {
                    worklogs: Vec::new(),
                    snapshot: WorklogPageSnapshot {
                        requested_task_latest_work_start: None,
                        active_worklog: None,
                        active_task_latest_work_start: None,
                    },
                    next_cursor: None,
                })
            });
            if let Ok(page) = &result {
                self.tracking = match &page.snapshot.active_worklog {
                    Some(worklog) => TrackingState::Running {
                        worklog: ActiveWorklog::begin(
                            worklog.id(),
                            worklog.task_id(),
                            worklog.start(),
                        ),
                    },
                    None => TrackingState::Idle,
                };
            }
            result
        }
    }

    #[test]
    fn application_and_tui_recover_tracking_after_a_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tracker.db");
        let task = Task::create(TaskId::generate(), TaskName::new("alpha").unwrap(), at(100));
        {
            let repository = SqliteRepository::open(&path).unwrap();
            repository.create_task(task.clone()).unwrap();
            repository
                .insert_worklog(&Worklog::begin(
                    WorklogId::generate(),
                    task.id,
                    DateTime::from_timestamp(100, 0).unwrap(),
                ))
                .unwrap();
        }
        let app =
            App::load(TrackerApplication::load(SqliteRepository::open(&path).unwrap()).unwrap());
        assert_eq!(app.active_task_id(), Some(task.id));
        assert_eq!(
            app.status(),
            &Status::Info("Recovered the previous active timer".to_owned())
        );
    }

    #[test]
    fn fresh_and_empty_apps_have_safe_selection() {
        let app = app_with(&["one", "two"]);
        assert_eq!(app.selected(), Some(0));
        assert_eq!(app.status(), &Status::Info("Ready".to_owned()));
        let mut empty = app_with(&[]);
        empty.handle(Command::MoveDown);
        empty.handle(Command::MoveUp);
        assert_eq!(empty.selected(), None);
    }

    #[test]
    fn selection_movement_stays_inside_the_task_list() {
        let mut app = app_with(&["one", "two", "three"]);
        app.handle(Command::MoveUp);
        assert_eq!(app.selected(), Some(0));
        for _ in 0..5 {
            app.handle(Command::MoveDown);
        }
        assert_eq!(app.selected(), Some(2));
        app.handle(Command::MoveUp);
        assert_eq!(app.selected(), Some(1));
        app.active_selection = None;
        app.handle(Command::MoveUp);
        assert_eq!(app.selected(), Some(2));
    }

    #[test]
    fn adding_a_task_updates_the_list_and_selection() {
        let mut app = app_with(&["one"]);
        app.handle(Command::OpenAdd);
        for character in "new task".chars() {
            app.handle(Command::Insert(character));
        }
        app.handle(Command::Confirm);
        assert_eq!(app.mode(), &Mode::Normal);
        assert_eq!(app.selected(), Some(0));
        assert_eq!(
            app.tasks()[app.selected().unwrap()].name().as_str(),
            "new task"
        );
        assert_eq!(app.status(), &Status::Info("Added \"new task\"".to_owned()));
    }

    #[test]
    fn adding_in_the_middle_selects_the_new_task() {
        let task = |tag, name| {
            Task::create(
                TaskId::from_uuid(uuid::Uuid::from_u128(tag)),
                TaskName::new(name).unwrap(),
                at(100),
            )
        };
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "one"),
            task(3, "three"),
        ]));
        app.handle(Command::MoveDown);
        app.handle(Command::OpenAdd);
        app.handle(Command::Insert('t'));
        app.handle(Command::Confirm);
        assert_eq!(
            app.tasks().iter().map(|task| task.id).collect::<Vec<_>>(),
            vec![
                TaskId::from_uuid(uuid::Uuid::from_u128(2)),
                TaskId::from_uuid(uuid::Uuid::from_u128(1)),
                TaskId::from_uuid(uuid::Uuid::from_u128(3)),
            ]
        );
        assert_eq!(app.selected(), Some(0));
    }

    #[test]
    fn refreshing_tasks_preserves_the_selected_task_after_reordering() {
        let task = |tag, name| {
            Task::create(
                TaskId::from_uuid(uuid::Uuid::from_u128(tag)),
                TaskName::new(name).unwrap(),
                at(100),
            )
        };
        let selected = task(3, "three");
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "one"),
            selected.clone(),
        ]));
        app.handle(Command::MoveDown);
        app.application.tasks.insert(1, task(2, "two"));
        app.sync_tasks_from_application();
        assert_eq!(app.selected(), Some(2));
        assert_eq!(app.tasks()[2].id, selected.id);
    }

    #[test]
    fn failed_writes_keep_the_active_modal_and_input_buffer() {
        let task = Task::create(
            TaskId::from_uuid(uuid::Uuid::from_u128(1)),
            TaskName::new("one").unwrap(),
            at(100),
        );
        let mut service = TestService::with_tasks(vec![task]);
        service.fail_create = true;
        service.fail_rename = true;
        service.fail_archive = true;
        let mut app = App::load(service);

        app.handle(Command::OpenAdd);
        for character in "blocked".chars() {
            app.handle(Command::Insert(character));
        }
        app.handle(Command::Confirm);
        assert!(matches!(
            app.mode(),
            Mode::Input {
                purpose: InputPurpose::Add,
                buffer,
            } if buffer == "blocked"
        ));
        assert_eq!(app.status(), &Status::Error("Storage error".to_owned()));
        app.handle(Command::Cancel);

        app.handle(Command::OpenRename);
        app.handle(Command::Confirm);
        assert!(matches!(
            app.mode(),
            Mode::Input {
                purpose: InputPurpose::Rename { .. },
                buffer,
            } if buffer == "one"
        ));
        assert_eq!(app.status(), &Status::Error("Storage error".to_owned()));
        app.handle(Command::Cancel);

        app.handle(Command::OpenArchiveConfirm);
        app.handle(Command::Confirm);
        assert!(matches!(app.mode(), Mode::ConfirmArchive { .. }));
        assert_eq!(app.status(), &Status::Error("Storage error".to_owned()));
    }

    #[test]
    fn invalid_input_keeps_the_dialog_and_reports_the_same_text() {
        let mut app = app_with(&["one"]);
        app.handle(Command::OpenAdd);
        app.handle(Command::Confirm);
        assert!(matches!(app.mode(), Mode::Input { .. }));
        assert_eq!(
            app.status(),
            &Status::Error("The task name must not be empty".to_owned())
        );
    }

    #[test]
    fn renaming_a_task_keeps_its_row_selected() {
        let mut app = app_with(&["old"]);
        app.handle(Command::OpenRename);
        for _ in 0..3 {
            app.handle(Command::Backspace);
        }
        for character in "new".chars() {
            app.handle(Command::Insert(character));
        }
        app.handle(Command::Confirm);
        assert_eq!(app.tasks()[0].name().as_str(), "new");
        assert_eq!(app.selected(), Some(0));
        assert_eq!(app.status(), &Status::Info("Renamed to \"new\"".to_owned()));
    }

    #[test]
    fn archiving_hides_the_task_and_clamps_selection() {
        let mut app = app_with(&["alpha", "beta"]);
        app.handle(Command::MoveDown);
        app.handle(Command::OpenArchiveConfirm);
        app.handle(Command::Confirm);
        assert_eq!(app.tasks().len(), 1);
        assert_eq!(app.tasks()[0].name().as_str(), "alpha");
        assert_eq!(app.selected(), Some(0));
        assert_eq!(app.status(), &Status::Info("Archived \"beta\"".to_owned()));
    }

    #[test]
    fn a_successful_archive_copies_tracking_refreshed_by_the_application() {
        let alpha = task(1, "alpha");
        let beta = task(2, "beta");
        let mut app = App::load(TestService::with_tasks(vec![alpha.clone(), beta.clone()]));
        app.application.archive_activates = Some(at(200));

        app.handle(Command::OpenArchiveConfirm);
        app.handle(Command::Confirm);

        assert_eq!(app.active_task_id(), Some(beta.id));
        assert_eq!(app.active_task_name(), Some("beta"));
        assert_eq!(app.status(), &Status::Info("Archived \"alpha\"".to_owned()));
    }

    #[test]
    fn archiving_the_active_task_keeps_the_timer() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::ToggleTracking);
        let active = app.active_task_id();
        app.handle(Command::OpenArchiveConfirm);
        app.handle(Command::Confirm);
        assert_eq!(app.active_task_id(), active);
        assert_eq!(app.tasks().len(), 1);
        assert_eq!(
            app.status(),
            &Status::Error("The active task cannot be archived".to_owned())
        );
    }

    #[test]
    fn space_starts_stops_and_restarts_with_separate_worklogs() {
        let mut app = app_with(&["alpha"]);
        let task_id = app.tasks()[0].id;
        app.handle(Command::ToggleTracking);
        assert_eq!(app.active_task_id(), Some(task_id));
        assert_eq!(text(app.status()), "Started \"alpha\"");
        app.handle(Command::ToggleTracking);
        assert_eq!(app.active_task_id(), None);
        assert_eq!(text(app.status()), "Stopped \"alpha\"");
        app.handle(Command::ToggleTracking);
        let worklogs = app
            .application
            .worklogs_for_task(task_id, None)
            .unwrap()
            .worklogs;
        assert_eq!(worklogs.len(), 2);
        assert_ne!(worklogs[0].id(), worklogs[1].id());
    }

    #[test]
    fn space_switches_to_another_task() {
        let mut app = app_with(&["alpha", "beta"]);
        app.handle(Command::ToggleTracking);
        app.handle(Command::MoveDown);
        let beta = app.tasks()[1].id;
        app.handle(Command::ToggleTracking);
        assert_eq!(app.active_task_id(), Some(beta));
        assert_eq!(
            app.status(),
            &Status::Info("Switched to \"beta\"".to_owned())
        );
    }

    #[test]
    fn tracking_outcomes_choose_the_right_status_and_clock_anchor() {
        let task = |tag, name| {
            Task::create(
                TaskId::from_uuid(uuid::Uuid::from_u128(tag)),
                TaskName::new(name).unwrap(),
                at(100),
            )
        };
        let old = DateTime::from_timestamp(100, 0).unwrap();

        let mut started_service = TestService::with_tasks(vec![task(1, "alpha")]);
        started_service.set_timestamp = Some(old);
        let mut started = App::load(started_service);
        started.handle(Command::ToggleTracking);
        assert_eq!(
            started.status(),
            &Status::Info("Started \"alpha\"".to_owned())
        );
        assert!(started.elapsed().unwrap() < Duration::from_secs(1));

        let mut existing_service = TestService::with_tasks(vec![task(1, "alpha")]);
        existing_service.set_returns_already_active = true;
        existing_service.set_timestamp = Some(old);
        let mut existing = App::load(existing_service);
        existing.handle(Command::ToggleTracking);
        assert_eq!(
            existing.status(),
            &Status::Info("Started \"alpha\"".to_owned())
        );
        assert!(existing.elapsed().unwrap() > Duration::from_secs(60));

        let alpha = task(1, "alpha");
        let beta = task(2, "beta");
        let mut switched_service = TestService::with_tasks(vec![alpha.clone(), beta.clone()]);
        switched_service.tracking = TrackingState::Running {
            worklog: ActiveWorklog::begin(
                WorklogId::from_uuid(uuid::Uuid::from_u128(10)),
                alpha.id,
                old,
            ),
        };
        switched_service.set_returns_already_active = true;
        switched_service.set_timestamp = Some(old);
        let mut switched = App::load(switched_service);
        switched.handle(Command::MoveDown);
        switched.handle(Command::ToggleTracking);
        assert_eq!(
            switched.status(),
            &Status::Info("Switched to \"beta\"".to_owned())
        );
    }

    #[test]
    fn switching_uses_one_monotonic_timestamp_for_both_worklogs() {
        let mut app = app_with(&["alpha", "beta"]);
        let alpha = app.tasks()[0].id;
        let beta = app.tasks()[1].id;
        app.handle(Command::ToggleTracking);
        app.freeze_elapsed_for_tests(Duration::from_secs(125));
        app.handle(Command::MoveDown);
        app.handle(Command::ToggleTracking);
        let alpha_worklog = app
            .application
            .worklogs_for_task(alpha, None)
            .unwrap()
            .worklogs
            .pop()
            .unwrap();
        let beta_worklog = app
            .application
            .worklogs_for_task(beta, None)
            .unwrap()
            .worklogs
            .pop()
            .unwrap();
        assert_eq!(alpha_worklog.end(), Some(beta_worklog.start()));
    }

    #[test]
    fn a_backward_wall_clock_jump_persists_a_nonnegative_duration() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tracker.db");
        let task = Task::create(TaskId::generate(), TaskName::new("alpha").unwrap(), at(100));
        let start = DateTime::from_timestamp_micros(Utc::now().timestamp_micros()).unwrap()
            + TimeDelta::hours(1);
        let repository = SqliteRepository::open(&path).unwrap();
        repository.create_task(task.clone()).unwrap();
        repository
            .insert_worklog(&Worklog::begin(WorklogId::generate(), task.id, start))
            .unwrap();
        let mut app = App::load(TrackerApplication::load(repository).unwrap());

        app.handle(Command::ToggleTracking);

        let stored = app
            .application
            .worklogs_for_task(task.id, None)
            .unwrap()
            .worklogs
            .remove(0);
        let duration = (stored.end().unwrap() - stored.start()).to_std().unwrap();
        assert!(duration < Duration::from_secs(60), "got {duration:?}");
        assert!(stored.end().unwrap() >= start);
    }

    #[test]
    fn a_forward_wall_clock_jump_persists_the_displayed_duration() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tracker.db");
        let task = Task::create(TaskId::generate(), TaskName::new("alpha").unwrap(), at(100));
        let start = Utc::now() - TimeDelta::hours(2);
        let repository = SqliteRepository::open(&path).unwrap();
        repository.create_task(task.clone()).unwrap();
        repository
            .insert_worklog(&Worklog::begin(WorklogId::generate(), task.id, start))
            .unwrap();
        let mut app = App::load(TrackerApplication::load(repository).unwrap());
        app.freeze_elapsed_for_tests(Duration::from_secs(5));
        let shown = app.elapsed().unwrap();

        app.handle(Command::ToggleTracking);

        let stored = app
            .application
            .worklogs_for_task(task.id, None)
            .unwrap()
            .worklogs
            .remove(0);
        let duration = (stored.end().unwrap() - stored.start()).to_std().unwrap();
        assert!(
            duration >= shown,
            "persisted {duration:?} < shown {shown:?}"
        );
        assert!(
            duration - shown < Duration::from_secs(2),
            "persisted {duration:?} must match shown {shown:?}"
        );
        assert!(
            duration < Duration::from_secs(60),
            "the two-hour wall-clock gap leaked into the worklog: {duration:?}"
        );
    }

    #[test]
    fn quitting_in_a_dialog_does_not_stop_tracking() {
        let mut app = app_with(&["alpha"]);
        app.handle(Command::ToggleTracking);
        app.handle(Command::OpenAdd);
        app.handle(Command::Quit);
        assert!(!app.is_running());
        assert!(app.active_task_id().is_some());
    }

    #[test]
    fn quitting_from_every_mode_leaves_tracking_active() {
        for command in [
            None,
            Some(Command::OpenAdd),
            Some(Command::OpenRename),
            Some(Command::OpenArchiveConfirm),
        ] {
            let task = Task::create(
                TaskId::from_uuid(uuid::Uuid::from_u128(1)),
                TaskName::new("one").unwrap(),
                at(100),
            );
            let mut service = TestService::with_tasks(vec![task.clone()]);
            service.tracking = TrackingState::Running {
                worklog: ActiveWorklog::begin(
                    WorklogId::from_uuid(uuid::Uuid::from_u128(10)),
                    task.id,
                    DateTime::from_timestamp(100, 0).unwrap(),
                ),
            };
            let mut app = App::load(service);
            if let Some(command) = command {
                app.handle(command);
            }
            app.handle(Command::Quit);
            assert!(!app.is_running());
            assert_eq!(app.active_task_id(), Some(task.id));
        }
    }

    #[test]
    fn input_is_bounded_to_the_domain_limit() {
        let mut app = app_with(&[]);
        app.handle(Command::OpenAdd);
        for character in "x".repeat(TaskName::MAX_LEN + 10).chars() {
            app.handle(Command::Insert(character));
        }
        app.handle(Command::Confirm);
        assert_eq!(
            app.tasks()[0].name().as_str().chars().count(),
            TaskName::MAX_LEN
        );
    }

    #[test]
    fn elapsed_clock_and_client_timestamp_share_one_duration() {
        let clock = ElapsedClock::anchored(Duration::from_secs(100));
        assert_eq!(clock.at(Duration::from_secs(5)), Duration::from_secs(105));
        let start = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            tracking_timestamp(start, Duration::from_secs(5)),
            start + TimeDelta::seconds(5)
        );
    }

    #[test]
    fn future_clock_anchors_at_zero_and_timestamp_overflow_saturates() {
        let now = Utc::now();
        assert_eq!(
            ElapsedClock::base_since(now + TimeDelta::seconds(1), now),
            Duration::ZERO
        );
        assert_eq!(
            ElapsedClock::base_since(now - TimeDelta::seconds(5), now),
            Duration::from_secs(5)
        );
        assert_eq!(
            tracking_timestamp(DateTime::<Utc>::MAX_UTC, Duration::MAX),
            DateTime::<Utc>::MAX_UTC
        );
    }

    #[test]
    fn cancel_commands_preserve_expected_state() {
        let mut app = app_with(&["one"]);
        app.handle(Command::OpenAdd);
        app.handle(Command::Insert('x'));
        app.handle(Command::Cancel);
        assert_eq!(app.mode(), &Mode::Normal);
        assert_eq!(app.tasks().len(), 1);
    }

    fn task(tag: u128, name: &str) -> Task {
        Task::create(
            TaskId::from_uuid(uuid::Uuid::from_u128(tag)),
            TaskName::new(name).unwrap(),
            at(100),
        )
    }

    fn archived_task(tag: u128, name: &str) -> Task {
        let mut task = task(tag, name);
        assert!(task.archive(at(100)));
        task
    }

    fn stamped_task(
        tag: u128,
        name: &str,
        archived: bool,
        created_at: i64,
        updated_at: i64,
    ) -> Task {
        Task::rehydrate(
            TaskId::from_uuid(uuid::Uuid::from_u128(tag)),
            TaskName::new(name).unwrap(),
            archived,
            at(created_at),
            at(updated_at),
        )
        .unwrap()
    }

    #[test]
    fn ordering_defaults_cycles_and_is_shared_by_both_views() {
        let tasks = vec![
            stamped_task(1, "older active", false, 100, 900),
            stamped_task(2, "newer active", false, 800, 800),
            stamped_task(3, "older archived", true, 200, 700),
            stamped_task(4, "newer archived", true, 600, 600),
        ];
        let mut service = TestService::with_tasks(tasks);
        service.latest_work_starts = vec![(TaskId::from_uuid(uuid::Uuid::from_u128(3)), at(1_000))];
        let mut app = App::load(service);

        assert_eq!(app.ordering(), TaskOrdering::RecentlyWorked);
        assert_eq!(app.tasks()[0].name().as_str(), "newer active");
        app.handle(Command::ShowArchivedTasks);
        assert_eq!(app.tasks()[0].name().as_str(), "older archived");
        app.handle(Command::ShowActiveTasks);
        app.handle(Command::CycleOrdering);
        assert_eq!(app.ordering(), TaskOrdering::RecentlyUpdated);
        assert_eq!(app.tasks()[0].name().as_str(), "older active");

        app.handle(Command::ShowArchivedTasks);
        assert_eq!(app.ordering(), TaskOrdering::RecentlyUpdated);
        assert_eq!(app.tasks()[0].name().as_str(), "older archived");
        app.handle(Command::CycleOrdering);
        assert_eq!(app.ordering(), TaskOrdering::RecentlyCreated);
        assert_eq!(app.tasks()[0].name().as_str(), "newer archived");
        app.handle(Command::CycleOrdering);
        assert_eq!(app.ordering(), TaskOrdering::RecentlyWorked);
    }

    #[test]
    fn sorting_preserves_each_views_selected_task_id() {
        let active = stamped_task(1, "active selection", false, 100, 900);
        let archived = stamped_task(3, "archived selection", true, 100, 900);
        let mut app = App::load(TestService::with_tasks(vec![
            active.clone(),
            stamped_task(2, "new active", false, 800, 800),
            archived.clone(),
            stamped_task(4, "new archived", true, 800, 800),
        ]));

        app.handle(Command::MoveDown);
        assert_eq!(app.tasks()[app.selected().unwrap()].id, active.id);
        app.handle(Command::ShowArchivedTasks);
        app.handle(Command::MoveDown);
        assert_eq!(app.tasks()[app.selected().unwrap()].id, archived.id);

        app.handle(Command::CycleOrdering);
        assert_eq!(app.tasks()[app.selected().unwrap()].id, archived.id);
        app.handle(Command::ShowActiveTasks);
        assert_eq!(app.tasks()[app.selected().unwrap()].id, active.id);
    }

    #[test]
    fn input_and_confirmation_modes_block_sorting() {
        let mut app = App::load(TestService::with_tasks(vec![task(1, "alpha")]));
        app.handle(Command::OpenAdd);
        app.handle(Command::CycleOrdering);
        assert_eq!(app.ordering(), TaskOrdering::RecentlyWorked);
        assert!(matches!(app.mode(), Mode::Input { .. }));

        app.handle(Command::Cancel);
        app.handle(Command::OpenArchiveConfirm);
        app.handle(Command::CycleOrdering);
        assert_eq!(app.ordering(), TaskOrdering::RecentlyWorked);
        assert!(matches!(app.mode(), Mode::ConfirmArchive { .. }));
    }

    #[test]
    fn start_switch_and_stop_keep_selection_on_the_operated_task() {
        let mut app = app_with(&["alpha", "beta"]);
        let alpha = app
            .tasks()
            .iter()
            .find(|task| task.name().as_str() == "alpha")
            .unwrap()
            .id;
        let beta = app
            .tasks()
            .iter()
            .find(|task| task.name().as_str() == "beta")
            .unwrap()
            .id;

        app.handle(Command::MoveDown);
        app.handle(Command::ToggleTracking);
        assert_eq!(app.tasks()[0].id, beta);
        assert_eq!(app.tasks()[app.selected().unwrap()].id, beta);

        app.handle(Command::MoveDown);
        app.handle(Command::ToggleTracking);
        assert_eq!(app.tasks()[0].id, alpha);
        assert_eq!(app.tasks()[app.selected().unwrap()].id, alpha);

        app.handle(Command::ToggleTracking);
        assert_eq!(app.tasks()[0].id, alpha, "stopping does not reorder");
        assert_eq!(app.tasks()[app.selected().unwrap()].id, alpha);
    }

    #[test]
    fn rename_reorders_recently_updated_and_keeps_the_task_selected() {
        let alpha = task(1, "alpha");
        let beta = task(2, "beta");
        let mut app = App::load(TestService::with_tasks(vec![alpha, beta.clone()]));
        app.handle(Command::CycleOrdering);
        app.handle(Command::MoveDown);
        app.handle(Command::OpenRename);
        app.handle(Command::Backspace);
        app.handle(Command::Insert('x'));
        app.handle(Command::Confirm);

        assert_eq!(app.ordering(), TaskOrdering::RecentlyUpdated);
        assert_eq!(app.tasks()[0].id, beta.id);
        assert_eq!(app.tasks()[app.selected().unwrap()].id, beta.id);
    }

    #[test]
    fn the_app_starts_in_the_active_view_and_switches_directionally() {
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "alpha"),
            archived_task(3, "gone"),
        ]));
        assert_eq!(app.view(), TaskView::Active);
        assert_eq!(app.tasks().len(), 1, "the archived task is not visible");

        app.handle(Command::ShowArchivedTasks);
        assert_eq!(app.view(), TaskView::Archived);
        assert_eq!(app.tasks().len(), 1);

        // l on the archived view is idempotent.
        app.handle(Command::ShowArchivedTasks);
        assert_eq!(app.view(), TaskView::Archived);

        app.handle(Command::ShowActiveTasks);
        assert_eq!(app.view(), TaskView::Active);

        // h on the active view is idempotent.
        app.handle(Command::ShowActiveTasks);
        assert_eq!(app.view(), TaskView::Active);
    }

    #[test]
    fn a_same_view_switch_does_not_select_for_a_view_without_a_memory() {
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "alpha"),
            task(2, "beta"),
            archived_task(3, "gone"),
        ]));
        // No remembered selection: the view's own command is a no-op and
        // must not invent one from the first row.
        app.active_selection = None;
        app.handle(Command::ShowActiveTasks);
        assert_eq!(app.selected(), None);

        app.handle(Command::ShowArchivedTasks);
        assert_eq!(app.selected(), Some(0), "a fresh view starts on row one");
        app.archived_selection = None;
        app.handle(Command::ShowArchivedTasks);
        assert_eq!(app.selected(), None);
    }

    #[test]
    fn each_view_remembers_its_selection_across_switches() {
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "alpha"),
            task(2, "beta"),
            archived_task(3, "gone"),
        ]));
        app.handle(Command::MoveDown);
        app.handle(Command::ShowArchivedTasks);
        assert_eq!(
            app.selected(),
            Some(0),
            "a fresh view starts on its first row"
        );

        app.handle(Command::ShowActiveTasks);
        assert_eq!(app.selected(), Some(1), "the active selection came back");
        assert_eq!(
            app.tasks()[1].id,
            TaskId::from_uuid(uuid::Uuid::from_u128(2))
        );

        app.handle(Command::ShowArchivedTasks);
        assert_eq!(app.selected(), Some(0), "the archived selection came back");
    }

    #[test]
    fn archived_movement_stays_inside_the_archived_list() {
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "alpha"),
            archived_task(3, "gone"),
            archived_task(4, "also gone"),
        ]));
        app.handle(Command::ShowArchivedTasks);
        assert_eq!(app.selected(), Some(0));
        app.handle(Command::MoveDown);
        assert_eq!(app.selected(), Some(1));
        app.handle(Command::MoveDown);
        assert_eq!(app.selected(), Some(1));
        app.handle(Command::MoveUp);
        assert_eq!(app.selected(), Some(0));
    }

    #[test]
    fn an_empty_view_has_no_selection() {
        let mut app = App::load(TestService::with_tasks(vec![task(1, "alpha")]));
        app.handle(Command::ShowArchivedTasks);
        assert_eq!(app.selected(), None);
        app.handle(Command::MoveDown);
        app.handle(Command::MoveUp);
        assert_eq!(app.selected(), None);
    }

    #[test]
    fn the_archived_view_refuses_active_actions_in_command_handling() {
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "alpha"),
            archived_task(3, "gone"),
        ]));
        app.handle(Command::ShowArchivedTasks);

        app.handle(Command::ToggleTracking);
        app.handle(Command::OpenAdd);
        app.handle(Command::OpenRename);
        app.handle(Command::OpenArchiveConfirm);

        assert_eq!(app.mode(), &Mode::Normal);
        assert_eq!(app.view(), TaskView::Archived);
        assert_eq!(app.active_task_id(), None, "no tracking was started");

        // Movement still works after the refused actions.
        app.handle(Command::ShowActiveTasks);
        app.handle(Command::ShowArchivedTasks);
        app.handle(Command::MoveDown);
        app.handle(Command::MoveUp);
        assert_eq!(app.selected(), Some(0));
    }

    #[test]
    fn the_active_view_refuses_unarchiving() {
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "alpha"),
            archived_task(3, "gone"),
        ]));
        app.handle(Command::UnarchiveSelected);

        assert_eq!(app.view(), TaskView::Active);
        assert_eq!(app.status(), &Status::Info("Ready".to_owned()));
        assert_eq!(app.tasks().len(), 1, "nothing was unarchived");
    }

    #[test]
    fn a_modal_in_the_archived_view_refuses_unarchiving() {
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "alpha"),
            archived_task(3, "gone"),
        ]));
        app.handle(Command::ShowArchivedTasks);
        // No command path reaches a modal on the archived view; the guard
        // must still refuse one for whenever the key map changes.
        app.mode = Mode::Input {
            purpose: InputPurpose::Add,
            buffer: String::new(),
        };
        app.handle(Command::UnarchiveSelected);

        assert!(matches!(app.mode(), Mode::Input { .. }));
        assert_eq!(app.view(), TaskView::Archived);
        assert_eq!(app.tasks().len(), 1, "nothing was unarchived");
        assert_eq!(app.status(), &Status::Info("Ready".to_owned()));
    }

    #[test]
    fn modals_block_view_switching_and_unarchiving() {
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "alpha"),
            archived_task(3, "gone"),
        ]));
        app.handle(Command::OpenAdd);
        app.handle(Command::Insert('x'));

        // Each switch is asserted immediately, so a guard that only blocks
        // one of the two commands cannot cancel the other out.
        app.handle(Command::ShowArchivedTasks);
        assert_eq!(app.view(), TaskView::Active, "the modal blocks the switch");

        app.handle(Command::ShowActiveTasks);
        assert_eq!(app.view(), TaskView::Active);

        app.handle(Command::UnarchiveSelected);
        assert!(matches!(app.mode(), Mode::Input { .. }));
        assert_eq!(app.view(), TaskView::Active);
        assert!(matches!(app.mode(), Mode::Input { buffer, .. } if buffer == "x"));

        app.handle(Command::Cancel);
        app.handle(Command::OpenArchiveConfirm);
        app.handle(Command::UnarchiveSelected);
        assert!(matches!(app.mode(), Mode::ConfirmArchive { .. }));
        assert_eq!(app.view(), TaskView::Active);
    }

    #[test]
    fn archiving_remembers_the_task_in_the_archived_view_and_clamps() {
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "alpha"),
            task(2, "beta"),
            task(3, "gamma"),
        ]));
        app.handle(Command::MoveDown);
        app.handle(Command::OpenArchiveConfirm);
        app.handle(Command::Confirm);

        assert_eq!(app.view(), TaskView::Active);
        assert_eq!(app.selected(), Some(1), "the selection clamped to gamma");

        app.handle(Command::ShowArchivedTasks);
        assert_eq!(app.selected(), Some(0));
        assert_eq!(
            app.tasks()[0].id,
            TaskId::from_uuid(uuid::Uuid::from_u128(2))
        );
    }

    #[test]
    fn archiving_the_last_row_clamps_to_the_previous_row() {
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "alpha"),
            task(2, "beta"),
        ]));
        app.handle(Command::MoveDown);
        app.handle(Command::OpenArchiveConfirm);
        app.handle(Command::Confirm);
        assert_eq!(app.tasks().len(), 1);
        assert_eq!(app.selected(), Some(0));
        assert_eq!(
            app.tasks()[0].id,
            TaskId::from_uuid(uuid::Uuid::from_u128(1))
        );
    }

    #[test]
    fn unarchiving_stays_in_the_archived_view_and_reports_restored() {
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "alpha"),
            task(2, "beta"),
            archived_task(3, "gone"),
        ]));
        app.handle(Command::ShowArchivedTasks);
        app.handle(Command::UnarchiveSelected);

        assert_eq!(app.view(), TaskView::Archived);
        assert_eq!(app.status(), &Status::Info("Restored \"gone\"".to_owned()));
        assert_eq!(app.tasks().len(), 0, "the restored task left the list");
        assert_eq!(app.selected(), None, "the archived list is empty now");

        app.handle(Command::ShowActiveTasks);
        assert_eq!(app.tasks().len(), 3);
        assert_eq!(app.selected(), Some(2), "the restored task is selected");
        assert_eq!(
            app.tasks()[2].id,
            TaskId::from_uuid(uuid::Uuid::from_u128(3))
        );
    }

    #[test]
    fn unarchiving_clamps_the_archived_selection_to_the_nearest_row() {
        let mut app = App::load(TestService::with_tasks(vec![
            task(1, "alpha"),
            archived_task(3, "gone"),
            archived_task(4, "also gone"),
        ]));
        app.handle(Command::ShowArchivedTasks);
        app.handle(Command::MoveDown);
        app.handle(Command::UnarchiveSelected);

        assert_eq!(app.view(), TaskView::Archived);
        assert_eq!(app.tasks().len(), 1);
        assert_eq!(app.selected(), Some(0));
        assert_eq!(
            app.tasks()[0].id,
            TaskId::from_uuid(uuid::Uuid::from_u128(3))
        );
    }

    #[test]
    fn a_failed_unarchive_keeps_the_view_selection_and_reports_the_error() {
        let mut service = TestService::with_tasks(vec![task(1, "alpha"), archived_task(3, "gone")]);
        service.fail_unarchive = true;
        let mut app = App::load(service);
        // Another client's activity reaches the service after the app loaded;
        // the failed write must still resynchronize the TUI's tracking state.
        app.application.tracking = TrackingState::Running {
            worklog: ActiveWorklog::begin(
                WorklogId::from_uuid(uuid::Uuid::from_u128(20)),
                TaskId::from_uuid(uuid::Uuid::from_u128(3)),
                DateTime::from_timestamp(100, 0).unwrap(),
            ),
        };
        app.handle(Command::ShowArchivedTasks);
        app.handle(Command::UnarchiveSelected);

        assert_eq!(app.view(), TaskView::Archived);
        assert_eq!(app.selected(), Some(0));
        assert_eq!(
            app.tasks()[0].id,
            TaskId::from_uuid(uuid::Uuid::from_u128(3))
        );
        assert_eq!(
            app.active_task_id(),
            Some(TaskId::from_uuid(uuid::Uuid::from_u128(3))),
            "the failed unarchive still refreshes tracking state"
        );
        assert_eq!(app.status(), &Status::Error("Storage error".to_owned()));
    }

    #[test]
    fn unarchiving_refreshes_a_timer_another_client_started() {
        let mut service = TestService::with_tasks(vec![task(1, "alpha"), archived_task(3, "gone")]);
        service.unarchive_activates = Some(DateTime::from_timestamp(100, 0).unwrap());
        let mut app = App::load(service);
        app.handle(Command::ShowArchivedTasks);
        app.handle(Command::UnarchiveSelected);

        assert_eq!(app.view(), TaskView::Archived);
        assert_eq!(
            app.active_task_id(),
            Some(TaskId::from_uuid(uuid::Uuid::from_u128(3))),
            "the concurrent worklog reached the TUI's tracking state"
        );
        assert_eq!(app.active_task_name(), Some("gone"));
        assert!(
            app.elapsed().unwrap() > Duration::from_secs(60),
            "the clock anchored to the concurrent worklog's start"
        );
        assert_eq!(app.status(), &Status::Info("Restored \"gone\"".to_owned()));

        assert_eq!(app.tasks().len(), 0, "the restored task left the list");
        app.handle(Command::ShowActiveTasks);
        assert_eq!(app.selected(), Some(1), "the restored task is selected");
        assert_eq!(
            app.tasks()[1].id,
            TaskId::from_uuid(uuid::Uuid::from_u128(3))
        );
    }

    #[test]
    fn the_timer_header_resolves_the_active_task_outside_the_visible_list() {
        let mut app = App::load(TestService::with_tasks(vec![archived_task(3, "gone")]));
        let alpha = task(1, "alpha");
        app.application.tasks.push(alpha.clone());
        app.tracking = TrackingState::Running {
            worklog: ActiveWorklog::begin(
                WorklogId::from_uuid(uuid::Uuid::from_u128(10)),
                alpha.id,
                DateTime::from_timestamp(100, 0).unwrap(),
            ),
        };
        app.handle(Command::ShowArchivedTasks);

        assert_eq!(app.view(), TaskView::Archived);
        assert_eq!(app.tasks().len(), 1, "only archived tasks are listed");
        assert_eq!(app.active_task_name(), Some("alpha"));
    }

    fn worklog_id(tag: u128) -> WorklogId {
        WorklogId::from_uuid(uuid::Uuid::from_u128(tag))
    }

    /// A stopped worklog for the task, started at `start` and running one
    /// minute.
    fn history_worklog(tag: u128, task_id: TaskId, start: i64) -> Worklog {
        Worklog::new(worklog_id(tag), task_id, at(start), Some(at(start + 60))).unwrap()
    }

    fn cursor(start: i64, tag: u128) -> WorklogCursor {
        WorklogCursor {
            task_id: TaskId::from_uuid(uuid::Uuid::from_u128(1)),
            start: at(start),
            id: worklog_id(tag),
            revision: 0,
        }
    }

    fn page(worklogs: Vec<Worklog>, next_cursor: Option<WorklogCursor>) -> WorklogPage {
        let requested_task_latest_work_start = worklogs.iter().map(Worklog::start).max();
        let active_worklog = worklogs.iter().find(|worklog| worklog.is_active()).cloned();
        WorklogPage {
            snapshot: WorklogPageSnapshot {
                requested_task_latest_work_start,
                active_task_latest_work_start: active_worklog.as_ref().map(Worklog::start),
                active_worklog,
            },
            worklogs,
            next_cursor,
        }
    }

    fn page_with_active(
        worklogs: Vec<Worklog>,
        active_worklog: Option<Worklog>,
        next_cursor: Option<WorklogCursor>,
    ) -> WorklogPage {
        let requested_task_latest_work_start = worklogs.iter().map(Worklog::start).max();
        WorklogPage {
            snapshot: WorklogPageSnapshot {
                requested_task_latest_work_start,
                active_task_latest_work_start: active_worklog.as_ref().map(Worklog::start),
                active_worklog,
            },
            worklogs,
            next_cursor,
        }
    }

    #[test]
    fn deletion_opening_requires_normal_history_mode() {
        let alpha = task(1, "alpha");
        let target = history_worklog(10, alpha.id, 100);
        let mut service = TestService::with_tasks(vec![alpha]);
        service.worklog_pages = vec![Ok(page(vec![target.clone()], None))];
        service.authoritative_worklogs = vec![target];
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);

        app.mode = Mode::ConfirmArchive {
            task_id: TaskId::from_uuid(uuid::Uuid::from_u128(1)),
            name: "alpha".to_owned(),
        };
        app.handle(Command::OpenDeletion);
        assert!(matches!(app.mode(), Mode::ConfirmArchive { .. }));

        app.mode = Mode::Normal;
        app.screen = Screen::TaskList;
        app.handle(Command::OpenDeletion);
        assert_eq!(app.mode(), &Mode::Normal);
    }

    #[test]
    fn completed_deletion_uses_the_snapshot_and_preserves_the_cursor() {
        let alpha = task(1, "alpha");
        let target = history_worklog(12, alpha.id, 300);
        let following = history_worklog(11, alpha.id, 200);
        let cursor = cursor(200, 11);
        let mut service = TestService::with_tasks(vec![alpha.clone()]);
        service.latest_work_starts = vec![(alpha.id, target.start())];
        service.worklog_pages = vec![Ok(page(
            vec![target.clone(), following.clone()],
            Some(cursor),
        ))];
        service.authoritative_worklogs = vec![target.clone(), following.clone()];
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);
        app.handle(Command::OpenDeletion);

        assert_eq!(app.deletion(), Some(&target));
        app.handle(Command::Confirm);

        assert_eq!(
            app.application.deletion_calls,
            vec![(target.id(), alpha.id, target.times())]
        );
        let history = app.history().unwrap();
        assert_eq!(history.worklogs, vec![following.clone()]);
        assert_eq!(history.next_cursor, Some(cursor));
        assert_eq!(app.history_selected_index(), Some(0));
        assert_eq!(app.mode(), &Mode::Normal);
        assert_eq!(app.status(), &Status::Info("Deleted worklog".to_owned()));
        assert_eq!(
            app.application.tasks(TaskOrdering::RecentlyWorked)[0].latest_work_start,
            Some(following.start())
        );
        assert_eq!(app.application.worklog_reads.get(), 1);
    }

    #[test]
    fn deletion_selection_moves_to_the_following_row_or_previous_row() {
        let alpha = task(1, "alpha");
        let rows = vec![
            history_worklog(13, alpha.id, 300),
            history_worklog(12, alpha.id, 200),
            history_worklog(11, alpha.id, 100),
        ];
        let mut service = TestService::with_tasks(vec![alpha.clone()]);
        service.worklog_pages = vec![Ok(page(rows.clone(), None))];
        service.authoritative_worklogs = rows.clone();
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);
        app.handle(Command::MoveDown);
        app.handle(Command::OpenDeletion);
        app.handle(Command::Confirm);
        assert_eq!(
            app.history()
                .unwrap()
                .worklogs
                .iter()
                .map(Worklog::id)
                .collect::<Vec<_>>(),
            vec![worklog_id(13), worklog_id(11)]
        );
        assert_eq!(app.history_selected_index(), Some(1));
        assert_eq!(
            app.application.tasks(TaskOrdering::RecentlyWorked)[0].latest_work_start,
            Some(at(300))
        );

        app.handle(Command::OpenDeletion);
        app.handle(Command::Confirm);
        assert_eq!(app.history_selected_index(), Some(0));
        assert_eq!(
            app.application.tasks(TaskOrdering::RecentlyWorked)[0].latest_work_start,
            Some(at(300))
        );

        app.handle(Command::OpenDeletion);
        app.handle(Command::Confirm);
        assert_eq!(app.history_selected_index(), None);
        assert!(app.history().unwrap().worklogs.is_empty());
        assert_eq!(
            app.application.tasks(TaskOrdering::RecentlyWorked)[0].latest_work_start,
            None
        );
    }

    #[test]
    fn repeated_successful_deletions_update_the_latest_work_aggregate() {
        let alpha = task(1, "alpha");
        let rows = vec![
            history_worklog(13, alpha.id, 300),
            history_worklog(12, alpha.id, 200),
            history_worklog(11, alpha.id, 100),
        ];
        let mut service = TestService::with_tasks(vec![alpha.clone()]);
        service.worklog_pages = vec![Ok(page(rows.clone(), None))];
        service.authoritative_worklogs = rows;
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);

        for expected in [Some(at(200)), Some(at(100)), None] {
            app.handle(Command::OpenDeletion);
            app.handle(Command::Confirm);
            assert_eq!(
                app.application.tasks(TaskOrdering::RecentlyWorked)[0].latest_work_start,
                expected
            );
        }
    }

    #[test]
    fn test_deletion_uses_authoritative_worklogs_and_validates_the_snapshot() {
        let alpha = task(1, "alpha");
        let beta = task(2, "beta");
        let target = history_worklog(10, alpha.id, 300);
        let older = history_worklog(11, alpha.id, 200);
        let active = Worklog::begin(worklog_id(12), alpha.id, at(100));
        let mut service = TestService::with_tasks(vec![alpha.clone(), beta.clone()]);
        service.authoritative_worklogs = vec![target.clone(), older.clone(), active.clone()];

        assert_eq!(
            service.delete_completed_worklog(worklog_id(99), alpha.id, target.times()),
            Err(ApplicationError::WorklogDeletionWrite {
                write: RepositoryError::WorklogNotFound { id: worklog_id(99) }
            })
        );
        assert_eq!(
            service.delete_completed_worklog(active.id(), alpha.id, active.times()),
            Err(ApplicationError::WorklogDeletionWrite {
                write: RepositoryError::WorklogIsActive { id: active.id() }
            })
        );
        assert_eq!(
            service.delete_completed_worklog(target.id(), beta.id, target.times()),
            Err(ApplicationError::WorklogDeletionWrite {
                write: RepositoryError::WorklogChanged { id: target.id() }
            })
        );
        assert_eq!(
            service.delete_completed_worklog(
                target.id(),
                alpha.id,
                WorklogTimes::new(at(301), Some(at(361)))
            ),
            Err(ApplicationError::WorklogDeletionWrite {
                write: RepositoryError::WorklogChanged { id: target.id() }
            })
        );

        let outcome = service
            .delete_completed_worklog(target.id(), alpha.id, target.times())
            .unwrap();
        assert!(matches!(
            outcome,
            DeleteCompletedWorklogOutcome::Deleted { .. }
        ));
        assert_eq!(service.authoritative_worklogs, vec![older, active]);
        assert_eq!(
            service
                .latest_work_starts
                .iter()
                .find(|(task_id, _)| *task_id == alpha.id)
                .map(|(_, start)| *start),
            Some(at(200))
        );
    }

    #[test]
    fn deleting_the_loaded_newest_row_uses_an_unloaded_older_row_for_latest_work() {
        let alpha = task(1, "alpha");
        let newest = history_worklog(10, alpha.id, 300);
        let older = history_worklog(11, alpha.id, 200);
        let mut service = TestService::with_tasks(vec![alpha.clone()]);
        service.latest_work_starts = vec![(alpha.id, newest.start())];
        service.worklog_pages = vec![Ok(page(vec![newest.clone()], Some(cursor(300, 10))))];
        service.authoritative_worklogs = vec![newest.clone(), older.clone()];
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);
        app.handle(Command::OpenDeletion);
        app.handle(Command::Confirm);

        assert!(app.history().unwrap().worklogs.is_empty());
        assert_eq!(app.history().unwrap().next_cursor, Some(cursor(300, 10)));
        assert_eq!(
            app.application.tasks(TaskOrdering::RecentlyWorked)[0].latest_work_start,
            Some(older.start())
        );
    }

    #[test]
    fn active_history_rows_are_not_deletable_and_archived_rows_are() {
        let alpha = task(1, "alpha");
        let active = Worklog::begin(worklog_id(10), alpha.id, at(300));
        let mut service = TestService::with_tasks(vec![alpha.clone()]);
        service.worklog_pages = vec![Ok(page(vec![active.clone()], None))];
        service.authoritative_worklogs = vec![active.clone()];
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);
        app.handle(Command::OpenDeletion);
        assert_eq!(app.mode(), &Mode::Normal);
        assert_eq!(
            app.status(),
            &Status::Error(ACTIVE_WORKLOG_DELETE_MESSAGE.to_owned())
        );
        assert!(app.application.deletion_calls.is_empty());

        let archived = archived_task(2, "archived");
        let completed = history_worklog(11, archived.id, 100);
        let mut service = TestService::with_tasks(vec![archived.clone()]);
        service.worklog_pages = vec![Ok(page(vec![completed.clone()], None))];
        service.authoritative_worklogs = vec![completed.clone()];
        let mut app = App::load(service);
        app.handle(Command::ShowArchivedTasks);
        app.handle(Command::OpenHistory);
        app.handle(Command::OpenDeletion);
        assert_eq!(app.deletion(), Some(&completed));
    }

    #[test]
    fn deletion_cancel_and_ordinary_failure_keep_the_snapshot() {
        let alpha = task(1, "alpha");
        let target = history_worklog(10, alpha.id, 100);
        let mut service = TestService::with_tasks(vec![alpha.clone()]);
        service.worklog_pages = vec![Ok(page(vec![target.clone()], None))];
        service.authoritative_worklogs = vec![target.clone()];
        service.deletion_error = Some(ApplicationError::WorklogDeletionWrite {
            write: RepositoryError::Backend {
                message: "private backend detail".to_owned(),
            },
        });
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);
        app.handle(Command::OpenDeletion);
        app.handle(Command::Confirm);
        assert_eq!(app.deletion(), Some(&target));
        assert_eq!(text(app.status()), "Storage error");
        assert!(!text(app.status()).contains("private backend detail"));
        app.handle(Command::Cancel);
        assert_eq!(app.mode(), &Mode::Normal);
        assert_eq!(app.status(), &Status::Info("Deletion cancelled".to_owned()));
    }

    #[test]
    fn deletion_recovery_failure_keeps_the_snapshot_without_refreshing_history() {
        let alpha = task(1, "alpha");
        let target = history_worklog(10, alpha.id, 100);
        let mut service = TestService::with_tasks(vec![alpha.clone()]);
        service.worklog_pages = vec![Ok(page(vec![target.clone()], None))];
        service.authoritative_worklogs = vec![target.clone()];
        service.deletion_error = Some(ApplicationError::WorklogDeletionRecovery {
            write: RepositoryError::WorklogChanged { id: target.id() },
            recovery: RepositoryError::Backend {
                message: "recovery secret".to_owned(),
            },
        });
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);
        app.handle(Command::OpenDeletion);
        app.handle(Command::Confirm);

        assert_eq!(app.deletion(), Some(&target));
        assert_eq!(app.history().unwrap().worklogs, vec![target]);
        assert_eq!(app.application.worklog_reads.get(), 1);
        assert_eq!(
            text(app.status()),
            "Deletion failed: Worklog changed in another client. State recovery failed: Storage error."
        );
        assert!(!text(app.status()).contains("recovery secret"));
    }

    #[test]
    fn stale_deletion_refreshes_and_requires_confirmation_again() {
        let alpha = task(1, "alpha");
        let target = history_worklog(10, alpha.id, 100);
        let newest = history_worklog(10, alpha.id, 120);
        let mut service = TestService::with_tasks(vec![alpha.clone()]);
        service.worklog_pages = vec![
            Ok(page(vec![target.clone()], Some(cursor(100, 10)))),
            Ok(page(vec![newest.clone()], None)),
        ];
        service.authoritative_worklogs = vec![target.clone()];
        service.deletion_error = Some(ApplicationError::WorklogDeletionWrite {
            write: RepositoryError::WorklogNotFound { id: target.id() },
        });
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);
        app.handle(Command::OpenDeletion);
        app.handle(Command::Confirm);

        assert_eq!(app.mode(), &Mode::Normal);
        assert_eq!(app.history().unwrap().worklogs, vec![newest]);
        assert_eq!(app.history_selected_index(), Some(0));
        assert_eq!(
            text(app.status()),
            "Worklog was not found. Press d to confirm deletion again."
        );
        assert_eq!(app.application.worklog_reads.get(), 2);
    }

    #[test]
    fn changed_deletion_of_a_missing_row_reports_a_refresh() {
        let alpha = task(1, "alpha");
        let target = history_worklog(10, alpha.id, 100);
        let replacement = history_worklog(11, alpha.id, 200);
        let mut service = TestService::with_tasks(vec![alpha.clone()]);
        service.worklog_pages = vec![
            Ok(page(vec![target.clone()], None)),
            Ok(page(vec![replacement], None)),
        ];
        service.authoritative_worklogs = vec![target.clone()];
        service.deletion_error = Some(ApplicationError::WorklogDeletionWrite {
            write: RepositoryError::WorklogChanged { id: target.id() },
        });
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);
        app.handle(Command::OpenDeletion);
        app.handle(Command::Confirm);

        assert_eq!(
            text(app.status()),
            "Worklog changed. History was refreshed."
        );
    }

    #[test]
    fn failed_stale_refresh_discards_rows_and_marks_history_unavailable() {
        let alpha = task(1, "alpha");
        let target = history_worklog(10, alpha.id, 100);
        let mut service = TestService::with_tasks(vec![alpha.clone()]);
        service.worklog_pages = vec![
            Ok(page(vec![target.clone()], None)),
            Err(TestService::failure()),
        ];
        service.authoritative_worklogs = vec![target.clone()];
        service.deletion_error = Some(ApplicationError::WorklogDeletionWrite {
            write: RepositoryError::WorklogNotFound { id: target.id() },
        });
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);
        app.handle(Command::OpenDeletion);
        app.handle(Command::Confirm);

        let history = app.history().unwrap();
        assert_eq!(history.availability, HistoryAvailability::Unavailable);
        assert!(history.worklogs.is_empty());
        assert_eq!(history.next_cursor, None);
        assert_eq!(app.mode(), &Mode::Normal);
        assert_eq!(
            text(app.status()),
            "Worklog changed, but history refresh failed: Storage error"
        );
        assert!(!text(app.status()).contains("private backend detail"));
        app.handle(Command::OpenDeletion);
        assert_eq!(app.mode(), &Mode::Normal);
    }

    #[test]
    fn deleting_loaded_pages_then_loading_older_selects_the_first_appended_row() {
        let alpha = task(1, "alpha");
        let loaded = (1..=50)
            .map(|tag| history_worklog(tag, alpha.id, 2_000 - tag as i64))
            .collect::<Vec<_>>();
        let older = history_worklog(51, alpha.id, 1_900);
        let mut service = TestService::with_tasks(vec![alpha.clone()]);
        service.worklog_pages = vec![
            Ok(page(loaded.clone(), Some(cursor(1_950, 50)))),
            Ok(page(vec![older.clone()], None)),
        ];
        service.authoritative_worklogs = loaded.clone();
        service.authoritative_worklogs.push(older.clone());
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);

        for _ in 0..50 {
            app.handle(Command::OpenDeletion);
            app.handle(Command::Confirm);
        }

        let history = app.history().unwrap();
        assert!(history.worklogs.is_empty());
        assert_eq!(history.next_cursor, Some(cursor(1_950, 50)));
        assert_eq!(app.history_selected_index(), None);

        app.handle(Command::LoadOlderWorklogs);

        let history = app.history().unwrap();
        assert_eq!(history.worklogs, vec![older]);
        assert_eq!(history.next_cursor, None);
        assert_eq!(app.history_selected_index(), Some(0));
    }

    #[test]
    fn active_race_refreshes_history_and_keeps_confirmation_closed() {
        let alpha = task(1, "alpha");
        let completed = history_worklog(10, alpha.id, 100);
        let active = Worklog::begin(completed.id(), alpha.id, at(100));
        let mut service = TestService::with_tasks(vec![alpha.clone()]);
        service.worklog_pages = vec![
            Ok(page(vec![completed.clone()], None)),
            Ok(page(vec![active.clone()], None)),
        ];
        service.authoritative_worklogs = vec![completed];
        service.deletion_error = Some(ApplicationError::WorklogDeletionWrite {
            write: RepositoryError::WorklogIsActive { id: active.id() },
        });
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);
        app.handle(Command::OpenDeletion);
        app.handle(Command::Confirm);

        assert_eq!(app.mode(), &Mode::Normal);
        assert_eq!(app.history().unwrap().worklogs, vec![active]);
        assert_eq!(text(app.status()), ACTIVE_WORKLOG_DELETE_MESSAGE);
    }

    #[test]
    fn successful_deletion_does_not_reanchor_an_unrelated_timer() {
        let alpha = task(1, "alpha");
        let target = history_worklog(10, alpha.id, 100);
        let active = Worklog::begin(worklog_id(11), alpha.id, at(300));
        let mut service = TestService::with_tasks(vec![alpha.clone()]);
        service.tracking = TrackingState::Running {
            worklog: ActiveWorklog::begin(active.id(), alpha.id, active.start()),
        };
        service.worklog_pages = vec![Ok(page_with_active(
            vec![active.clone(), target.clone()],
            Some(active.clone()),
            None,
        ))];
        service.authoritative_worklogs = vec![active.clone(), target];
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);
        app.handle(Command::MoveDown);
        app.freeze_elapsed_for_tests(Duration::from_secs(600));
        let before = app.elapsed().unwrap();
        app.handle(Command::OpenDeletion);
        app.handle(Command::Confirm);
        assert!(app.elapsed().unwrap() >= before);
    }

    #[test]
    fn enter_opens_the_selected_tasks_history_and_escape_returns_to_it() {
        let alpha = task(1, "alpha");
        let first = page(
            vec![
                history_worklog(11, alpha.id, 200),
                history_worklog(10, alpha.id, 100),
            ],
            Some(cursor(100, 10)),
        );
        let second = page(vec![history_worklog(12, alpha.id, 300)], None);
        let mut service = TestService::with_tasks(vec![alpha.clone(), task(2, "beta")]);
        service.worklog_pages = vec![Ok(first.clone()), Ok(second.clone())];
        let mut app = App::load(service);

        app.handle(Command::OpenHistory);

        assert_eq!(app.screen(), Screen::WorklogHistory);
        let history = app.history().expect("the history is open");
        assert_eq!(history.task_id, alpha.id);
        assert_eq!(history.worklogs, first.worklogs);
        assert_eq!(history.next_cursor, first.next_cursor);
        assert_eq!(
            app.history_selected_index(),
            Some(0),
            "the newest row leads"
        );
        assert_eq!(
            app.status(),
            &Status::Info("History of \"alpha\"".to_owned())
        );

        // The task-list selection was never touched, so Escape returns to
        // the same task, and reopening loads the newest page afresh.
        app.handle(Command::MoveDown);
        app.handle(Command::BackToTaskList);
        assert_eq!(app.screen(), Screen::TaskList);
        assert_eq!(app.history(), None);
        assert_eq!(app.selected(), Some(0));
        assert_eq!(app.tasks()[0].id, alpha.id);

        app.handle(Command::OpenHistory);
        assert_eq!(app.history().unwrap().worklogs, second.worklogs);
    }

    #[test]
    fn history_uses_the_session_timezone_snapshot() {
        let mut app = App::load(TestService::with_tasks(vec![task(1, "alpha")]));
        app.set_timezone_for_tests(chrono_tz::Europe::London);
        for seconds in [0, 1_700_000_000] {
            assert_eq!(
                app.local_time(at(seconds)),
                crate::ui::local_time(at(seconds), &chrono_tz::Europe::London)
            );
        }
        app.freeze_offset_for_tests(FixedOffset::east_opt(2 * 3600).unwrap());
        assert_eq!(app.local_time(at(0)), "1970-01-01 02:00");
    }

    #[test]
    fn timezone_resolution_accepts_common_tz_forms_and_reports_the_utc_fallback() {
        for (value, expected) in [
            ("UTC", chrono_tz::UTC),
            (":UTC", chrono_tz::UTC),
            (
                "/usr/share/zoneinfo/Europe/London",
                chrono_tz::Europe::London,
            ),
            (
                "../usr/share/zoneinfo/posix/Europe/London",
                chrono_tz::Europe::London,
            ),
            (":/etc/zoneinfo/Europe/London", chrono_tz::Europe::London),
            ("../etc/zoneinfo/Europe/London", chrono_tz::Europe::London),
            (
                "/usr/share/zoneinfo/right/Europe/London",
                chrono_tz::Europe::London,
            ),
        ] {
            assert_eq!(parse_timezone_name(value), Some(expected));
        }
        assert_eq!(
            resolve_timezone(
                Some(":/etc/zoneinfo/America/Phoenix"),
                Some("America/Denver")
            ),
            (chrono_tz::America::Phoenix, None),
            "a valid environment override wins over the OS timezone"
        );
        assert_eq!(
            resolve_timezone(Some("not a timezone"), Some("Europe/Paris")),
            (chrono_tz::Europe::Paris, None)
        );
        assert_eq!(
            resolve_timezone(Some("not a timezone"), Some("also invalid")),
            (
                chrono_tz::UTC,
                Some("Could not detect an IANA timezone; using UTC for this session.")
            )
        );
    }

    #[test]
    fn enter_opens_the_history_from_the_archived_view() {
        let gone = archived_task(3, "gone");
        let mut service = TestService::with_tasks(vec![task(1, "alpha"), gone.clone()]);
        service.worklog_pages = vec![Ok(page(vec![history_worklog(5, gone.id, 100)], None))];
        let mut app = App::load(service);
        app.handle(Command::ShowArchivedTasks);

        app.handle(Command::OpenHistory);

        assert_eq!(app.screen(), Screen::WorklogHistory);
        assert_eq!(app.history().unwrap().task_id, gone.id);

        app.handle(Command::BackToTaskList);
        assert_eq!(app.view(), TaskView::Archived);
        assert_eq!(app.selected(), Some(0), "the archived selection returned");
    }

    #[test]
    fn enter_on_an_empty_task_list_is_a_no_op() {
        let mut app = App::load(TestService::with_tasks(vec![]));

        app.handle(Command::OpenHistory);

        assert_eq!(app.screen(), Screen::TaskList);
        assert_eq!(app.history(), None);
        assert_eq!(app.status(), &Status::Info("Ready".to_owned()));
        assert_eq!(
            app.application.worklog_reads.get(),
            0,
            "no task was selected, so nothing was read"
        );
    }

    #[test]
    fn a_failed_history_load_keeps_the_task_list_and_reports_the_error() {
        let mut service = TestService::with_tasks(vec![task(1, "alpha")]);
        service.worklog_pages = vec![Err(TestService::failure())];
        let mut app = App::load(service);

        app.handle(Command::OpenHistory);

        assert_eq!(app.screen(), Screen::TaskList);
        assert_eq!(app.history(), None);
        assert_eq!(app.status(), &Status::Error("Storage error".to_owned()));
    }

    #[test]
    fn history_movement_clamps_without_wrapping() {
        let alpha = task(1, "alpha");
        let mut service = TestService::with_tasks(vec![alpha.clone()]);
        service.worklog_pages = vec![Ok(page(
            vec![
                history_worklog(13, alpha.id, 300),
                history_worklog(12, alpha.id, 200),
                history_worklog(11, alpha.id, 100),
            ],
            None,
        ))];
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);

        app.handle(Command::MoveUp);
        assert_eq!(app.history_selected_index(), Some(0), "no wrap to the end");
        for _ in 0..5 {
            app.handle(Command::MoveDown);
        }
        assert_eq!(
            app.history_selected_index(),
            Some(2),
            "no wrap to the start"
        );
        app.handle(Command::MoveUp);
        assert_eq!(app.history_selected_index(), Some(1));
    }

    #[test]
    fn an_empty_history_has_no_selection_and_movement_does_nothing() {
        let mut service = TestService::with_tasks(vec![task(1, "alpha")]);
        service.worklog_pages = vec![Ok(page(Vec::new(), None))];
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);

        assert_eq!(app.history_selected_index(), None);
        app.handle(Command::MoveDown);
        app.handle(Command::MoveUp);
        assert_eq!(app.history_selected_index(), None);
    }

    #[test]
    fn loading_older_appends_the_page_and_keeps_the_selection() {
        let alpha = task(1, "alpha");
        let first = page(
            vec![
                history_worklog(20, alpha.id, 200),
                history_worklog(19, alpha.id, 100),
            ],
            Some(cursor(100, 19)),
        );
        let older = page(
            vec![
                history_worklog(18, alpha.id, 50),
                history_worklog(17, alpha.id, 40),
            ],
            None,
        );
        let mut service = TestService::with_tasks(vec![alpha.clone()]);
        service.worklog_pages = vec![Ok(first), Ok(older)];
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);
        app.handle(Command::MoveDown);
        assert_eq!(app.history_selected_index(), Some(1));

        app.handle(Command::LoadOlderWorklogs);

        let history = app.history().expect("the history is still open");
        assert_eq!(history.worklogs.len(), 4, "the older page was appended");
        assert_eq!(
            history
                .worklogs
                .iter()
                .map(|worklog| worklog.id())
                .collect::<Vec<_>>(),
            vec![
                worklog_id(20),
                worklog_id(19),
                worklog_id(18),
                worklog_id(17)
            ],
            "older worklogs follow the loaded ones"
        );
        assert_eq!(history.next_cursor, None);
        assert_eq!(app.history_selected_index(), Some(1), "the row stayed put");
        assert_eq!(
            app.status(),
            &Status::Info("Loaded 2 older worklogs".to_owned())
        );
    }

    #[test]
    fn loading_older_reloads_newest_when_the_loaded_active_worklog_stopped() {
        let alpha = task(1, "alpha");
        let active = Worklog::begin(worklog_id(20), alpha.id, at(200));
        let first = page(vec![active], Some(cursor(200, 20)));
        let continuation = page_with_active(vec![history_worklog(19, alpha.id, 100)], None, None);
        let newest = page(vec![history_worklog(20, alpha.id, 200)], None);
        let mut service = TestService::with_tasks(vec![alpha.clone()]);
        service.worklog_pages = vec![Ok(first), Ok(continuation), Ok(newest)];
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);

        app.handle(Command::LoadOlderWorklogs);

        assert_eq!(
            app.history()
                .unwrap()
                .worklogs
                .iter()
                .map(Worklog::id)
                .collect::<Vec<_>>(),
            vec![worklog_id(20)],
            "the continuation was discarded after the active row changed"
        );
        assert_eq!(
            app.status(),
            &Status::Info("History changed and was refreshed".to_owned())
        );
        assert_eq!(app.application.worklog_reads.get(), 3);
    }

    #[test]
    fn loading_older_appends_when_the_active_worklog_is_unchanged() {
        let alpha = task(1, "alpha");
        let active = Worklog::begin(worklog_id(20), alpha.id, at(200));
        let first = page(vec![active.clone()], Some(cursor(200, 20)));
        let continuation =
            page_with_active(vec![history_worklog(19, alpha.id, 100)], Some(active), None);
        let mut service = TestService::with_tasks(vec![alpha.clone()]);
        service.worklog_pages = vec![Ok(first), Ok(continuation)];
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);

        app.handle(Command::LoadOlderWorklogs);

        assert_eq!(
            app.history()
                .unwrap()
                .worklogs
                .iter()
                .map(Worklog::id)
                .collect::<Vec<_>>(),
            vec![worklog_id(20), worklog_id(19)]
        );
        assert_eq!(
            app.status(),
            &Status::Info("Loaded 1 older worklogs".to_owned())
        );
        assert_eq!(app.application.worklog_reads.get(), 2);
    }

    #[test]
    fn loading_older_appends_an_unchanged_active_worklog_after_a_full_newest_page() {
        let alpha = task(1, "alpha");
        let active = Worklog::begin(worklog_id(51), alpha.id, at(500));
        let newest = (1..=50)
            .map(|tag| Worklog::new(worklog_id(tag), alpha.id, at(500), Some(at(500))).unwrap())
            .collect();
        let first = page_with_active(newest, Some(active.clone()), Some(cursor(500, 50)));
        let continuation = page_with_active(
            vec![active.clone(), history_worklog(52, alpha.id, 400)],
            Some(active.clone()),
            Some(cursor(400, 52)),
        );
        let older = page_with_active(vec![history_worklog(53, alpha.id, 300)], Some(active), None);
        let mut service = TestService::with_tasks(vec![alpha]);
        service.worklog_pages = vec![Ok(first), Ok(continuation), Ok(older)];
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);

        app.handle(Command::LoadOlderWorklogs);

        let history = app.history().unwrap();
        assert_eq!(history.worklogs.len(), 52);
        assert_eq!(history.worklogs[50].id(), worklog_id(51));
        assert_eq!(history.next_cursor, Some(cursor(400, 52)));

        app.handle(Command::LoadOlderWorklogs);

        let history = app.history().unwrap();
        assert_eq!(history.worklogs.len(), 53);
        assert_eq!(history.worklogs[52].id(), worklog_id(53));
        assert_eq!(history.next_cursor, None);
        assert_eq!(app.application.worklog_reads.get(), 3);
    }

    #[test]
    fn loading_older_reloads_newest_when_the_active_worklog_start_changes() {
        let alpha = task(1, "alpha");
        let first = page(
            vec![Worklog::begin(worklog_id(20), alpha.id, at(200))],
            Some(cursor(200, 20)),
        );
        let active = Worklog::begin(worklog_id(20), alpha.id, at(300));
        let continuation = page_with_active(
            vec![history_worklog(19, alpha.id, 100)],
            Some(active.clone()),
            None,
        );
        let newest = page_with_active(vec![active.clone()], Some(active), None);
        let mut service = TestService::with_tasks(vec![alpha.clone()]);
        service.worklog_pages = vec![Ok(first), Ok(continuation), Ok(newest)];
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);

        app.handle(Command::LoadOlderWorklogs);

        assert_eq!(
            app.history()
                .unwrap()
                .worklogs
                .iter()
                .map(Worklog::id)
                .collect::<Vec<_>>(),
            vec![worklog_id(20)]
        );
        assert_eq!(
            app.status(),
            &Status::Info("History changed and was refreshed".to_owned())
        );
        assert_eq!(app.application.worklog_reads.get(), 3);
    }

    #[test]
    fn loading_older_reloads_newest_when_active_work_starts_for_the_open_task() {
        let alpha = task(1, "alpha");
        let first = page(
            vec![
                history_worklog(20, alpha.id, 200),
                history_worklog(19, alpha.id, 100),
            ],
            Some(cursor(100, 19)),
        );
        let active = Worklog::begin(worklog_id(21), alpha.id, at(300));
        let continuation = page_with_active(
            vec![history_worklog(18, alpha.id, 50)],
            Some(active.clone()),
            None,
        );
        let newest = page_with_active(
            vec![active, history_worklog(20, alpha.id, 200)],
            Some(Worklog::begin(worklog_id(21), alpha.id, at(300))),
            Some(cursor(200, 20)),
        );
        let mut service = TestService::with_tasks(vec![alpha.clone()]);
        service.worklog_pages = vec![Ok(first), Ok(continuation), Ok(newest)];
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);

        app.handle(Command::LoadOlderWorklogs);

        assert_eq!(
            app.history()
                .unwrap()
                .worklogs
                .iter()
                .map(Worklog::id)
                .collect::<Vec<_>>(),
            vec![worklog_id(21), worklog_id(20)]
        );
        assert_eq!(
            app.status(),
            &Status::Info("History changed and was refreshed".to_owned())
        );
        assert_eq!(app.application.worklog_reads.get(), 3);
    }

    #[test]
    fn loading_older_ignores_active_work_for_another_task() {
        let alpha = task(1, "alpha");
        let beta = task(2, "beta");
        let first = page(
            vec![
                history_worklog(20, alpha.id, 200),
                history_worklog(19, alpha.id, 100),
            ],
            Some(cursor(100, 19)),
        );
        let continuation = page_with_active(
            vec![history_worklog(18, alpha.id, 50)],
            Some(Worklog::begin(worklog_id(21), beta.id, at(300))),
            None,
        );
        let mut service = TestService::with_tasks(vec![alpha.clone(), beta]);
        service.worklog_pages = vec![Ok(first), Ok(continuation)];
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);

        app.handle(Command::LoadOlderWorklogs);

        assert_eq!(
            app.history()
                .unwrap()
                .worklogs
                .iter()
                .map(Worklog::id)
                .collect::<Vec<_>>(),
            vec![worklog_id(20), worklog_id(19), worklog_id(18)]
        );
        assert_eq!(
            app.status(),
            &Status::Info("Loaded 1 older worklogs".to_owned())
        );
        assert_eq!(app.application.worklog_reads.get(), 2);
    }

    #[test]
    fn a_failed_active_row_reload_marks_history_unavailable() {
        let alpha = task(1, "alpha");
        let active = Worklog::begin(worklog_id(20), alpha.id, at(200));
        let first = page(vec![active], Some(cursor(200, 20)));
        let continuation = page_with_active(vec![history_worklog(19, alpha.id, 100)], None, None);
        let mut service = TestService::with_tasks(vec![alpha.clone()]);
        service.worklog_pages = vec![Ok(first), Ok(continuation), Err(TestService::failure())];
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);

        app.handle(Command::LoadOlderWorklogs);

        let history = app.history().unwrap();
        assert_eq!(history.availability, HistoryAvailability::Unavailable);
        assert!(history.worklogs.is_empty());
        assert_eq!(app.application.worklog_reads.get(), 3);
    }

    #[test]
    fn loading_older_at_the_end_of_the_history_changes_nothing() {
        let alpha = task(1, "alpha");
        let mut service = TestService::with_tasks(vec![alpha.clone()]);
        service.worklog_pages = vec![Ok(page(vec![history_worklog(20, alpha.id, 200)], None))];
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);

        app.handle(Command::LoadOlderWorklogs);

        assert_eq!(app.history().unwrap().worklogs.len(), 1);
        assert_eq!(app.history_selected_index(), Some(0));
        assert_eq!(app.status(), &Status::Info("No older worklogs".to_owned()));
        assert_eq!(
            app.application.worklog_reads.get(),
            1,
            "the end of the history is not read again"
        );
    }

    #[test]
    fn a_failed_load_older_preserves_the_displayed_history() {
        let alpha = task(1, "alpha");
        let first = page(
            vec![
                history_worklog(20, alpha.id, 200),
                history_worklog(19, alpha.id, 100),
            ],
            Some(cursor(100, 19)),
        );
        let mut service = TestService::with_tasks(vec![alpha.clone()]);
        service.worklog_pages = vec![Ok(first), Err(TestService::failure())];
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);
        app.handle(Command::MoveDown);

        app.handle(Command::LoadOlderWorklogs);

        let history = app.history().expect("the history is still open");
        assert_eq!(history.worklogs.len(), 2, "no row was lost");
        assert_eq!(
            history.next_cursor,
            Some(cursor(100, 19)),
            "the cursor state survived"
        );
        assert_eq!(app.history_selected_index(), Some(1), "the row stayed put");
        assert_eq!(app.status(), &Status::Error("Storage error".to_owned()));
    }

    #[test]
    fn history_change_while_loading_older_resets_to_the_newest_page() {
        let alpha = task(1, "alpha");
        let first = page(
            vec![
                history_worklog(20, alpha.id, 200),
                history_worklog(19, alpha.id, 100),
            ],
            Some(cursor(100, 19)),
        );
        let newest = page(
            vec![
                history_worklog(21, alpha.id, 300),
                history_worklog(19, alpha.id, 90),
            ],
            Some(cursor(90, 19)),
        );
        let mut service = TestService::with_tasks(vec![alpha.clone()]);
        service.worklog_pages = vec![
            Ok(first),
            Err(ApplicationError::Repository(
                RepositoryError::WorklogHistoryChanged { task_id: alpha.id },
            )),
            Ok(newest),
        ];
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);
        app.handle(Command::MoveDown);

        app.handle(Command::LoadOlderWorklogs);

        let history = app.history().unwrap();
        assert_eq!(
            history.worklogs.iter().map(Worklog::id).collect::<Vec<_>>(),
            vec![worklog_id(21), worklog_id(19)]
        );
        assert_eq!(history.next_cursor, Some(cursor(90, 19)));
        assert_eq!(app.history_selected_index(), Some(1));
        assert_eq!(
            app.status(),
            &Status::Info("History changed and was refreshed".to_owned())
        );
        assert_eq!(app.application.worklog_reads.get(), 3);
    }

    #[test]
    fn failed_reset_after_history_change_marks_history_unavailable() {
        let alpha = task(1, "alpha");
        let first = page(
            vec![
                history_worklog(20, alpha.id, 200),
                history_worklog(19, alpha.id, 100),
            ],
            Some(cursor(100, 19)),
        );
        let mut service = TestService::with_tasks(vec![alpha.clone()]);
        service.worklog_pages = vec![
            Ok(first),
            Err(ApplicationError::Repository(
                RepositoryError::WorklogHistoryChanged { task_id: alpha.id },
            )),
            Err(TestService::failure()),
        ];
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);
        app.handle(Command::MoveDown);

        app.handle(Command::LoadOlderWorklogs);

        let history = app.history().unwrap();
        assert_eq!(history.availability, HistoryAvailability::Unavailable);
        assert!(history.worklogs.is_empty());
        assert_eq!(history.next_cursor, None);
        assert_eq!(history.selected, Some(worklog_id(19)));
        assert_eq!(app.history_selected_index(), None);
        assert_eq!(
            app.status(),
            &Status::Error("History changed, but refresh failed: Storage error".to_owned())
        );
    }

    #[test]
    fn refresh_reloads_the_newest_page_and_keeps_the_selection_by_id() {
        let alpha = task(1, "alpha");
        let first = page(
            vec![
                history_worklog(20, alpha.id, 200),
                history_worklog(19, alpha.id, 100),
            ],
            Some(cursor(100, 19)),
        );
        let newest = page(
            vec![
                history_worklog(21, alpha.id, 300),
                history_worklog(19, alpha.id, 100),
            ],
            None,
        );
        let mut service = TestService::with_tasks(vec![alpha.clone()]);
        service.worklog_pages = vec![Ok(first), Ok(newest)];
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);
        app.handle(Command::MoveDown);

        app.handle(Command::RefreshWorklogs);

        let history = app.history().expect("the history is still open");
        assert_eq!(
            history
                .worklogs
                .iter()
                .map(|worklog| worklog.id())
                .collect::<Vec<_>>(),
            vec![worklog_id(21), worklog_id(19)],
            "the newest page replaced the loaded one"
        );
        assert_eq!(history.next_cursor, None);
        assert_eq!(
            app.history_selected_index(),
            Some(1),
            "the selection followed its worklog id"
        );
        assert_eq!(app.status(), &Status::Info("Refreshed".to_owned()));
    }

    #[test]
    fn refresh_falls_back_to_the_newest_row_when_the_selection_left_the_page() {
        let alpha = task(1, "alpha");
        let first = page(
            vec![
                history_worklog(20, alpha.id, 300),
                history_worklog(19, alpha.id, 200),
                history_worklog(18, alpha.id, 100),
            ],
            Some(cursor(100, 18)),
        );
        let older = page(vec![history_worklog(17, alpha.id, 50)], None);
        // The reload keeps only the newest page; the selected worklog of
        // the longer loaded range is not on it.
        let newest = page(vec![history_worklog(20, alpha.id, 300)], None);
        let mut service = TestService::with_tasks(vec![alpha.clone()]);
        service.worklog_pages = vec![Ok(first), Ok(older), Ok(newest)];
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);
        app.handle(Command::LoadOlderWorklogs);
        for _ in 0..3 {
            app.handle(Command::MoveDown);
        }
        assert_eq!(app.history_selected_index(), Some(3));

        app.handle(Command::RefreshWorklogs);

        assert_eq!(app.history().unwrap().worklogs.len(), 1);
        assert_eq!(app.history_selected_index(), Some(0));
    }

    #[test]
    fn a_failed_refresh_preserves_the_displayed_history() {
        let alpha = task(1, "alpha");
        let first = page(
            vec![history_worklog(20, alpha.id, 200)],
            Some(cursor(200, 20)),
        );
        let mut service = TestService::with_tasks(vec![alpha.clone()]);
        service.worklog_pages = vec![Ok(first), Err(TestService::failure())];
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);

        app.handle(Command::RefreshWorklogs);

        let history = app.history().expect("the history is still open");
        assert_eq!(
            history
                .worklogs
                .iter()
                .map(|worklog| worklog.id())
                .collect::<Vec<_>>(),
            vec![worklog_id(20)],
            "no row was lost"
        );
        assert_eq!(history.next_cursor, Some(cursor(200, 20)));
        assert_eq!(app.history_selected_index(), Some(0));
        assert_eq!(app.status(), &Status::Error("Storage error".to_owned()));
    }

    #[test]
    fn task_list_commands_do_not_act_on_the_history_screen() {
        let alpha = task(1, "alpha");
        let first = page(
            vec![
                history_worklog(20, alpha.id, 200),
                history_worklog(19, alpha.id, 100),
            ],
            None,
        );
        let mut service = TestService::with_tasks(vec![alpha.clone(), task(2, "beta")]);
        service.worklog_pages = vec![Ok(first)];
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);
        let before = app.history().expect("the history is open").clone();
        let tasks_before = app.tasks().to_vec();

        app.handle(Command::ToggleTracking);
        app.handle(Command::CycleOrdering);
        app.handle(Command::ShowArchivedTasks);
        app.handle(Command::ShowActiveTasks);
        app.handle(Command::OpenAdd);
        app.handle(Command::OpenRename);
        app.handle(Command::OpenArchiveConfirm);
        app.handle(Command::UnarchiveSelected);
        app.handle(Command::OpenHistory);
        app.handle(Command::Insert('x'));
        app.handle(Command::Backspace);
        app.handle(Command::Confirm);
        app.handle(Command::Cancel);

        assert_eq!(app.screen(), Screen::WorklogHistory);
        assert_eq!(app.mode(), &Mode::Normal);
        assert_eq!(app.history().unwrap(), &before);
        assert_eq!(app.view(), TaskView::Active);
        assert_eq!(app.ordering(), TaskOrdering::RecentlyWorked);
        assert_eq!(app.tasks(), tasks_before.as_slice());
        assert_eq!(app.active_task_id(), None, "no tracking was started");
        assert_eq!(
            app.application.worklog_reads.get(),
            1,
            "the open history was not read again"
        );
    }

    #[test]
    fn history_commands_do_not_act_on_the_task_list() {
        let mut app = App::load(TestService::with_tasks(vec![task(1, "alpha")]));

        app.handle(Command::LoadOlderWorklogs);
        app.handle(Command::RefreshWorklogs);
        app.handle(Command::BackToTaskList);

        assert_eq!(app.screen(), Screen::TaskList);
        assert_eq!(app.history(), None);
        assert_eq!(app.status(), &Status::Info("Ready".to_owned()));
        assert_eq!(
            app.application.worklog_reads.get(),
            0,
            "no history is open, so nothing was read"
        );
    }

    #[test]
    fn opening_a_history_requires_normal_mode() {
        let mut app = App::load(TestService::with_tasks(vec![task(1, "alpha")]));
        app.handle(Command::OpenAdd);
        app.handle(Command::OpenHistory);
        assert!(matches!(app.mode(), Mode::Input { .. }));
        assert_eq!(app.screen(), Screen::TaskList);
        assert_eq!(app.application.worklog_reads.get(), 0);

        app.handle(Command::Cancel);
        app.mode = Mode::ConfirmArchive {
            task_id: TaskId::from_uuid(uuid::Uuid::from_u128(1)),
            name: "alpha".to_owned(),
        };
        app.handle(Command::OpenHistory);
        assert!(matches!(app.mode(), Mode::ConfirmArchive { .. }));
        assert_eq!(app.screen(), Screen::TaskList);
        assert_eq!(app.application.worklog_reads.get(), 0);
    }

    #[test]
    fn quitting_from_the_history_leaves_tracking_active() {
        let task = task(1, "alpha");
        let mut service = TestService::with_tasks(vec![task.clone()]);
        service.tracking = TrackingState::Running {
            worklog: ActiveWorklog::begin(worklog_id(10), task.id, at(100)),
        };
        service.worklog_pages = vec![Ok(page(
            vec![Worklog::begin(worklog_id(10), task.id, at(100))],
            None,
        ))];
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);

        app.handle(Command::Quit);

        assert!(!app.is_running());
        assert_eq!(app.active_task_id(), Some(task.id));
    }

    #[test]
    fn opening_history_adopts_an_active_worklog_that_another_client_started() {
        let task = task(1, "alpha");
        let active = Worklog::begin(worklog_id(10), task.id, at(100));
        let mut service = TestService::with_tasks(vec![task.clone()]);
        service.worklog_pages = vec![Ok(page(vec![active.clone()], None))];
        let mut app = App::load(service);
        assert_eq!(app.active_task_id(), None);

        app.handle(Command::OpenHistory);

        assert_eq!(app.active_task_id(), Some(task.id));
        assert_eq!(app.active_worklog_id(), Some(active.id()));
        app.freeze_elapsed_for_tests(Duration::from_secs(125));
        assert_eq!(
            app.history_row_duration(&active).as_secs(),
            app.elapsed()
                .expect("the header adopted a monotonic clock")
                .as_secs()
        );
    }

    #[test]
    fn history_row_durations_use_the_monotonic_clock_for_the_running_worklog() {
        let task = task(1, "alpha");
        let active = Worklog::begin(worklog_id(10), task.id, at(100));
        let mut service = TestService::with_tasks(vec![task.clone()]);
        service.tracking = TrackingState::Running {
            worklog: ActiveWorklog::begin(active.id(), task.id, at(100)),
        };
        service.worklog_pages = vec![Ok(page(
            vec![active, history_worklog(11, task.id, 200)],
            None,
        ))];
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);
        app.freeze_elapsed_for_tests(Duration::from_secs(125));

        let rows = app.history().expect("the history is open").worklogs.clone();
        let running = app.history_row_duration(&rows[0]);
        assert!(
            running >= Duration::from_secs(125) && running < Duration::from_secs(126),
            "the running row shares the header's clock, got {running:?}"
        );
        assert!(
            app.elapsed().unwrap() >= Duration::from_secs(125),
            "the header reads the same clock"
        );
        assert_eq!(
            app.history_row_duration(&rows[1]),
            Duration::from_secs(60),
            "a stopped row derives its duration from its stored times"
        );
        let unmatched = Worklog::begin(worklog_id(12), task.id, at(300));
        assert_eq!(
            app.history_row_duration(&unmatched),
            Duration::ZERO,
            "an inconsistent running row never falls back to wall time"
        );
    }

    fn correction_history_app(initial: Worklog, reload: Vec<Worklog>) -> App<TestService> {
        let task = task(1, "alpha");
        let mut service = TestService::with_tasks(vec![task]);
        service.worklog_pages = vec![
            Ok(page(vec![initial], Some(cursor(50, 50)))),
            Ok(page(reload, None)),
        ];
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);
        app
    }

    fn correction_app(initial: Worklog, reload: Vec<Worklog>) -> App<TestService> {
        let mut app = correction_history_app(initial, reload);
        app.freeze_offset_for_tests(FixedOffset::east_opt(2 * 3600).unwrap());
        app.handle(Command::OpenCorrection);
        app
    }

    fn correction_app_in<Tz>(
        initial: Worklog,
        reload: Vec<Worklog>,
        timezone: &Tz,
    ) -> App<TestService>
    where
        Tz: TimeZone,
        Tz::Offset: std::fmt::Display,
    {
        let mut app = correction_history_app(initial, reload);
        app.open_correction_in(timezone);
        app
    }

    #[test]
    fn correction_prefills_local_minutes() {
        let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
        let start = DateTime::from_timestamp(100, 123_456_000).unwrap();
        let end = DateTime::from_timestamp(200, 654_321_000).unwrap();
        let completed = Worklog::new(worklog_id(10), task_id, start, Some(end)).unwrap();
        let app = correction_app(completed.clone(), vec![completed]);
        let draft = app.correction().unwrap();
        assert_eq!(draft.start().text(), "1970-01-01 02:01");
        assert_eq!(draft.end().unwrap().text(), "1970-01-01 02:03");

        let active = Worklog::begin(worklog_id(11), task_id, start);
        let mut app = correction_app(active.clone(), vec![active]);
        assert!(app.correction().unwrap().end().is_none());
        app.handle(Command::SwitchCorrectionField);
        assert_eq!(app.correction().unwrap().focused(), CorrectionField::Start);
    }

    #[test]
    fn timestamp_input_edits_and_moves_at_the_character_cursor() {
        let mut input = TimestampInput::new("123".to_owned());

        input.move_left();
        input.insert('9');
        assert_eq!(input.text(), "1293");
        assert_eq!(input.cursor(), 3);

        input.backspace();
        assert_eq!(input.text(), "123");
        assert_eq!(input.cursor(), 2);

        input.delete();
        assert_eq!(input.text(), "12");
        assert_eq!(input.cursor(), 2);

        input.move_left();
        assert_eq!(input.cursor(), 1);
        input.move_right();
        assert_eq!(input.cursor(), 2);
    }

    #[test]
    fn correction_switches_fields_and_edits_at_a_bounded_character_cursor() {
        let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
        let worklog = history_worklog(10, task_id, 100);
        let mut app = correction_app(worklog.clone(), vec![worklog]);
        app.handle(Command::SwitchCorrectionField);
        assert_eq!(app.correction().unwrap().focused(), CorrectionField::End);
        let original = app.correction().unwrap().end().unwrap().text().to_owned();
        app.handle(Command::MoveCursorLeft);
        app.handle(Command::Backspace);
        app.handle(Command::Insert('9'));
        assert_ne!(app.correction().unwrap().end().unwrap().text(), original);
        app.handle(Command::MoveCursorRight);
        app.handle(Command::Delete);
        app.handle(Command::SwitchCorrectionField);
        assert_eq!(app.correction().unwrap().focused(), CorrectionField::Start);

        let mut input = TimestampInput::new("1🕒".to_owned());
        input.backspace();
        assert_eq!(input.text(), "1");
        input.insert('x');
        assert_eq!(input.text(), "1", "invalid timestamp text is ignored");
        for _ in 0..100 {
            input.insert('2');
        }
        assert_eq!(input.text().chars().count(), TimestampInput::MAX_LEN);
    }

    #[test]
    fn correction_commands_adjust_and_edit_the_focused_timestamp() {
        let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
        let worklog = history_worklog(10, task_id, 100);
        let mut app = correction_app(worklog.clone(), vec![worklog.clone()]);

        app.handle(Command::AdjustForwardOneHour);
        assert_eq!(app.correction().unwrap().start().text(), "1970-01-01 03:01");

        let mut app = correction_app(worklog.clone(), vec![worklog]);
        let original = app.correction().unwrap().start().text().to_owned();
        app.handle(Command::Backspace);
        assert_eq!(
            app.correction().unwrap().start().text(),
            &original[..original.len() - 1]
        );
        app.handle(Command::Insert('1'));
        assert_eq!(app.correction().unwrap().start().text(), original);
        assert_eq!(
            app.correction().unwrap().start().cursor(),
            app.correction().unwrap().start().text().chars().count()
        );
    }

    #[test]
    fn correction_parsing_requires_local_minute_format() {
        let timezone = FixedOffset::east_opt(2 * 3600).unwrap();
        assert_eq!(
            parse_correction_timestamp("1970-01-01 02:00", &timezone, None).unwrap(),
            DateTime::from_timestamp(0, 0).unwrap()
        );
        for invalid in [
            "1970-01-01 00:00+02:00",
            "1970-01-01 00:00:01",
            "1970-01-01 00:00:00",
            "+9999-01-01 00:00",
            "10000-01-01 00:00",
            "not-a-timestamp",
        ] {
            assert_eq!(
                parse_correction_timestamp(invalid, &timezone, None),
                Err("Use YYYY-MM-DD HH:MM")
            );
        }
    }

    #[test]
    fn correction_years_format_and_parse_with_chrono_strict_expanded_form() {
        let timezone = FixedOffset::east_opt(0).unwrap();
        for (year, expected) in [
            (-1, "-0001-01-02 03:04"),
            (0, "0000-01-02 03:04"),
            (9999, "9999-01-02 03:04"),
            (10000, "+10000-01-02 03:04"),
        ] {
            let instant = Utc.with_ymd_and_hms(year, 1, 2, 3, 4, 0).single().unwrap();
            assert_eq!(
                correction_timestamp(instant, &timezone).as_deref(),
                Some(expected)
            );
            assert_eq!(
                parse_correction_timestamp(expected, &timezone, None),
                Ok(instant)
            );
        }
    }

    #[test]
    fn local_to_utc_range_overflow_is_not_reported_as_a_daylight_saving_gap() {
        assert_eq!(
            parse_correction_timestamp(
                "-262143-01-01 00:00",
                &FixedOffset::east_opt(60).unwrap(),
                None,
            ),
            Err(OUTSIDE_EDITABLE_RANGE)
        );
        assert_eq!(
            parse_correction_timestamp(
                "+262142-12-31 23:59",
                &FixedOffset::west_opt(60).unwrap(),
                None,
            ),
            Err(OUTSIDE_EDITABLE_RANGE)
        );
    }

    #[test]
    fn timestamp_input_accepts_every_supported_year_shape_up_to_chrono_limit() {
        for timestamp in [
            "-0001-01-02 03:04",
            "0000-01-02 03:04",
            "9999-01-02 03:04",
            "+10000-01-02 03:04",
        ] {
            let mut input = TimestampInput::new(String::new());
            for character in timestamp.chars() {
                input.insert(character);
            }
            assert_eq!(input.text(), timestamp);
        }

        for longest in ["-262143-01-01 00:00", "+262142-12-31 23:59"] {
            assert_eq!(longest.chars().count(), TimestampInput::MAX_LEN);
            let mut input = TimestampInput::new(String::new());
            for character in longest.chars() {
                input.insert(character);
            }
            input.insert('0');
            assert_eq!(input.text(), longest);
        }
    }

    #[derive(Clone, Copy, Debug)]
    struct CorrectionTestZone;

    impl CorrectionTestZone {
        fn offset(seconds: i64) -> FixedOffset {
            let offset = if seconds < 3_600 {
                3_600
            } else if seconds < 18_000 {
                7_200
            } else if seconds < 100_000 {
                3_600
            } else {
                10_800
            };
            FixedOffset::east_opt(offset).unwrap()
        }
    }

    impl TimeZone for CorrectionTestZone {
        type Offset = FixedOffset;

        fn from_offset(_offset: &Self::Offset) -> Self {
            Self
        }

        fn offset_from_local_date(&self, local: &NaiveDate) -> MappedLocalTime<Self::Offset> {
            self.offset_from_local_datetime(&local.and_hms_opt(0, 0, 0).unwrap())
        }

        fn offset_from_local_datetime(
            &self,
            local: &NaiveDateTime,
        ) -> MappedLocalTime<Self::Offset> {
            let seconds = local.and_utc().timestamp();
            if seconds < 7_200 {
                MappedLocalTime::Single(FixedOffset::east_opt(3_600).unwrap())
            } else if seconds < 10_800 {
                MappedLocalTime::None
            } else if seconds < 21_600 {
                MappedLocalTime::Single(FixedOffset::east_opt(7_200).unwrap())
            } else if seconds < 25_200 {
                MappedLocalTime::Ambiguous(
                    FixedOffset::east_opt(7_200).unwrap(),
                    FixedOffset::east_opt(3_600).unwrap(),
                )
            } else if seconds < 103_600 {
                MappedLocalTime::Single(FixedOffset::east_opt(3_600).unwrap())
            } else if seconds < 110_800 {
                MappedLocalTime::None
            } else {
                MappedLocalTime::Single(FixedOffset::east_opt(10_800).unwrap())
            }
        }

        fn offset_from_utc_date(&self, utc: &NaiveDate) -> Self::Offset {
            Self::offset(utc.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp())
        }

        fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> Self::Offset {
            Self::offset(utc.and_utc().timestamp())
        }
    }

    #[derive(Clone, Copy, Debug)]
    struct SubminuteTransitionZone;

    impl TimeZone for SubminuteTransitionZone {
        type Offset = FixedOffset;

        fn from_offset(_offset: &Self::Offset) -> Self {
            Self
        }

        fn offset_from_local_date(&self, local: &NaiveDate) -> MappedLocalTime<Self::Offset> {
            self.offset_from_local_datetime(&local.and_hms_opt(0, 0, 0).unwrap())
        }

        fn offset_from_local_datetime(
            &self,
            local: &NaiveDateTime,
        ) -> MappedLocalTime<Self::Offset> {
            let seconds = local.and_utc().timestamp();
            if seconds < 330 {
                MappedLocalTime::Single(FixedOffset::east_opt(30).unwrap())
            } else if seconds < 360 {
                MappedLocalTime::None
            } else {
                MappedLocalTime::Single(FixedOffset::east_opt(60).unwrap())
            }
        }

        fn offset_from_utc_date(&self, utc: &NaiveDate) -> Self::Offset {
            self.offset_from_utc_datetime(&utc.and_hms_opt(0, 0, 0).unwrap())
        }

        fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> Self::Offset {
            if utc.and_utc().timestamp() < 300 {
                FixedOffset::east_opt(30).unwrap()
            } else {
                FixedOffset::east_opt(60).unwrap()
            }
        }
    }

    #[test]
    fn changed_ambiguous_input_uses_the_original_offset_or_is_rejected() {
        let zone = CorrectionTestZone;
        let mut input = TimestampInput::new("1970-01-01 00:00".to_owned());
        input.replace("1970-01-01 06:30".to_owned());

        assert_eq!(
            resolve_correction_timestamp(&input, &zone, at(10_000)).unwrap(),
            at(16_200)
        );
        assert_eq!(
            resolve_correction_timestamp(&input, &zone, at(20_000)).unwrap(),
            at(19_800)
        );
        assert_eq!(
            resolve_correction_timestamp(&input, &zone, at(100_000)),
            Err("Ambiguous local time")
        );
    }

    #[test]
    fn minute_and_hour_adjustments_reformat_in_the_configured_timezone() {
        let utc = FixedOffset::east_opt(0).unwrap();
        let original = DateTime::from_timestamp(0, 123_456_000).unwrap();
        assert_eq!(
            adjusted_correction_timestamp(
                &TimestampInput::new("1970-01-01 00:00".to_owned()),
                TimeDelta::minutes(5),
                &utc,
                original
            )
            .unwrap(),
            ("1970-01-01 00:05".to_owned(), at(300))
        );
        assert_eq!(
            adjusted_correction_timestamp(
                &TimestampInput::new("1970-01-01 00:00".to_owned()),
                TimeDelta::hours(-1),
                &utc,
                original
            )
            .unwrap(),
            ("1969-12-31 23:00".to_owned(), at(-3_600))
        );
        let zone = CorrectionTestZone;
        let original = DateTime::from_timestamp(3300, 0).unwrap();
        assert_eq!(
            adjusted_correction_timestamp(
                &TimestampInput::new("1970-01-01 01:55".to_owned()),
                TimeDelta::minutes(5),
                &zone,
                original
            )
            .unwrap(),
            ("1970-01-01 03:00".to_owned(), at(3_600))
        );
    }

    #[test]
    fn subminute_offset_transition_rejects_an_unrepresentable_absolute_adjustment() {
        assert_eq!(
            parse_correction_timestamp("1970-01-01 00:01", &SubminuteTransitionZone, None,),
            Ok(at(30)),
            "local wall-clock second 00 can map to nonzero UTC seconds",
        );

        let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
        let worklog = Worklog::begin(worklog_id(10), task_id, at(30));
        let mut app = correction_app_in(worklog.clone(), vec![worklog], &SubminuteTransitionZone);
        let before = app.correction().unwrap().clone();

        app.adjust_correction_in(TimeDelta::minutes(5), &SubminuteTransitionZone);

        assert_eq!(app.correction().unwrap(), &before);
        assert_eq!(
            text(app.status()),
            "Adjustment cannot be represented as a local minute"
        );
        assert!(app.application.correction_calls.is_empty());
    }

    #[test]
    fn fallback_adjustment_keeps_the_resolved_occurrence_when_the_text_repeats() {
        let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
        let original_start = DateTime::from_timestamp(16_230, 123_456_000).unwrap();
        let worklog = Worklog::begin(worklog_id(10), task_id, original_start);
        let mut app = correction_app_in(worklog.clone(), vec![worklog], &CorrectionTestZone);

        app.adjust_correction_in(TimeDelta::hours(1), &CorrectionTestZone);

        assert_eq!(app.correction().unwrap().start().text(), "1970-01-01 06:30");
        app.confirm_correction_in(&CorrectionTestZone);
        let replacement = app.application.correction_calls[0].2;
        assert_eq!(replacement.start(), at(19_800));
        assert_eq!(replacement.end(), None);
    }

    #[test]
    fn changed_gap_input_is_rejected_without_calling_the_application() {
        let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
        let worklog = Worklog::begin(worklog_id(10), task_id, at(0));
        let mut app = correction_app_in(worklog.clone(), vec![worklog], &CorrectionTestZone);
        app.correction_mut_for_tests()
            .start
            .replace("1970-01-01 02:30".to_owned());

        app.confirm_correction_in(&CorrectionTestZone);

        assert_eq!(text(app.status()), "Start: Local time does not exist");
        assert!(app.application.correction_calls.is_empty());
        assert!(app.correction().is_some());
    }

    #[test]
    fn an_unparseable_adjustment_keeps_the_draft() {
        let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
        let worklog = history_worklog(10, task_id, 100);
        let mut app = correction_app(worklog.clone(), vec![worklog]);
        app.correction_mut_for_tests()
            .start
            .replace("bad".to_owned());
        let before = app.correction().unwrap().clone();
        app.handle(Command::AdjustForwardFiveMinutes);
        assert_eq!(app.correction().unwrap(), &before);
        assert_eq!(text(app.status()), "Use YYYY-MM-DD HH:MM");
    }

    #[test]
    fn escape_cancels_correction_without_writing() {
        let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
        let worklog = history_worklog(10, task_id, 100);
        let mut app = correction_app(worklog.clone(), vec![worklog]);
        app.handle(Command::Insert('2'));
        app.handle(Command::Cancel);
        assert_eq!(app.mode(), &Mode::Normal);
        assert_eq!(
            app.status(),
            &Status::Info("Correction cancelled".to_owned())
        );
        assert!(app.application.correction_calls.is_empty());
    }

    #[test]
    fn correction_failure_reports_write_and_recovery_causes() {
        let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
        let worklog = history_worklog(10, task_id, 100);
        let mut app = correction_app(worklog.clone(), vec![worklog]);
        app.application.correction_error = Some(ApplicationError::WorklogCorrectionRecovery {
            write: RepositoryError::WorklogChanged { id: worklog_id(10) },
            recovery: RepositoryError::Backend {
                message: "reload failed".to_owned(),
            },
        });

        app.handle(Command::Confirm);

        assert_eq!(
            text(app.status()),
            "Worklog changed. State recovery also failed: Storage error. Cancel and press r to refresh."
        );
        assert!(app.correction().is_some());
    }

    #[test]
    fn repository_error_presentation_is_stable_for_every_variant() {
        let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
        let worklog_id = worklog_id(2);
        let presented = [
            (
                RepositoryError::TaskNotFound { id: task_id },
                "Task not found",
            ),
            (
                RepositoryError::WorklogNotFound { id: worklog_id },
                "Worklog not found",
            ),
            (
                RepositoryError::WorklogAlreadyStopped { id: worklog_id },
                "Worklog is already stopped",
            ),
            (
                RepositoryError::WorklogChanged { id: worklog_id },
                "Worklog changed in another client",
            ),
            (
                RepositoryError::WorklogHistoryChanged { task_id },
                "Worklog history changed. Press r to refresh",
            ),
            (
                RepositoryError::SameTaskWorklogOverlap { id: worklog_id },
                "The worklog overlaps another worklog",
            ),
            (
                RepositoryError::WorklogAlreadyExists { id: worklog_id },
                "Worklog already exists",
            ),
            (
                RepositoryError::TaskAlreadyExists { id: task_id },
                "Task already exists",
            ),
            (
                RepositoryError::ActiveWorklogExists,
                "Another worklog is active",
            ),
            (
                RepositoryError::TaskArchived { id: task_id },
                "Task is archived",
            ),
            (
                RepositoryError::TaskIsActive { id: task_id },
                "Task has active work",
            ),
            (
                RepositoryError::Constraint {
                    message: "private constraint".to_owned(),
                },
                "Storage rejected the change",
            ),
            (
                RepositoryError::CorruptData {
                    field: "private field",
                },
                "Stored data is invalid",
            ),
            (
                RepositoryError::Backend {
                    message: "private backend".to_owned(),
                },
                "Storage error",
            ),
        ];
        for (error, expected) in presented {
            assert_eq!(repository_error_text(&error), expected);
        }
    }

    #[test]
    fn deletion_statuses_use_stable_sanitized_text() {
        let active = ApplicationError::WorklogDeletionWrite {
            write: RepositoryError::WorklogIsActive { id: worklog_id(2) },
        };
        assert_eq!(
            application_error_text(&active),
            ACTIVE_WORKLOG_DELETE_MESSAGE
        );
        let active_recovery = ApplicationError::WorklogDeletionRecovery {
            write: RepositoryError::WorklogIsActive { id: worklog_id(2) },
            recovery: RepositoryError::Backend {
                message: "recovery secret".to_owned(),
            },
        };
        assert_eq!(
            application_error_text(&active_recovery),
            "Deletion failed: Running worklogs cannot be deleted. State recovery failed: Storage error."
        );
        let recovery = ApplicationError::WorklogDeletionRecovery {
            write: RepositoryError::Backend {
                message: "write secret".to_owned(),
            },
            recovery: RepositoryError::Backend {
                message: "recovery secret".to_owned(),
            },
        };
        assert_eq!(
            application_error_text(&recovery),
            "Deletion failed: Storage error. State recovery failed: Storage error."
        );
    }

    #[test]
    fn repository_statuses_hide_backend_secrets_and_corrupt_data_fields() {
        let secret = "postgres://user:secret@host/tracker";
        let backend = RepositoryError::Backend {
            message: secret.to_owned(),
        };
        let corrupt = RepositoryError::CorruptData {
            field: "account token",
        };
        let ordinary = [
            ApplicationError::Repository(backend.clone()),
            ApplicationError::TrackingWrite(backend.clone()),
            ApplicationError::TrackingRecovery(backend.clone()),
            ApplicationError::TaskRecovery(corrupt.clone()),
            ApplicationError::WorklogCorrectionWrite {
                write: backend.clone(),
            },
            ApplicationError::WorklogCorrectionRecovery {
                write: backend.clone(),
                recovery: corrupt,
            },
            ApplicationError::WorklogDeletionWrite {
                write: backend.clone(),
            },
            ApplicationError::WorklogDeletionRecovery {
                write: backend.clone(),
                recovery: RepositoryError::CorruptData {
                    field: "account token",
                },
            },
        ];
        for error in ordinary {
            let status = application_error_text(&error);
            assert!(!status.contains(secret), "{status}");
            assert!(!status.contains("account token"), "{status}");
        }
        let correction = ApplicationError::WorklogCorrectionRecovery {
            write: RepositoryError::WorklogChanged { id: worklog_id(10) },
            recovery: backend,
        };
        let status = correction_error_text(&correction);
        assert!(!status.contains(secret), "{status}");
        assert_eq!(
            status,
            "Worklog changed. State recovery also failed: Storage error. Cancel and press r to refresh."
        );
    }

    #[test]
    fn generic_correction_recovery_reports_both_sanitized_causes() {
        let write_secret = "postgres://writer:secret@host/tracker";
        let recovery_secret = "postgres://reader:secret@host/tracker";
        let error = ApplicationError::WorklogCorrectionRecovery {
            write: RepositoryError::Backend {
                message: write_secret.to_owned(),
            },
            recovery: RepositoryError::Backend {
                message: recovery_secret.to_owned(),
            },
        };

        let status = correction_error_text(&error);

        assert_eq!(
            status,
            "Correction failed: Storage error. State recovery failed: Storage error."
        );
        assert!(!status.contains(write_secret), "{status}");
        assert!(!status.contains(recovery_secret), "{status}");
    }

    #[test]
    fn overlap_recovery_failure_keeps_the_overlap_error_text() {
        let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
        let worklog = history_worklog(10, task_id, 100);
        let mut app = correction_app(worklog.clone(), vec![worklog]);
        app.application.correction_error = Some(ApplicationError::WorklogCorrectionRecovery {
            write: RepositoryError::SameTaskWorklogOverlap { id: worklog_id(10) },
            recovery: RepositoryError::Backend {
                message: "reload failed".to_owned(),
            },
        });

        app.handle(Command::Confirm);

        assert_eq!(
            text(app.status()),
            "The corrected time overlaps another worklog. State recovery also failed: Storage error."
        );
    }

    #[test]
    fn quitting_from_correction_does_not_write() {
        let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
        let worklog = history_worklog(10, task_id, 100);
        let mut app = correction_app(worklog.clone(), vec![worklog]);
        app.handle(Command::Quit);
        assert!(!app.is_running());
        assert!(app.application.correction_calls.is_empty());
        assert!(app.correction().is_some());
    }

    #[test]
    fn every_correction_failure_keeps_the_full_draft_open() {
        let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
        let worklog = history_worklog(10, task_id, 100);
        let failures = [
            ApplicationError::InvalidWorklogCorrection(WorklogCorrectionError::EndBeforeStart),
            ApplicationError::WorklogCorrectionWrite {
                write: RepositoryError::SameTaskWorklogOverlap { id: worklog.id() },
            },
            ApplicationError::WorklogCorrectionWrite {
                write: RepositoryError::Backend {
                    message: "write failed".to_owned(),
                },
            },
            ApplicationError::WorklogCorrectionRecovery {
                write: RepositoryError::Backend {
                    message: "write failed".to_owned(),
                },
                recovery: RepositoryError::Backend {
                    message: "reload failed".to_owned(),
                },
            },
            ApplicationError::WorklogCorrectionWrite {
                write: RepositoryError::WorklogChanged { id: worklog.id() },
            },
        ];
        for failure in failures {
            let mut app = correction_app(worklog.clone(), vec![worklog.clone()]);
            app.application.correction_error = Some(failure.clone());
            let before = app.correction().unwrap().clone();
            app.handle(Command::Confirm);
            assert_eq!(
                app.correction().unwrap(),
                &before,
                "draft changed for {failure:?}"
            );
            if matches!(
                failure,
                ApplicationError::WorklogCorrectionWrite {
                    write: RepositoryError::WorklogChanged { .. }
                }
            ) {
                assert_eq!(
                    text(app.status()),
                    "Worklog changed. Cancel and press r to refresh."
                );
            }
        }
    }

    #[test]
    fn parse_failures_keep_both_drafts_and_skip_the_application() {
        let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
        let worklog = history_worklog(10, task_id, 100);
        let mut app = correction_app(worklog.clone(), vec![worklog]);
        app.correction_mut_for_tests()
            .start
            .replace("bad".to_owned());
        let before = app.correction().unwrap().clone();
        app.handle(Command::Confirm);
        assert_eq!(app.correction().unwrap(), &before);
        assert!(app.application.correction_calls.is_empty());
    }

    #[test]
    fn changed_and_unchanged_fields_keep_their_independent_precision() {
        let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
        let start = DateTime::from_timestamp(100, 123_456_000).unwrap();
        let end = DateTime::from_timestamp(200, 654_321_000).unwrap();
        let worklog = Worklog::new(worklog_id(10), task_id, start, Some(end)).unwrap();

        let mut app = correction_app(worklog.clone(), vec![worklog.clone()]);
        app.correction_mut_for_tests()
            .start
            .replace("1970-01-01 02:02".to_owned());
        app.handle(Command::Confirm);
        let replacement = app.application.correction_calls[0].2;
        assert_eq!(replacement.start(), at(120));
        assert_eq!(replacement.end(), Some(end));

        let mut app = correction_app(worklog.clone(), vec![worklog]);
        app.handle(Command::SwitchCorrectionField);
        app.correction_mut_for_tests()
            .end
            .as_mut()
            .unwrap()
            .replace("1970-01-01 02:04".to_owned());
        app.handle(Command::Confirm);
        let replacement = app.application.correction_calls[0].2;
        assert_eq!(replacement.start(), start);
        assert_eq!(replacement.end(), Some(at(240)));
    }

    #[test]
    fn text_edited_back_to_its_opening_value_preserves_the_original_instant() {
        let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
        let start = DateTime::from_timestamp(100, 123_456_000).unwrap();
        let worklog = Worklog::begin(worklog_id(10), task_id, start);
        let mut app = correction_app(worklog.clone(), vec![worklog]);

        app.handle(Command::Backspace);
        app.handle(Command::Insert('1'));
        app.handle(Command::Confirm);

        assert_eq!(app.application.correction_calls[0].2.start(), start);
    }

    #[test]
    fn untouched_fields_preserve_exact_utc_when_the_timezone_is_stable() {
        let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
        let start = DateTime::from_timestamp(100, 123_456_000).unwrap();
        let end = DateTime::from_timestamp(200, 654_321_000).unwrap();
        let worklog = Worklog::new(worklog_id(10), task_id, start, Some(end)).unwrap();
        let mut app = correction_app(worklog.clone(), vec![worklog]);

        app.handle(Command::Confirm);

        assert_eq!(
            app.application.correction_calls[0].2,
            WorklogTimes::new(start, Some(end))
        );
    }

    #[test]
    fn a_timezone_snapshot_keeps_utc_rules_when_london_changes_later() {
        let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
        let original = Utc
            .with_ymd_and_hms(2025, 1, 15, 10, 0, 0)
            .single()
            .unwrap();
        let worklog = Worklog::begin(worklog_id(10), task_id, original);
        let mut app = correction_history_app(worklog.clone(), vec![worklog]);
        app.set_timezone_for_tests(chrono_tz::UTC);
        app.handle(Command::OpenCorrection);
        app.correction_mut_for_tests()
            .start
            .replace("2025-07-01 10:00".to_owned());

        let london = chrono_tz::Europe::London;
        assert_eq!(
            parse_correction_timestamp("2025-07-01 10:00", &london, Some(original)),
            Ok(Utc.with_ymd_and_hms(2025, 7, 1, 9, 0, 0).single().unwrap())
        );

        app.handle(Command::Confirm);

        assert_eq!(
            app.application.correction_calls[0].2.start(),
            Utc.with_ymd_and_hms(2025, 7, 1, 10, 0, 0).single().unwrap()
        );
    }

    #[test]
    fn out_of_range_local_timestamps_cannot_open_correction() {
        let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
        for (timestamp, offset) in [
            (DateTime::<Utc>::MIN_UTC, FixedOffset::west_opt(1).unwrap()),
            (DateTime::<Utc>::MAX_UTC, FixedOffset::east_opt(1).unwrap()),
        ] {
            assert_eq!(correction_timestamp(timestamp, &offset), None);
            let worklog = Worklog::begin(
                worklog_id(offset.local_minus_utc() as u128),
                task_id,
                timestamp,
            );
            let mut app = correction_history_app(worklog, Vec::new());
            app.freeze_offset_for_tests(offset);

            app.handle(Command::OpenCorrection);

            assert_eq!(app.mode(), &Mode::Normal);
            assert_eq!(text(app.status()), OUTSIDE_EDITABLE_RANGE);
            assert!(app.application.correction_calls.is_empty());
        }
    }

    #[test]
    fn successful_correction_discards_older_pages_and_resolves_selection() {
        let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
        let corrected = history_worklog(10, task_id, 100);
        let newest = history_worklog(11, task_id, 300);
        let mut app = correction_app(corrected.clone(), vec![newest.clone(), corrected.clone()]);
        app.handle(Command::Confirm);
        assert_eq!(app.mode(), &Mode::Normal);
        assert_eq!(app.history().unwrap().worklogs.len(), 2);
        assert_eq!(app.history().unwrap().next_cursor, None);
        assert_eq!(app.history_selected_index(), Some(1));
        assert_eq!(app.application.worklog_reads.get(), 2);
        let (id, expected, replacement, occurred_at) = app.application.correction_calls[0];
        assert_eq!(id, corrected.id());
        assert_eq!(expected, corrected.times());
        assert_eq!(replacement, corrected.times());
        assert!(occurred_at <= Utc::now());

        let mut app = correction_app(corrected.clone(), vec![newest.clone()]);
        app.handle(Command::Confirm);
        assert_eq!(app.history().unwrap().selected, Some(newest.id()));
        assert_eq!(app.history_selected_index(), Some(0));
    }

    #[test]
    fn correction_and_history_commands_require_their_own_screen_and_mode() {
        let task = task(1, "alpha");
        let worklog = history_worklog(10, task.id, 100);
        let mut service = TestService::with_tasks(vec![task]);
        service.worklog_pages = vec![Ok(page(vec![worklog], None))];
        let mut app = App::load(service);
        app.handle(Command::OpenHistory);
        app.screen = Screen::TaskList;
        app.handle(Command::OpenCorrection);
        assert_eq!(app.mode(), &Mode::Normal);
        assert_eq!(app.screen(), Screen::TaskList);

        let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
        let worklog = history_worklog(10, task_id, 100);
        let mut app = correction_app(worklog.clone(), vec![worklog]);
        let reads = app.application.worklog_reads.get();
        app.handle(Command::LoadOlderWorklogs);
        assert_eq!(app.application.worklog_reads.get(), reads);
        app.handle(Command::RefreshWorklogs);
        assert_eq!(app.application.worklog_reads.get(), reads);
        app.handle(Command::BackToTaskList);
        assert_eq!(app.screen(), Screen::WorklogHistory);
        assert!(app.correction().is_some());
    }

    #[test]
    fn unavailable_history_renders_its_retry_message_with_the_focused_border() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::style::Color;

        let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
        let worklog = history_worklog(10, task_id, 100);
        let mut app = correction_app(worklog.clone(), vec![worklog]);
        app.mode = Mode::Normal;
        app.mark_history_unavailable();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| crate::ui::render(frame, &app))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let text = (0..buffer.area.height)
            .flat_map(|y| (0..buffer.area.width).map(move |x| buffer[(x, y)].symbol()))
            .collect::<String>();

        assert!(text.contains("History unavailable. Press r to retry."));
        assert_eq!(buffer[(0, 1)].fg, Color::Blue);
    }

    #[test]
    fn saved_correction_marks_history_unavailable_until_retry_succeeds() {
        let task_id = TaskId::from_uuid(uuid::Uuid::from_u128(1));
        let original = history_worklog(10, task_id, 100);
        let corrected = Worklog::new(worklog_id(10), task_id, at(110), Some(at(170))).unwrap();
        let mut service = TestService::with_tasks(vec![task(1, "alpha")]);
        service.worklog_pages = vec![
            Ok(page(vec![original.clone()], Some(cursor(100, 10)))),
            Err(TestService::failure()),
            Ok(page(vec![corrected.clone()], None)),
        ];
        let mut app = App::load(service);
        app.freeze_offset_for_tests(FixedOffset::east_opt(0).unwrap());
        app.handle(Command::OpenHistory);
        app.handle(Command::OpenCorrection);
        {
            let draft = app.correction_mut_for_tests();
            draft.start.replace("1970-01-01 00:01".to_owned());
            draft
                .end
                .as_mut()
                .unwrap()
                .replace("1970-01-01 00:02".to_owned());
        }

        app.handle(Command::Confirm);

        let history = app.history().unwrap();
        assert_eq!(history.availability, HistoryAvailability::Unavailable);
        assert!(!history.is_available());
        assert!(history.worklogs.is_empty());
        assert_eq!(history.next_cursor, None);
        assert_eq!(history.selected, Some(original.id()));
        assert_eq!(app.history_selected_index(), None);
        assert_eq!(app.mode(), &Mode::Normal);
        assert_eq!(
            app.status(),
            &Status::Error("Correction saved, but history refresh failed".to_owned())
        );

        app.handle(Command::MoveDown);
        app.handle(Command::MoveUp);
        assert_eq!(app.history().unwrap().selected, Some(original.id()));

        app.handle(Command::OpenCorrection);
        app.handle(Command::LoadOlderWorklogs);
        assert_eq!(app.application.worklog_reads.get(), 2);
        assert!(app.correction().is_none());

        app.handle(Command::RefreshWorklogs);
        let history = app.history().unwrap();
        assert_eq!(history.availability, HistoryAvailability::Available);
        assert_eq!(history.worklogs, vec![corrected]);
        assert_eq!(history.next_cursor, None);
        assert_eq!(app.history_selected_index(), Some(0));
        assert_eq!(app.status(), &Status::Info("Refreshed".to_owned()));
    }

    #[test]
    fn failed_post_save_reload_keeps_the_externally_corrected_active_aggregate() {
        let active_task = task(1, "active");
        let corrected_task = task(2, "corrected");
        let active = Worklog::begin(worklog_id(10), active_task.id, at(600));
        let corrected = history_worklog(11, corrected_task.id, 480);
        let mut service =
            TestService::with_tasks(vec![active_task.clone(), corrected_task.clone()]);
        service.latest_work_starts = vec![(active_task.id, at(600)), (corrected_task.id, at(480))];
        service.tracking = TrackingState::Running {
            worklog: ActiveWorklog::begin(active.id(), active.task_id(), active.start()),
        };
        service.worklog_pages = vec![
            Ok(page_with_active(
                vec![corrected.clone()],
                Some(active.clone()),
                None,
            )),
            Err(TestService::failure()),
        ];
        let mut app = App::load(service);
        app.freeze_offset_for_tests(FixedOffset::east_opt(0).unwrap());
        app.handle(Command::MoveDown);
        app.handle(Command::OpenHistory);
        app.handle(Command::OpenCorrection);
        app.application.latest_work_starts[0].1 = at(540);
        app.application.tracking = TrackingState::Running {
            worklog: ActiveWorklog::begin(active.id(), active.task_id(), at(540)),
        };
        {
            let draft = app.correction_mut_for_tests();
            draft.start.replace("1970-01-01 00:07".to_owned());
            draft
                .end
                .as_mut()
                .unwrap()
                .replace("1970-01-01 00:08".to_owned());
        }

        app.handle(Command::Confirm);

        assert_eq!(
            app.application
                .tasks(TaskOrdering::RecentlyWorked)
                .into_iter()
                .find(|item| item.task.id == active_task.id)
                .unwrap()
                .latest_work_start,
            Some(at(540))
        );
        assert_eq!(
            app.status(),
            &Status::Error("Correction saved, but history refresh failed".to_owned())
        );
    }

    #[test]
    fn correction_is_available_from_archived_history() {
        let mut archived = task(1, "archived");
        archived.archive(at(200));
        let worklog = history_worklog(10, archived.id, 100);
        let mut service = TestService::with_tasks(vec![archived]);
        service.worklog_pages = vec![
            Ok(page(vec![worklog.clone()], None)),
            Ok(page(vec![worklog], None)),
        ];
        let mut app = App::load(service);
        app.handle(Command::ShowArchivedTasks);
        app.handle(Command::OpenHistory);
        app.handle(Command::OpenCorrection);
        assert!(app.correction().is_some());
        app.handle(Command::Confirm);
        assert_eq!(app.mode(), &Mode::Normal);
        assert_eq!(app.view(), TaskView::Archived);
    }

    #[test]
    fn successful_correction_refreshes_recently_worked_ordering() {
        let repository = SqliteRepository::open_in_memory().unwrap();
        let alpha = task(1, "alpha");
        let beta = task(2, "beta");
        repository.create_task(alpha.clone()).unwrap();
        repository.create_task(beta.clone()).unwrap();
        repository
            .insert_worklog(&history_worklog(10, alpha.id, 200))
            .unwrap();
        repository
            .insert_worklog(&history_worklog(11, beta.id, 300))
            .unwrap();
        let mut app = App::load(TrackerApplication::load(repository).unwrap());
        app.freeze_offset_for_tests(FixedOffset::east_opt(0).unwrap());
        assert_eq!(app.tasks()[0].id, beta.id);
        app.handle(Command::OpenHistory);
        app.handle(Command::OpenCorrection);
        {
            let draft = app.correction_mut_for_tests();
            draft.start.replace("1970-01-01 00:01".to_owned());
            draft
                .end
                .as_mut()
                .unwrap()
                .replace("1970-01-01 00:02".to_owned());
        }
        app.handle(Command::Confirm);
        assert_eq!(app.tasks()[0].id, alpha.id);
        assert_eq!(app.tasks()[1].id, beta.id);
    }

    #[test]
    fn correction_reload_uses_the_final_active_start_for_elapsed_and_stopping() {
        let task = task(1, "alpha");
        let now = Utc::now();
        let initial = Worklog::begin(worklog_id(10), task.id, now - TimeDelta::minutes(10));
        let correction_start = now - TimeDelta::minutes(1);
        let final_start = now - TimeDelta::minutes(2);
        let final_active = Worklog::begin(worklog_id(10), task.id, final_start);
        let mut service = TestService::with_tasks(vec![task]);
        service.worklog_pages = vec![
            Ok(page_with_active(vec![initial.clone()], Some(initial), None)),
            Ok(page_with_active(
                vec![final_active.clone()],
                Some(final_active.clone()),
                None,
            )),
        ];
        let mut app = App::load(service);
        app.freeze_offset_for_tests(FixedOffset::east_opt(0).unwrap());
        app.handle(Command::OpenHistory);
        app.handle(Command::OpenCorrection);
        app.correction_mut_for_tests().start.replace(
            correction_timestamp(correction_start, &FixedOffset::east_opt(0).unwrap())
                .expect("the test timestamp is representable"),
        );

        app.handle(Command::Confirm);

        app.freeze_elapsed_for_tests(Duration::from_secs(180));
        assert!(app.elapsed().unwrap() >= Duration::from_secs(180));
        assert_eq!(app.history_row_duration(&final_active).as_secs(), 180);
        app.handle(Command::BackToTaskList);
        app.handle(Command::ToggleTracking);
        let (_, stopped_at) = app.application.clear_calls[0];
        assert!(stopped_at >= final_start + TimeDelta::seconds(180));
        assert!(stopped_at < final_start + TimeDelta::seconds(181));
    }

    #[test]
    fn completed_correction_preserves_a_frozen_timer_across_a_wall_clock_jump() {
        let task = task(1, "alpha");
        let active = Worklog::begin(worklog_id(10), task.id, at(100));
        let completed = Worklog::new(worklog_id(11), task.id, at(50), Some(at(60))).unwrap();
        let mut service = TestService::with_tasks(vec![task]);
        service.worklog_pages = vec![
            Ok(page_with_active(
                vec![active.clone(), completed.clone()],
                Some(active.clone()),
                None,
            )),
            Ok(page_with_active(
                vec![active.clone(), completed],
                Some(active.clone()),
                None,
            )),
        ];
        let mut app = App::load(service);
        app.freeze_offset_for_tests(FixedOffset::east_opt(0).unwrap());
        app.handle(Command::OpenHistory);
        app.handle(Command::MoveDown);
        app.handle(Command::OpenCorrection);
        app.freeze_elapsed_for_tests(Duration::from_secs(600));

        app.handle(Command::Confirm);
        app.sync_tracking_after_history_reload_at(at(10_000), Instant::now());

        app.handle(Command::BackToTaskList);
        app.handle(Command::ToggleTracking);
        let (_, stopped_at) = app.application.clear_calls[0];
        assert!(stopped_at >= at(700));
        assert!(stopped_at < at(701));
    }

    #[test]
    fn correcting_active_start_reanchors_header_and_running_row_without_negatives() {
        let task = task(1, "alpha");
        let now = Utc::now();
        let original_start = now - TimeDelta::minutes(10);
        let corrected_start = now - TimeDelta::minutes(1);
        let original = Worklog::begin(worklog_id(10), task.id, original_start);
        let corrected = Worklog::begin(worklog_id(10), task.id, corrected_start);
        let mut service = TestService::with_tasks(vec![task.clone()]);
        service.tracking = TrackingState::Running {
            worklog: ActiveWorklog::begin(original.id(), task.id, original_start),
        };
        service.worklog_pages = vec![
            Ok(page(vec![original], None)),
            Ok(page(vec![corrected.clone()], None)),
        ];
        let mut app = App::load(service);
        app.freeze_offset_for_tests(FixedOffset::east_opt(0).unwrap());
        app.handle(Command::OpenHistory);
        app.handle(Command::OpenCorrection);
        app.correction_mut_for_tests().start.replace(
            correction_timestamp(corrected_start, &FixedOffset::east_opt(0).unwrap())
                .expect("the test timestamp is representable"),
        );
        app.handle(Command::Confirm);

        let header = app.elapsed().unwrap();
        let row = app.history_row_duration(&corrected);
        assert!(header >= Duration::from_secs(59) && header < Duration::from_secs(62));
        assert!(row >= Duration::from_secs(59) && row < Duration::from_secs(62));

        assert!(app.elapsed().unwrap() >= Duration::from_secs(59));
    }
}

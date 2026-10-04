use std::path::PathBuf;

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use clap::{Args, Parser, Subcommand, ValueEnum};
use tracker_domain::{TaskId, WorklogId};

#[derive(Debug, Parser)]
#[command(
    name = "tt-cli",
    version,
    about = "Time Tracker commands for scripts and agents"
)]
pub struct Cli {
    /// Use an explicit SQLite database instead of the TUI's default database.
    #[arg(long, global = true, conflicts_with = "server")]
    pub db: Option<PathBuf>,
    /// Use a tracker server, with no local database fallback.
    #[arg(long, global = true, conflicts_with = "db")]
    pub server: Option<String>,
    /// The operation timestamp. Defaults to the current instant.
    #[arg(long, global = true, value_parser = timestamp)]
    pub at: Option<DateTime<Utc>>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Manage active and archived tasks.
    Tasks {
        #[command(subcommand)]
        command: Tasks,
    },
    /// Inspect tracking or set the desired tracking state.
    Tracking {
        #[command(subcommand)]
        command: Tracking,
    },
    /// Read or change worklog history with explicit concurrency guards.
    Worklogs {
        #[command(subcommand)]
        command: Worklogs,
    },
    /// Sum time in a half-open UTC interval or inclusive local calendar dates.
    Reports(ReportArgs),
}

#[derive(Debug, Subcommand)]
pub enum Tasks {
    List {
        #[arg(long, value_enum, default_value = "active")]
        state: TaskState,
        #[arg(long, value_enum, default_value = "worked")]
        sort: TaskSort,
        /// Case-insensitive subsequence search, ordered by recent activity.
        #[arg(long)]
        search: Option<String>,
    },
    Get {
        task_id: TaskId,
    },
    Create {
        name: String,
    },
    Rename {
        task_id: TaskId,
        name: String,
    },
    Archive {
        task_id: TaskId,
    },
    Restore {
        task_id: TaskId,
    },
    /// Preview tasks inactive for more than 14 days. Keep the returned token.
    PreviewInactive,
    /// Archive exactly the previously previewed candidate set.
    ArchiveInactive {
        /// JSON token from preview-inactive, or @PATH containing that token.
        #[arg(long)]
        preview: String,
        /// Explicit consent to archive the previewed tasks.
        #[arg(long, required = true)]
        yes: bool,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum TaskState {
    Active,
    Archived,
    All,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum TaskSort {
    Worked,
    Updated,
    Created,
}

#[derive(Debug, Subcommand)]
pub enum Tracking {
    Status,
    /// Start or switch atomically. Starting the active task leaves it running.
    Start {
        task_id: TaskId,
    },
    /// Stop only the expected timer. Repeating a stop while idle is safe.
    Stop {
        #[arg(long)]
        expected_active: WorklogId,
    },
}

#[derive(Debug, Args)]
pub struct ExpectedTimes {
    #[arg(long, value_parser = timestamp)]
    pub expected_start: DateTime<Utc>,
    /// RFC3339 timestamp with offset, or 'running' for an active worklog.
    #[arg(long, value_parser = end_timestamp)]
    pub expected_end: EndTimestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndTimestamp(pub Option<DateTime<Utc>>);

#[derive(Debug, Subcommand)]
pub enum Worklogs {
    /// Read one page of up to 50 rows, including archived tasks.
    List {
        #[arg(long)]
        task: Option<TaskId>,
        /// JSON next_cursor from the preceding page.
        #[arg(long)]
        cursor: Option<String>,
    },
    Correct {
        worklog_id: WorklogId,
        #[command(flatten)]
        expected: ExpectedTimes,
        /// Replacement start. Omission preserves the expected start.
        #[arg(long, value_parser = timestamp)]
        start: Option<DateTime<Utc>>,
        /// Replacement end. Omission preserves the expected end.
        #[arg(long, value_parser = end_timestamp)]
        end: Option<EndTimestamp>,
    },
    Move {
        worklog_id: WorklogId,
        destination_task: TaskId,
        #[arg(long)]
        expected_task: TaskId,
        #[command(flatten)]
        expected: ExpectedTimes,
    },
    Delete {
        worklog_id: WorklogId,
        #[arg(long)]
        expected_task: TaskId,
        #[command(flatten)]
        expected: ExpectedTimes,
        /// Explicit consent to permanently delete this completed worklog.
        #[arg(long, required = true)]
        yes: bool,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Preset {
    Today,
    Yesterday,
    Week,
    Month,
    Year,
}

#[derive(Debug, Args)]
pub struct ReportArgs {
    /// Defaults to today when no explicit range is supplied.
    #[arg(value_enum, conflicts_with_all = ["start", "from"])]
    pub preset: Option<Preset>,
    /// Inclusive RFC3339 instant, with an explicit offset.
    #[arg(long, value_parser = timestamp, requires = "end", conflicts_with = "from")]
    pub start: Option<DateTime<Utc>>,
    /// Exclusive RFC3339 instant, with an explicit offset.
    #[arg(long, value_parser = timestamp, requires = "start", conflicts_with = "to")]
    pub end: Option<DateTime<Utc>>,
    /// First inclusive calendar date, YYYY-MM-DD.
    #[arg(long, requires = "to")]
    pub from: Option<NaiveDate>,
    /// Last inclusive calendar date, YYYY-MM-DD.
    #[arg(long, requires = "from")]
    pub to: Option<NaiveDate>,
    /// IANA timezone. Defaults to valid TZ, the system timezone, then UTC.
    #[arg(long)]
    pub timezone: Option<Tz>,
}

pub fn timestamp(value: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(value)
        .map(|at| at.with_timezone(&Utc))
        .map_err(|_| "expected an RFC3339 timestamp with a UTC offset".to_owned())
}

pub fn end_timestamp(value: &str) -> Result<EndTimestamp, String> {
    if value == "running" {
        Ok(EndTimestamp(None))
    } else {
        timestamp(value).map(|at| EndTimestamp(Some(at)))
    }
}

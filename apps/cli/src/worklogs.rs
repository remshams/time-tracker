use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracker_application::{GlobalWorklogCursor, WorklogCursor};
use tracker_domain::{TaskId, Worklog, WorklogId, WorklogTimes};
use tracker_protocol::{GlobalWorklogCursorDto, WorklogCursorDto, WorklogDto};

use crate::args::{ExpectedTimes, Worklogs};
use crate::backend::Backend;
use crate::{CliError, read_json};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum CursorToken {
    Task {
        backend: String,
        cursor: WorklogCursorDto,
    },
    All {
        backend: String,
        cursor: GlobalWorklogCursorDto,
    },
}

pub(crate) fn validate(
    command: &mut Worklogs,
    identity: &str,
    now: DateTime<Utc>,
) -> Result<(), CliError> {
    match command {
        Worklogs::List {
            task,
            cursor: Some(text),
        } => {
            let token: CursorToken = read_json(text)?;
            decode_cursor(&token, *task, identity)?;
            *text = serde_json::to_string(&token).map_err(CliError::input)?;
        }
        Worklogs::Correct {
            start: None,
            end: None,
            ..
        } => return Err(CliError::input("correction requires --start or --end")),
        Worklogs::Delete { yes, expected, .. } => {
            if !*yes {
                return Err(CliError::input("delete requires --yes"));
            }
            if expected.expected_end.0.is_none() {
                return Err(CliError::input("only completed worklogs can be deleted"));
            }
        }
        _ => {}
    }
    validate_mutation(command, now)?;
    Ok(())
}

fn canonical(at: DateTime<Utc>) -> DateTime<Utc> {
    DateTime::from_timestamp_micros(at.timestamp_micros()).expect("UTC timestamp fits microseconds")
}

fn selected(id: WorklogId, task: TaskId, expected: &ExpectedTimes) -> Result<Worklog, CliError> {
    Worklog::new(
        id,
        task,
        canonical(expected.expected_start),
        expected.expected_end.0.map(canonical),
    )
    .map_err(CliError::input)
}

fn validate_mutation(command: &Worklogs, now: DateTime<Utc>) -> Result<(), CliError> {
    match command {
        Worklogs::Correct {
            expected,
            start,
            end,
            ..
        } => {
            let original = WorklogTimes::new(
                canonical(expected.expected_start),
                expected.expected_end.0.map(canonical),
            );
            original.validate().map_err(CliError::input)?;
            let replacement = replacement(expected, *start, *end);
            let replacement = WorklogTimes::new(
                canonical(replacement.start()),
                replacement.end().map(canonical),
            );
            original
                .validate_correction(replacement, canonical(now))
                .map_err(CliError::input)?;
        }
        Worklogs::Move {
            worklog_id,
            expected_task,
            expected,
            destination_task,
        } => {
            selected(*worklog_id, *expected_task, expected)?
                .moved_to(*destination_task)
                .map_err(CliError::input)?;
        }
        Worklogs::Delete {
            worklog_id,
            expected_task,
            expected,
            ..
        } => {
            selected(*worklog_id, *expected_task, expected)?;
        }
        Worklogs::List { .. } => {}
    }
    Ok(())
}

fn replacement(
    expected: &ExpectedTimes,
    start: Option<DateTime<Utc>>,
    end: Option<crate::args::EndTimestamp>,
) -> WorklogTimes {
    WorklogTimes::new(
        start.unwrap_or(expected.expected_start),
        end.map_or(expected.expected_end.0, |end| end.0),
    )
}

enum DecodedCursor {
    Task(WorklogCursor),
    All(GlobalWorklogCursor),
}

fn decode_cursor(
    token: &CursorToken,
    task: Option<TaskId>,
    identity: &str,
) -> Result<DecodedCursor, CliError> {
    match token {
        CursorToken::Task { backend, cursor } => {
            check_backend(backend, identity)?;
            let task_id = cursor.task_id.parse().map_err(CliError::input)?;
            if task != Some(task_id) {
                return Err(CliError::input(
                    "cursor belongs to a different task or scope",
                ));
            }
            Ok(DecodedCursor::Task(WorklogCursor {
                task_id,
                start: cursor.start,
                id: cursor.id.parse().map_err(CliError::input)?,
                revision: cursor.revision,
            }))
        }
        CursorToken::All { backend, cursor } => {
            check_backend(backend, identity)?;
            if task.is_some() {
                return Err(CliError::input("global cursor cannot be used with --task"));
            }
            Ok(DecodedCursor::All(GlobalWorklogCursor {
                start: cursor.start,
                id: cursor.id.parse().map_err(CliError::input)?,
                revision: cursor.revision,
            }))
        }
    }
}

fn check_backend(actual: &str, expected: &str) -> Result<(), CliError> {
    if actual == expected {
        Ok(())
    } else {
        Err(CliError::input("cursor belongs to a different backend"))
    }
}

fn times(expected: &ExpectedTimes) -> WorklogTimes {
    WorklogTimes::new(expected.expected_start, expected.expected_end.0)
}

pub(crate) async fn execute(
    backend: &mut Backend,
    command: Worklogs,
    now: DateTime<Utc>,
) -> Result<Value, CliError> {
    match command {
        Worklogs::List { task, cursor } => list(backend, task, cursor.as_deref()).await,
        Worklogs::Correct {
            worklog_id,
            expected,
            start,
            end,
        } => {
            let replacement = replacement(&expected, start, end);
            let worklog = backend
                .correct_worklog(worklog_id, times(&expected), replacement, now)
                .await?;
            crate::json(WorklogDto::from(&worklog))
        }
        Worklogs::Move {
            worklog_id,
            destination_task,
            expected_task,
            expected,
        } => {
            let worklog = backend
                .move_worklog(
                    worklog_id,
                    expected_task,
                    times(&expected),
                    destination_task,
                )
                .await?;
            crate::json(WorklogDto::from(&worklog))
        }
        Worklogs::Delete {
            worklog_id,
            expected_task,
            expected,
            ..
        } => {
            let worklog = backend
                .delete_completed_worklog(worklog_id, expected_task, times(&expected))
                .await?;
            Ok(json!({"deleted": WorklogDto::from(&worklog)}))
        }
    }
}

async fn list(
    backend: &mut Backend,
    task: Option<TaskId>,
    cursor: Option<&str>,
) -> Result<Value, CliError> {
    let cursor = cursor.map(read_json::<CursorToken>).transpose()?;
    let cursor = cursor
        .as_ref()
        .map(|token| decode_cursor(token, task, &backend.identity))
        .transpose()?;
    if let Some(task) = task {
        let after = match cursor {
            Some(DecodedCursor::Task(cursor)) => Some(cursor),
            _ => None,
        };
        let page = backend.worklogs_for_task(task, after.as_ref()).await?;
        let next = page.next_cursor.map(|cursor| CursorToken::Task {
            backend: backend.identity.clone(),
            cursor: cursor.into(),
        });
        Ok(
            json!({"worklogs": page.worklogs.iter().map(WorklogDto::from).collect::<Vec<_>>(), "next_cursor": next}),
        )
    } else {
        let after = match cursor {
            Some(DecodedCursor::All(cursor)) => Some(cursor),
            _ => None,
        };
        let page = backend.all_worklogs(after.as_ref()).await?;
        let next = page.next_cursor.map(|cursor| CursorToken::All {
            backend: backend.identity.clone(),
            cursor: cursor.into(),
        });
        Ok(
            json!({"worklogs": page.worklogs.iter().map(WorklogDto::from).collect::<Vec<_>>(), "next_cursor": next}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::EndTimestamp;

    fn correction(start: DateTime<Utc>, end: DateTime<Utc>, replacement: EndTimestamp) -> Worklogs {
        Worklogs::Correct {
            worklog_id: "00000000-0000-7000-8000-000000000001".parse().unwrap(),
            expected: ExpectedTimes {
                expected_start: start,
                expected_end: EndTimestamp(Some(end)),
            },
            start: None,
            end: Some(replacement),
        }
    }

    #[test]
    fn original_interval_errors_precede_replacement_completion_errors() {
        let start = DateTime::from_timestamp(200, 0).unwrap();
        let end = DateTime::from_timestamp(100, 0).unwrap();
        let mut command = correction(start, end, EndTimestamp(None));
        let error = validate(&mut command, "unused", start).unwrap_err();
        assert_eq!(error.message, "worklog end must not precede its start");
        assert_eq!(error.exit_code, 2);
    }

    #[test]
    fn original_and_replacement_intervals_compare_canonical_microseconds() {
        let earlier = DateTime::from_timestamp(100, 1).unwrap();
        let later = DateTime::from_timestamp(100, 999).unwrap();
        let now = DateTime::from_timestamp(100, 0).unwrap();
        let mut command = correction(later, earlier, EndTimestamp(Some(later)));
        assert!(validate(&mut command, "unused", now).is_ok());
    }
}

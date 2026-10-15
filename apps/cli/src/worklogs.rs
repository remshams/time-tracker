use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracker_application::timestamps::{canonical_timestamp as canonical, canonical_worklog_times};
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

/// Rebuilds the reviewed worklog at stored precision so validation matches backend writes.
fn selected(id: WorklogId, task: TaskId, expected: &ExpectedTimes) -> Result<Worklog, CliError> {
    Worklog::new(
        id,
        task,
        canonical(expected.expected_start),
        expected.expected_end.0.map(canonical),
    )
    .map_err(CliError::input)
}

/// Checks reviewed intervals and correction rules at microsecond precision before execution.
fn validate_mutation(command: &Worklogs, now: DateTime<Utc>) -> Result<(), CliError> {
    match command {
        Worklogs::Correct {
            expected,
            start,
            end,
            ..
        } => {
            let original = canonical_worklog_times(times(expected));
            original.validate().map_err(CliError::input)?;
            let replacement = canonical_worklog_times(replacement(expected, *start, *end));
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
            if task.is_none() {
                return Err(CliError::input("task cursor requires --task"));
            }
            validate_revision(cursor.revision)?;
            Ok(DecodedCursor::Task(WorklogCursor {
                id: cursor.id.parse().map_err(CliError::input)?,
                revision: cursor.revision,
            }))
        }
        CursorToken::All { backend, cursor } => {
            check_backend(backend, identity)?;
            if task.is_some() {
                return Err(CliError::input("global cursor cannot be used with --task"));
            }
            validate_revision(cursor.revision)?;
            Ok(DecodedCursor::All(GlobalWorklogCursor {
                id: cursor.id.parse().map_err(CliError::input)?,
                revision: cursor.revision,
            }))
        }
    }
}

fn validate_revision(revision: i64) -> Result<(), CliError> {
    if revision < 0 {
        Err(CliError::input("cursor revision must be nonnegative"))
    } else {
        Ok(())
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

    #[test]
    fn minimal_cursor_tokens_accept_both_scopes_and_nonnegative_revisions() {
        let id = "00000000-0000-7000-8000-000000000001";
        let task = Some(id.parse().unwrap());
        for revision in [0, 7] {
            for scope in ["task", "all"] {
                let token: CursorToken = serde_json::from_value(json!({
                    "scope": scope,
                    "backend": "local",
                    "cursor": {"id": id, "revision": revision}
                }))
                .unwrap();
                let selected = if scope == "task" { task } else { None };
                assert!(decode_cursor(&token, selected, "local").is_ok());
                assert!(decode_cursor(&token, selected, "other").is_err());
                assert!(
                    decode_cursor(
                        &token,
                        if selected.is_some() { None } else { task },
                        "local"
                    )
                    .is_err()
                );
            }
        }
    }

    #[test]
    fn cursor_tokens_reject_invalid_partial_and_legacy_positions() {
        let id = "00000000-0000-7000-8000-000000000001";
        for scope in ["task", "all"] {
            for cursor in [
                json!({"id": id}),
                json!({"revision": 0}),
                json!({"id": "invalid", "revision": 0}),
                json!({"id": id, "revision": -1}),
                json!({"id": id, "revision": 0, "start": "2026-01-01T00:00:00Z"}),
                json!({"id": id, "revision": 0, "task_id": id}),
            ] {
                let token = serde_json::from_value::<CursorToken>(json!({
                    "scope": scope, "backend": "local", "cursor": cursor
                }));
                if let Ok(token) = token {
                    assert!(
                        decode_cursor(
                            &token,
                            if scope == "task" {
                                Some(id.parse().unwrap())
                            } else {
                                None
                            },
                            "local"
                        )
                        .is_err()
                    );
                }
            }
        }
    }

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
        assert!(validate(&mut command, "unused", now).is_err());
    }
}

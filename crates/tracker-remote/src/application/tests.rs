use super::*;
use chrono::Duration;
use std::time::Duration as StdDuration;
fn at() -> DateTime<Utc> {
    DateTime::from_timestamp(1_800_000_000, 0).unwrap()
}
fn worklog(id: WorklogId, task_id: TaskId, end: Option<DateTime<Utc>>) -> WorklogDto {
    WorklogDto {
        id: id.to_string(),
        task_id: task_id.to_string(),
        start: at() + Duration::seconds(10),
        end,
    }
}
#[test]
fn report_cooldown_ends_at_five_seconds() {
    let now = Instant::now();
    assert!(!report_cooldown_active(None, now));
    assert!(report_cooldown_active(
        Some(now - StdDuration::from_millis(4_999)),
        now
    ));
    assert!(!report_cooldown_active(
        Some(now - StdDuration::from_secs(5)),
        now
    ));
    assert!(!report_cooldown_active(
        Some(now - StdDuration::from_millis(5_001)),
        now
    ));
}
#[test]
fn tracking_result_checks_every_started_and_stopped_identifier_and_state() {
    let task_id = TaskId::generate();
    let other_task = TaskId::generate();
    let started_id = WorklogId::generate();
    let old_id = WorklogId::generate();
    let start = worklog(started_id, task_id, None);
    let stop = worklog(old_id, task_id, Some(at() + Duration::seconds(20)));

    assert!(matches!(
        set_tracking_result(
            MutationResultDto::Worklog(start.clone()),
            task_id,
            started_id,
            None
        )
        .unwrap(),
        SetActiveTaskOutcome::Started { .. }
    ));
    for (dto, expected_task, expected_id, previous) in [
        (start.clone(), task_id, started_id, Some(old_id)),
        (start.clone(), task_id, old_id, None),
        (start.clone(), other_task, started_id, None),
        (stop.clone(), task_id, old_id, None),
    ] {
        assert!(
            set_tracking_result(
                MutationResultDto::Worklog(dto),
                expected_task,
                expected_id,
                previous
            )
            .is_err()
        );
    }

    let switched = MutationResultDto::TrackingSwitched {
        stopped: stop.clone(),
        started: start.clone(),
    };
    assert!(matches!(
        set_tracking_result(switched, task_id, started_id, Some(old_id)).unwrap(),
        SetActiveTaskOutcome::Switched { .. }
    ));
    for (stopped, started, expected_task, expected_id, previous) in [
        (stop.clone(), start.clone(), task_id, started_id, None),
        (
            start.clone(),
            start.clone(),
            task_id,
            started_id,
            Some(started_id),
        ),
        (stop.clone(), start.clone(), task_id, old_id, Some(old_id)),
        (
            stop.clone(),
            start.clone(),
            other_task,
            started_id,
            Some(old_id),
        ),
        (stop.clone(), stop.clone(), task_id, old_id, Some(old_id)),
    ] {
        assert!(
            set_tracking_result(
                MutationResultDto::TrackingSwitched { stopped, started },
                expected_task,
                expected_id,
                previous
            )
            .is_err()
        );
    }
    assert!(matches!(
        set_tracking_result(
            MutationResultDto::TrackingAlreadyActive(start.clone()),
            task_id,
            started_id,
            Some(old_id)
        )
        .unwrap(),
        SetActiveTaskOutcome::AlreadyActive { .. }
    ));
    assert!(
        set_tracking_result(
            MutationResultDto::TrackingAlreadyActive(stop),
            task_id,
            started_id,
            Some(old_id)
        )
        .is_err()
    );
    assert!(
        set_tracking_result(
            MutationResultDto::TrackingAlreadyActive(start),
            other_task,
            started_id,
            Some(old_id)
        )
        .is_err()
    );
}
#[test]
fn idle_tracking_result_is_a_successful_no_op() {
    assert_eq!(
        clear_tracking_result(
            MutationResultDto::TrackingAlreadyIdle,
            WorklogId::generate()
        )
        .unwrap(),
        ClearActiveTaskOutcome::AlreadyIdle
    );
}
#[test]
fn stopping_result_rejects_the_wrong_worklog_or_an_unfinished_worklog() {
    let expected_id = WorklogId::generate();
    let task_id = TaskId::generate();
    let stopped = worklog(expected_id, task_id, Some(at() + Duration::seconds(20)));
    assert!(matches!(
        clear_tracking_result(MutationResultDto::Worklog(stopped.clone()), expected_id).unwrap(),
        ClearActiveTaskOutcome::Stopped { .. }
    ));
    assert!(
        clear_tracking_result(MutationResultDto::Worklog(stopped), WorklogId::generate()).is_err()
    );
    assert!(
        clear_tracking_result(
            MutationResultDto::Worklog(worklog(expected_id, task_id, None)),
            expected_id
        )
        .is_err()
    );
}
#[test]
fn http_failures_keep_unavailable_conflict_and_protocol_distinct() {
    let error = |status| RemoteError::Http {
        status,
        body: Vec::new(),
    };
    assert_eq!(
        classify_error(&error(StatusCode::INTERNAL_SERVER_ERROR)),
        RemoteFailureKind::Unavailable
    );
    assert_eq!(
        classify_error(&error(StatusCode::REQUEST_TIMEOUT)),
        RemoteFailureKind::Unavailable
    );
    assert_eq!(
        classify_error(&error(StatusCode::CONFLICT)),
        RemoteFailureKind::Conflict
    );
    assert_eq!(
        classify_error(&error(StatusCode::BAD_REQUEST)),
        RemoteFailureKind::Protocol
    );
    assert_eq!(
        classify_error(&RemoteError::Unavailable("offline".into())),
        RemoteFailureKind::Unavailable
    );
}
#[test]
fn not_found_category_depends_on_whether_a_worklog_was_requested() {
    let error = RemoteError::Http {
        status: StatusCode::NOT_FOUND,
        body: serde_json::to_vec(&ErrorDto {
            code: ErrorCode::NotFound,
            message: "Missing entry".into(),
        })
        .unwrap(),
    };
    assert_eq!(
        map_application_error(&error, Some(WorklogId::generate()), None)
            .failure()
            .category(),
        ApplicationFailureCategory::WorklogNotFound
    );
    assert_eq!(
        map_application_error(&error, None, Some(TaskId::generate()))
            .failure()
            .category(),
        ApplicationFailureCategory::TaskNotFound
    );
    let invalid_body = RemoteError::Http {
        status: StatusCode::CONFLICT,
        body: Vec::new(),
    };
    assert_eq!(
        map_application_error(&invalid_body, None, None)
            .failure()
            .message(),
        "Tracker state changed. Refresh and retry."
    );
}
#[test]
fn remote_conflicts_keep_editor_and_archive_reasons_with_safe_messages() {
    let cases = [
        (
            ErrorCode::WorklogHistoryChanged,
            ApplicationFailureCategory::WorklogHistoryChanged,
            "History changed\nRefresh the page".to_owned(),
            "History changedRefresh the page".to_owned(),
        ),
        (
            ErrorCode::WorklogOverlap,
            ApplicationFailureCategory::WorklogOverlap,
            "Worklog overlaps another entry".to_owned(),
            "Worklog overlaps another entry".to_owned(),
        ),
        (
            ErrorCode::ActiveWorklog,
            ApplicationFailureCategory::ActiveWorklog,
            "\tStop the active worklog\r".to_owned(),
            "Stop the active worklog".to_owned(),
        ),
        (
            ErrorCode::ActiveTask,
            ApplicationFailureCategory::ActiveTask,
            "\n\t\r".to_owned(),
            "Server rejected the request".to_owned(),
        ),
        (
            ErrorCode::InactiveTaskCandidatesChanged,
            ApplicationFailureCategory::InactiveTaskCandidatesChanged,
            "x".repeat(520),
            "x".repeat(512),
        ),
    ];
    for (code, category, message, expected_message) in cases {
        let error = RemoteError::Http {
            status: StatusCode::CONFLICT,
            body: serde_json::to_vec(&ErrorDto { code, message }).unwrap(),
        };
        let failure = map_application_error(&error, Some(WorklogId::generate()), None).failure();
        assert_eq!(failure.category(), category);
        assert_eq!(failure.message(), expected_message);
        assert_eq!(
            failure.source(),
            tracker_application::ApplicationFailureSource::Operation
        );
        assert!(!failure.recovery_failed());
    }
}
#[test]
fn application_failures_preserve_transport_origin_and_redact_internal_responses() {
    use tracker_application::ApplicationFailureSource;

    let cases = [
        (
            RemoteError::Unavailable("private URL".into()),
            ApplicationFailureSource::RemoteUnavailable,
        ),
        (
            RemoteError::Protocol("private response body".into()),
            ApplicationFailureSource::RemoteProtocol,
        ),
        (
            RemoteError::InvalidEndpoint("private endpoint"),
            ApplicationFailureSource::RemoteProtocol,
        ),
        (
            RemoteError::Http {
                status: StatusCode::BAD_REQUEST,
                body: b"private body".to_vec(),
            },
            ApplicationFailureSource::RemoteProtocol,
        ),
        (
            RemoteError::Http {
                status: StatusCode::REQUEST_TIMEOUT,
                body: Vec::new(),
            },
            ApplicationFailureSource::RemoteUnavailable,
        ),
        (
            RemoteError::Http {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                body: serde_json::to_vec(&ErrorDto {
                    code: ErrorCode::WorklogChanged,
                    message: "private database credentials".into(),
                })
                .unwrap(),
            },
            ApplicationFailureSource::RemoteUnavailable,
        ),
        (
            RemoteError::Http {
                status: StatusCode::BAD_REQUEST,
                body: serde_json::to_vec(&ErrorDto {
                    code: ErrorCode::Internal,
                    message: "private database credentials".into(),
                })
                .unwrap(),
            },
            ApplicationFailureSource::RemoteUnavailable,
        ),
    ];
    for (error, source) in cases {
        let failure = map_application_error(&error, None, None).failure();
        assert_eq!(failure.source(), source);
        assert_eq!(failure.category(), ApplicationFailureCategory::General);
        assert!(!failure.recovery_failed());
        assert!(!failure.message().contains("private"));
    }
    let error = RemoteError::Http {
        status: StatusCode::CONFLICT,
        body: serde_json::to_vec(&ErrorDto {
            code: ErrorCode::WorklogChanged,
            message: "Worklog changed".into(),
        })
        .unwrap(),
    };
    let failure = map_application_error(&error, None, None).failure();
    assert_eq!(
        failure.category(),
        ApplicationFailureCategory::WorklogChanged
    );
    assert_eq!(failure.source(), ApplicationFailureSource::Operation);
}

use super::*;
fn at() -> DateTime<Utc> {
    DateTime::from_timestamp(1_800_000_000, 0).unwrap()
}
fn task(id: TaskId) -> TaskDto {
    TaskDto {
        id: id.to_string(),
        name: "Project work".into(),
        archived: false,
        created_at: at(),
        updated_at: at(),
        latest_work_start: None,
    }
}
#[test]
fn task_catalog_rejects_duplicates_and_invalid_tasks() {
    let item = task(TaskId::generate());
    let other = task(TaskId::generate());
    assert_eq!(
        decode_tasks(TasksDto {
            tasks: vec![item.clone(), other.clone()],
        })
        .unwrap()
        .len(),
        2
    );
    assert!(decode_tasks(TasksDto { tasks: vec![] }).unwrap().is_empty());
    for dto in [
        TasksDto {
            tasks: vec![item.clone(), item.clone()],
        },
        TasksDto {
            tasks: vec![TaskDto {
                id: "invalid".into(),
                ..item.clone()
            }],
        },
        TasksDto {
            tasks: vec![TaskDto {
                name: "Bad\u{1b}name".into(),
                ..item
            }],
        },
    ] {
        assert!(decode_tasks(dto).is_err());
    }
}
#[test]
fn tracking_accepts_a_task_absent_from_the_catalog_and_rejects_completed_entries() {
    let active = WorklogDto {
        id: WorklogId::generate().to_string(),
        task_id: TaskId::generate().to_string(),
        start: at(),
        end: None,
    };
    assert!(
        decode_tracking(TrackingDto {
            active_worklog: Some(active.clone()),
        })
        .is_ok()
    );
    assert!(
        decode_tracking(TrackingDto {
            active_worklog: Some(WorklogDto {
                end: Some(at()),
                ..active
            }),
        })
        .is_err()
    );
    assert_eq!(
        decode_tracking(TrackingDto {
            active_worklog: None,
        })
        .unwrap(),
        (None, TrackingState::Idle)
    );
}
#[test]
fn reports_validate_ids_durations_totals_and_range_without_task_cache_membership() {
    let row = tracker_protocol::ReportRowDto {
        task_id: TaskId::generate().to_string(),
        duration_us: 2,
    };
    let valid = ReportDto {
        start: at(),
        end: at() + TimeDelta::seconds(10),
        now: at(),
        rows: vec![row.clone()],
        total_us: 2,
    };
    assert!(validate_report(&valid).is_ok());
    for invalid in [
        ReportDto {
            rows: vec![row.clone(), row],
            total_us: 4,
            ..valid.clone()
        },
        ReportDto {
            total_us: 3,
            ..valid.clone()
        },
        ReportDto {
            end: at(),
            ..valid.clone()
        },
        ReportDto {
            rows: vec![tracker_protocol::ReportRowDto {
                task_id: "invalid".into(),
                duration_us: 2,
            }],
            ..valid.clone()
        },
        ReportDto {
            rows: vec![tracker_protocol::ReportRowDto {
                task_id: TaskId::generate().to_string(),
                duration_us: 0,
            }],
            total_us: 0,
            ..valid.clone()
        },
        ReportDto {
            rows: vec![
                tracker_protocol::ReportRowDto {
                    task_id: TaskId::generate().to_string(),
                    duration_us: i64::MAX,
                },
                tracker_protocol::ReportRowDto {
                    task_id: TaskId::generate().to_string(),
                    duration_us: 1,
                },
            ],
            ..valid.clone()
        },
    ] {
        assert!(validate_report(&invalid).is_err());
    }
}
/// Checks that persisted confirmation tokens retain exact metadata and reject protocol 5 tokens.
#[test]
fn persisted_preview_tokens_preserve_period_and_candidates() {
    let parsed_ids = vec![TaskId::generate(), TaskId::generate()];
    let candidate_ids: Vec<String> = parsed_ids.iter().map(ToString::to_string).collect();
    let token = serde_json::json!({
        "as_of": at(),
        "inactive_days": 21,
        "count": 2,
        "candidate_task_ids": candidate_ids,
    });
    let preview: InactiveTaskPreview = serde_json::from_value(token.clone()).unwrap();
    assert_eq!(preview.as_of(), at());
    assert_eq!(preview.inactive_days(), 21);
    assert_eq!(preview.count(), 2);
    assert_eq!(preview.dto.candidate_task_ids, candidate_ids);
    assert_eq!(preview.candidate_ids, parsed_ids);
    assert_eq!(serde_json::to_value(&preview).unwrap(), token);

    let mut protocol_5_token = token;
    protocol_5_token["revision"] = serde_json::json!("captured-revision");
    assert!(serde_json::from_value::<InactiveTaskPreview>(protocol_5_token).is_err());
}

/// Pins the saved token text so a loaded preview serializes to the same bytes.
#[test]
fn persisted_preview_token_text_is_unchanged() {
    let token = concat!(
        r#"{"as_of":"2027-01-15T08:00:00Z","inactive_days":14,"count":1,"#,
        r#""candidate_task_ids":["00000000-0000-7000-8000-000000000001"]}"#
    );
    let preview: InactiveTaskPreview = serde_json::from_str(token).unwrap();
    assert_eq!(serde_json::to_string(&preview).unwrap(), token);
}

/// Rejects restored tokens whose cutoff or count was edited after capture.
#[test]
fn restored_preview_tokens_reject_tampered_cutoffs_and_counts() {
    let token = serde_json::json!({
        "as_of": "2027-01-15T08:00:00Z",
        "inactive_days": 14,
        "count": 1,
        "candidate_task_ids": ["00000000-0000-7000-8000-000000000001"],
    });
    assert!(serde_json::from_value::<InactiveTaskPreview>(token.clone()).is_ok());
    for (field, value) in [
        ("as_of", serde_json::json!("2027-01-15T08:00:00.000000001Z")),
        ("count", serde_json::json!(2)),
        ("count", serde_json::json!(0)),
    ] {
        let mut tampered = token.clone();
        tampered[field] = value;
        let error = serde_json::from_value::<InactiveTaskPreview>(tampered).unwrap_err();
        assert_eq!(error.to_string(), "invalid inactive task preview");
    }
}

/// Checks receipt identity before retaining a successful command result.
#[test]
fn command_receipts_require_the_matching_request() {
    let guard = WriteGuard {
        request_id: "expected-command".into(),
    };
    let receipt = MutationDto {
        request_id: guard.request_id.clone(),
        replayed: false,
        result: MutationResultDto::ArchivedInactive { count: 2 },
    };
    let invalid = MutationDto {
        request_id: "different-command".into(),
        ..receipt.clone()
    };
    let mut client = RemoteSession::new("http://127.0.0.1:1").unwrap();
    let result = client.decode_receipt(
        Ok(invalid),
        &guard,
        MutationContext {
            worklog_id: None,
            task_id: None,
        },
        |result| match result {
            MutationResultDto::ArchivedInactive { count } => Ok(count),
            _ => panic!("fixture must return the archived count"),
        },
    );
    assert_eq!(
        result.unwrap_err().failure().source(),
        ApplicationFailureSource::RemoteProtocol
    );
    assert!(client.last_command_receipt().is_none());
    let mut client = RemoteSession::new("http://127.0.0.1:1").unwrap();
    let count = client
        .decode_receipt(
            Ok(receipt.clone()),
            &guard,
            MutationContext {
                worklog_id: None,
                task_id: None,
            },
            |result| match result {
                MutationResultDto::ArchivedInactive { count } => Ok(count),
                _ => panic!("fixture must return the archived count"),
            },
        )
        .unwrap();
    assert_eq!(count, 2);
    assert_eq!(client.last_command_receipt(), Some(&receipt));
}

/// Rejects preview metadata that cannot identify an exact, valid candidate set for confirmation.
#[test]
fn previews_require_complete_unique_ids_and_a_period() {
    let valid = InactiveTaskPreviewDto {
        as_of: at(),
        inactive_days: 7,
        count: 1,
        candidate_task_ids: vec![TaskId::generate().to_string()],
    };
    assert_eq!(
        validate_inactive_preview(&valid).unwrap(),
        vec![valid.candidate_task_ids[0].parse::<TaskId>().unwrap()]
    );
    for invalid in [
        InactiveTaskPreviewDto {
            count: 2,
            ..valid.clone()
        },
        InactiveTaskPreviewDto {
            count: 2,
            candidate_task_ids: vec![valid.candidate_task_ids[0].clone(); 2],
            ..valid.clone()
        },
        InactiveTaskPreviewDto {
            candidate_task_ids: vec!["invalid".into()],
            ..valid.clone()
        },
        InactiveTaskPreviewDto {
            inactive_days: 0,
            ..valid.clone()
        },
        InactiveTaskPreviewDto {
            as_of: at() + TimeDelta::nanoseconds(1),
            ..valid.clone()
        },
    ] {
        assert!(validate_inactive_preview(&invalid).is_err());
        let token = serde_json::to_string(&invalid).unwrap();
        let error = serde_json::from_str::<InactiveTaskPreview>(&token).unwrap_err();
        assert!(error.to_string().contains("invalid inactive task preview"));
    }
}
#[test]
fn move_candidates_use_only_the_confirmed_task_cache() {
    let source = TaskId::generate();
    let destination = TaskId::generate();
    let archived = TaskId::generate();
    let mut client = TasksClient::default();
    client.adopt_catalog(
        vec![
            task(source),
            task(destination),
            TaskDto {
                archived: true,
                ..task(archived)
            },
        ]
        .into_iter()
        .map(|dto| decode_task_item(dto).unwrap())
        .collect(),
    );
    let candidates = client.move_candidates(source, "pW");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].id, destination);
    assert!(client.move_candidates(source, "zz").is_empty());
}

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
fn task_catalog_rejects_duplicates_invalid_tasks_and_empty_revisions() {
    let item = task(TaskId::generate());
    for dto in [
        TasksDto {
            tasks: vec![item.clone(), item.clone()],
            revision: "r1".into(),
        },
        TasksDto {
            tasks: vec![],
            revision: String::new(),
        },
        TasksDto {
            tasks: vec![TaskDto {
                id: "invalid".into(),
                ..item.clone()
            }],
            revision: "r1".into(),
        },
        TasksDto {
            tasks: vec![TaskDto {
                name: "Bad\u{1b}name".into(),
                ..item
            }],
            revision: "r1".into(),
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
            revision: "r1".into()
        })
        .is_ok()
    );
    assert!(
        decode_tracking(TrackingDto {
            active_worklog: Some(WorklogDto {
                end: Some(at()),
                ..active
            }),
            revision: "r1".into()
        })
        .is_err()
    );
    assert!(
        decode_tracking(TrackingDto {
            active_worklog: None,
            revision: String::new()
        })
        .is_err()
    );
}
#[test]
fn reports_validate_ids_durations_totals_and_revision_without_task_cache_membership() {
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
        revision: "r1".into(),
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
            revision: String::new(),
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
#[test]
fn previews_require_complete_unique_ids_a_period_and_a_revision() {
    let valid = InactiveTaskPreviewDto {
        as_of: at(),
        inactive_days: 7,
        count: 1,
        candidate_task_ids: vec![TaskId::generate().to_string()],
        revision: "r1".into(),
    };
    assert!(validate_inactive_preview(&valid).is_ok());
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
            revision: String::new(),
            ..valid.clone()
        },
        InactiveTaskPreviewDto {
            as_of: at() + TimeDelta::nanoseconds(1),
            ..valid.clone()
        },
    ] {
        assert!(validate_inactive_preview(&invalid).is_err());
    }
}
#[test]
fn move_candidates_use_only_the_confirmed_task_cache() {
    let source = TaskId::generate();
    let destination = TaskId::generate();
    let archived = TaskId::generate();
    let mut client = RemoteApplication::disconnected("http://127.0.0.1:1").unwrap();
    client.task_observation = Some(ResourceObservation {
        revision: "confirmed".into(),
        value: vec![
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
    });
    let candidates = client.move_candidates(source, "pW");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].id, destination);
    assert!(client.move_candidates(source, "zz").is_empty());
}

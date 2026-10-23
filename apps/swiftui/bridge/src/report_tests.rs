use super::*;
use crate::test_support::*;
use tracker_application::WORKLOG_PAGE_SIZE;

fn at(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 0).unwrap()
}

fn report(bridge: &mut Bridge, start: i64, end: i64, now: i64) -> Value {
    let start = CString::new(timestamp(at(start))).unwrap();
    let end = CString::new(timestamp(at(end))).unwrap();
    let now = CString::new(timestamp(at(now))).unwrap();
    // SAFETY: The bridge and all strings remain live for this call.
    unsafe { report_at(bridge, start.as_ptr(), end.as_ptr(), now.as_ptr()) }
}

/// Refreshes totals and returns the captured report, or the refresh error.
///
/// # Safety
/// `bridge` must be live or null, and the strings must be valid C strings or null.
unsafe fn report_at(
    bridge: *mut Bridge,
    start: *const c_char,
    end: *const c_char,
    now: *const c_char,
) -> Value {
    // SAFETY: The caller keeps the bridge and all input strings live for this call.
    let result = response(unsafe { tt_bridge_refresh_totals(bridge, start, end, now) });
    if result.get("error").is_some() {
        return result;
    }
    // SAFETY: The refresh succeeded, so the bridge is live.
    response(unsafe { tt_bridge_report_observation(bridge) })
}

fn completed(
    application: &mut TrackerApplication<SqliteRepository>,
    task_id: TaskId,
    start: i64,
    end: i64,
) {
    application.set_active_task(task_id, at(start)).unwrap();
    let TrackingState::Running { worklog } = application.current_tracking() else {
        panic!("tracking should be running");
    };
    application
        .clear_active_task(worklog.id(), at(end))
        .unwrap();
}

#[test]
fn report_aggregates_all_history_and_clips_completed_and_active_work() {
    let repository = SqliteRepository::open_in_memory().unwrap();
    let mut application = TrackerApplication::load(repository).unwrap();
    let archived = application
        .create_task(TaskName::new("Archived work").unwrap(), at(0))
        .unwrap();
    let finished = application
        .create_task(TaskName::new("Finished work").unwrap(), at(0))
        .unwrap();
    let active = application
        .create_task(TaskName::new("Active work").unwrap(), at(0))
        .unwrap();
    let unused = application
        .create_task(TaskName::new("Unused work").unwrap(), at(0))
        .unwrap();
    completed(&mut application, archived.id(), 990, 1005);
    for index in 0..=WORKLOG_PAGE_SIZE as i64 {
        completed(
            &mut application,
            archived.id(),
            1010 + index * 2,
            1011 + index * 2,
        );
    }
    application.archive_task(archived.id(), at(1120)).unwrap();
    completed(&mut application, finished.id(), 1180, 1205);
    application.set_active_task(active.id(), at(1210)).unwrap();
    let mut bridge = Bridge::new(Backend::local(application).unwrap());

    let value = report(&mut bridge, 1000, 1200, 1190);
    assert!(value.get("error").is_none(), "{value}");
    assert_eq!(
        value["data"]["rows"],
        json!([
            {"taskId": archived.id().to_string(), "durationMicroseconds": 56_000_000},
            {"taskId": finished.id().to_string(), "durationMicroseconds": 20_000_000}
        ])
    );
    assert_eq!(
        crate::tests::test_active_value(&bridge.application)["taskId"],
        active.id().to_string()
    );
    assert!(
        crate::tests::test_task_values(&bridge.application)
            .as_array()
            .unwrap()
            .iter()
            .any(|task| task["id"] == archived.id().to_string() && task["archived"] == true)
    );
    assert!(
        !value["data"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["taskId"] == unused.id().to_string())
    );

    let value = report(&mut bridge, 1200, 1300, 1215);
    assert_eq!(
        value["data"]["rows"],
        json!([
            {"taskId": finished.id().to_string(), "durationMicroseconds": 5_000_000},
            {"taskId": active.id().to_string(), "durationMicroseconds": 5_000_000}
        ])
        .as_array()
        .map(|rows| {
            let mut rows = rows.clone();
            rows.sort_by_key(|row| row["taskId"].as_str().unwrap().to_owned());
            Value::Array(rows)
        })
        .unwrap()
    );
    let value = report(&mut bridge, 1200, 1212, 1220);
    assert!(value["data"]["rows"].as_array().unwrap().contains(&json!({
        "taskId": active.id().to_string(), "durationMicroseconds": 2_000_000
    })));
}

/// Checks that returned report cutoffs describe the precision used to calculate running time.
#[test]
fn local_reports_echo_the_microsecond_cutoff_used_for_totals() {
    let directory = tempfile::tempdir().unwrap();
    let mut bridge = open_fixture(&directory.path().join("precision.db")).unwrap();
    let task_id = bridge.application.tasks(TaskOrdering::default())[0]
        .task
        .id();
    bridge
        .application
        .set_active_task(task_id, at(1005))
        .unwrap();
    let now = DateTime::from_timestamp(1010, 123_456_789).unwrap();
    let expected_now = DateTime::from_timestamp(1010, 123_456_000).unwrap();

    let totals = bridge
        .application
        .refresh_totals(at(1000), at(1100), now)
        .unwrap();
    assert_eq!(totals.now, timestamp(expected_now));
    assert_eq!(totals.rows[0].duration_microseconds, 5_123_456);
}

#[test]
fn report_preserves_microseconds_and_accepts_utc_offsets() {
    let repository = SqliteRepository::open_in_memory().unwrap();
    let mut application = TrackerApplication::load(repository).unwrap();
    let task = application
        .create_task(TaskName::new("Precise work").unwrap(), at(0))
        .unwrap();
    application
        .set_active_task(task.id(), "2026-10-04T00:00:00.123456Z".parse().unwrap())
        .unwrap();
    let TrackingState::Running { worklog } = application.current_tracking() else {
        panic!("tracking should be running");
    };
    application
        .clear_active_task(worklog.id(), "2026-10-04T00:00:00.987654Z".parse().unwrap())
        .unwrap();
    let mut bridge = Bridge::new(Backend::local(application).unwrap());
    let start = c"2026-10-04T02:00:00+02:00";
    let end = c"2026-10-05T02:00:00+02:00";
    let now = c"2026-10-04T00:00:00.500000Z";
    // SAFETY: The bridge and all strings remain live for this call.
    let value = unsafe { report_at(&mut bridge, start.as_ptr(), end.as_ptr(), now.as_ptr()) };
    assert_eq!(
        value["data"]["rows"],
        json!([
            {"taskId": task.id().to_string(), "durationMicroseconds": 864_198}
        ])
    );
}

#[test]
fn report_rejects_null_malformed_and_invalid_ranges_and_returns_empty_totals() {
    let repository = SqliteRepository::open_in_memory().unwrap();
    let mut bridge =
        Bridge::new(Backend::local(TrackerApplication::load(repository).unwrap()).unwrap());
    let valid = c"2026-10-04T00:00:00Z";
    let malformed = c"invalid";
    let non_utf8 = CString::new(vec![0xff]).unwrap();
    // SAFETY: All handles and strings are live or null, as the API supports.
    unsafe {
        assert_eq!(
            report_at(ptr::null_mut(), ptr::null(), ptr::null(), ptr::null())["error"],
            "Database is not open"
        );
        for invalid in [ptr::null(), malformed.as_ptr(), non_utf8.as_ptr()] {
            assert_eq!(
                report_at(&mut bridge, invalid, valid.as_ptr(), valid.as_ptr())["error"],
                "Invalid report start"
            );
            assert_eq!(
                report_at(&mut bridge, valid.as_ptr(), invalid, valid.as_ptr())["error"],
                "Invalid report end"
            );
            assert_eq!(
                report_at(&mut bridge, valid.as_ptr(), valid.as_ptr(), invalid)["error"],
                "Invalid report timestamp"
            );
        }
    }
    for (start, end) in [(1000, 1000), (1001, 1000)] {
        let value = report(&mut bridge, start, end, 1000);
        assert_eq!(value["error"], "Report end must be later than start");
        assert_eq!(value["uncertain"], false);
        assert!(value.get("data").is_none());
    }
    let value = report(&mut bridge, 1000, 1100, 1050);
    assert_eq!(value["data"], json!({"rows":[], "now":timestamp(at(1050))}));
}

#[test]
fn remote_report_uses_server_totals_and_composes_task_and_tracking_resources() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server.db");
    let server = Server::start(tracker_server::router_for_database(&path).unwrap());
    let mut bridge = server.client();
    let mut writer = open_fixture(&path).unwrap();
    let tasks = writer.application.tasks(TaskOrdering::default());
    let task_id = tasks[0].task.id();
    writer
        .application
        .set_active_task(task_id, at(900))
        .unwrap_or_else(|error| panic!("{}", error.message));

    let value = report(&mut bridge, 1000, 1100, 1050);
    assert_eq!(
        value["data"]["rows"],
        json!([
            {"taskId": task_id.to_string(), "durationMicroseconds": 50_000_000}
        ])
    );
    assert!(value["data"].get("revision").is_none(), "{value}");
    assert_eq!(
        crate::tests::test_resource_values(&bridge.application),
        crate::tests::test_resource_values(&writer.application)
    );
    bridge
        .application
        .set_active_task(tasks[1].task.id(), at(1060))
        .unwrap_or_else(|error| panic!("{}", error.message));
    assert!(
        matches!(bridge.application.current_tracking(), TrackingState::Running { worklog } if worklog.task_id() == tasks[1].task.id())
    );
}

#[test]
fn remote_selected_report_reads_one_daily_overview_without_reconciliation() {
    use axum::{Json, routing::get};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tracker_protocol::{DailyOverviewDto, ReportDto, ReportRowDto};
    let mut application =
        TrackerApplication::load(SqliteRepository::open_in_memory().unwrap()).unwrap();
    let task = application
        .create_task(TaskName::new("Measured task").unwrap(), at(0))
        .unwrap();
    let resources = resource_dtos(application.tasks(TaskOrdering::default()), None);
    let requests = Arc::new(AtomicUsize::new(0));
    let read_requests = requests.clone();
    let router = resource_router(resources.clone(), vec![])
        .merge(healthy_router())
        .route(
            "/v1/state/daily-overview",
            get(move || {
                read_requests.fetch_add(1, Ordering::SeqCst);
                let dto = DailyOverviewDto {
                    tasks: resources.0.tasks.clone(),
                    active_worklog: None,
                    totals: ReportDto {
                        start: at(1000),
                        end: at(1100),
                        now: at(1050),
                        rows: vec![ReportRowDto {
                            task_id: task.id().to_string(),
                            duration_us: 50_000_000,
                        }],
                        total_us: 50_000_000,
                    },
                };
                async move { Json(dto) }
            }),
        );
    let server = Server::start(router);
    let mut bridge = server.client();
    let value = report(&mut bridge, 1000, 1100, 1050);
    assert!(value.get("error").is_none(), "{value}");
    assert!(value["data"].get("revision").is_none(), "{value}");
    assert!(tracking_json(&bridge.application).unwrap().value.is_none());
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    assert_eq!(value["data"]["rows"][0]["durationMicroseconds"], 50_000_000);
}

#[test]
fn remote_report_connection_failure_returns_a_certain_read_error_and_blocks_writes() {
    let directory = tempfile::tempdir().unwrap();
    let mut server = Server::start(
        tracker_server::router_for_database(&directory.path().join("server.db")).unwrap(),
    );
    let mut bridge = server.client();
    assert!(report(&mut bridge, 1000, 1100, 1050).get("error").is_none());
    server.stop();

    let value = report(&mut bridge, 1000, 1100, 1050);
    assert_eq!(value["kind"], "unavailable");
    assert_eq!(value["requiresRefresh"], true);
    assert_eq!(value["uncertain"], false);
    assert!(value.get("data").is_none());
    let error = bridge
        .application
        .set_active_task(TaskId::generate(), at(1060))
        .err()
        .unwrap();
    assert!(!error.uncertain);
    assert!(error.requires_refresh);
}

#[test]
fn connection_check_uses_only_health_and_rejects_incompatible_servers() {
    use axum::{Json, Router, routing::get};
    for version in [tracker_protocol::VERSION, 2] {
        let server = Server::start(Router::new().route(
            "/v1/health",
            get(move || async move {
                Json(tracker_protocol::HealthDto {
                    status: "ok".into(),
                    protocol_version: version,
                })
            }),
        ));
        let mut bridge = server.client();
        // SAFETY: The bridge is live and uniquely accessed.
        let value = response(unsafe { tt_bridge_check_connection(&mut bridge) });
        if version == tracker_protocol::VERSION {
            assert!(value.get("error").is_none(), "{value}");
            assert!(bridge.application.tasks(TaskOrdering::default()).is_empty());
            assert!(matches!(
                bridge.application.current_tracking(),
                TrackingState::Idle
            ));
        } else {
            assert_eq!(value["kind"], "protocol");
            assert_eq!(value["uncertain"], false);
        }
    }
}

#[test]
fn connection_check_supports_local_handles_and_rejects_null_handles() {
    let application =
        TrackerApplication::load(SqliteRepository::open_in_memory().unwrap()).unwrap();
    let mut bridge = Bridge::new(Backend::local(application).unwrap());
    // SAFETY: A live bridge and a null pointer are both supported inputs.
    unsafe {
        let value = response(tt_bridge_check_connection(&mut bridge));
        assert!(value.get("error").is_none(), "{value}");
        assert_eq!(
            response(tt_bridge_check_connection(std::ptr::null_mut()))["error"],
            "Database is not open"
        );
    }
}

#[test]
fn resource_exports_distinguish_unloaded_empty_and_idle_observations() {
    let server =
        Server::start(resource_router(resource_dtos(vec![], None), vec![]).merge(healthy_router()));
    let mut bridge = server.client();
    // SAFETY: Each call has exclusive access to the live bridge.
    unsafe {
        assert!(response(tt_bridge_tasks(&mut bridge))["data"].is_null());
        assert!(response(tt_bridge_tracking(&mut bridge))["data"].is_null());
        assert!(response(tt_bridge_report_observation(&mut bridge))["data"].is_null());
        assert!(
            response(tt_bridge_refresh_resources(&mut bridge))
                .get("error")
                .is_none()
        );
        assert_eq!(
            response(tt_bridge_tasks(&mut bridge))["data"],
            json!({"value":[]})
        );
        assert_eq!(
            response(tt_bridge_tracking(&mut bridge))["data"],
            json!({"value":null})
        );
        assert!(response(tt_bridge_report_observation(&mut bridge))["data"].is_null());
    }
}

#[test]
fn failed_aggregate_refresh_retains_each_confirmed_resource_observation() {
    use axum::{Json, http::StatusCode, response::IntoResponse, routing::get};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use tracker_protocol::{ErrorCode, ErrorDto, TaskListStateDto};
    let failing = Arc::new(AtomicBool::new(false));
    let fail_reads = failing.clone();
    let server = Server::start(healthy_router().route(
        "/v1/state/task-list",
        get(move || {
            let fail = fail_reads.load(Ordering::SeqCst);
            async move {
                if fail {
                    (
                        StatusCode::CONFLICT,
                        Json(ErrorDto {
                            code: ErrorCode::Conflict,
                            message: "Task list changed".into(),
                        }),
                    )
                        .into_response()
                } else {
                    Json(TaskListStateDto {
                        tasks: vec![],
                        active_worklog: None,
                    })
                    .into_response()
                }
            }
        }),
    ));
    let mut bridge = server.client();
    // SAFETY: Each call has exclusive access to the live bridge.
    unsafe {
        assert!(
            response(tt_bridge_refresh_resources(&mut bridge))
                .get("error")
                .is_none()
        );
        let tasks = response(tt_bridge_tasks(&mut bridge));
        let tracking = response(tt_bridge_tracking(&mut bridge));
        failing.store(true, Ordering::SeqCst);

        let failure = response(tt_bridge_refresh_resources(&mut bridge));

        assert_eq!(failure["kind"], "conflict");
        assert_eq!(failure["requiresRefresh"], true);
        assert_eq!(response(tt_bridge_tasks(&mut bridge)), tasks);
        assert_eq!(response(tt_bridge_tracking(&mut bridge)), tracking);
    }
}

#[test]
fn resource_exports_and_refreshes_reject_null_handles() {
    // SAFETY: Null handles are supported inputs.
    unsafe {
        for returned in [
            tt_bridge_tasks(ptr::null_mut()),
            tt_bridge_tracking(ptr::null_mut()),
            tt_bridge_report_observation(ptr::null_mut()),
            tt_bridge_refresh_resources(ptr::null_mut()),
            tt_bridge_refresh_totals(ptr::null_mut(), ptr::null(), ptr::null(), ptr::null()),
        ] {
            assert_eq!(response(returned)["error"], "Database is not open");
        }
    }
}

#[test]
fn local_totals_refresh_adopts_the_task_list_and_preserves_failed_reads() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("selected-local.db");
    let mut bridge = open_fixture(&path).unwrap();
    let mut writer = open_fixture(&path).unwrap();
    let original = writer.application.tasks(TaskOrdering::default());
    writer
        .application
        .create_task(TaskName::new("Unrelated task").unwrap(), at(0))
        .unwrap();
    writer
        .application
        .set_active_task(original[0].task.id(), at(900))
        .unwrap();

    let totals = report(&mut bridge, 1000, 1100, 1050);
    assert_eq!(
        totals["data"]["rows"],
        json!([
            {"taskId": original[0].task.id().to_string(), "durationMicroseconds": 50_000_000}
        ])
    );
    assert!(totals["data"].get("revision").is_none(), "{totals}");
    assert_eq!(
        crate::tests::test_resource_values(&bridge.application),
        crate::tests::test_resource_values(&writer.application)
    );
    let confirmed_resources = crate::tests::test_resource_values(&bridge.application);
    let confirmed_report = serde_json::to_value(&bridge.report_observation).unwrap();
    let failure = report(&mut bridge, 1100, 1000, 1050);
    assert_eq!(failure["error"], "Report end must be later than start");
    assert_eq!(failure["uncertain"], false);
    assert_eq!(
        crate::tests::test_resource_values(&bridge.application),
        confirmed_resources
    );
    assert_eq!(
        serde_json::to_value(&bridge.report_observation).unwrap(),
        confirmed_report
    );
}

#[test]
fn remote_totals_refresh_loads_the_task_list_and_retains_it_after_failure() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("selected-remote.db");
    let mut writer = open_fixture(&path).unwrap();
    let tasks = writer.application.tasks(TaskOrdering::default());
    writer
        .application
        .set_active_task(tasks[0].task.id(), at(900))
        .unwrap();
    let mut server = Server::start(tracker_server::router_for_database(&path).unwrap());
    let mut bridge = server.client();

    let totals = report(&mut bridge, 1000, 1100, 1050);
    assert!(totals.get("error").is_none(), "{totals}");
    assert_eq!(
        totals["data"]["rows"][0]["durationMicroseconds"],
        50_000_000
    );
    assert_eq!(
        crate::tests::test_resource_values(&bridge.application),
        crate::tests::test_resource_values(&writer.application)
    );
    let confirmed_tasks = serde_json::to_value(tasks_json(&bridge.application)).unwrap();
    let confirmed_tracking = serde_json::to_value(tracking_json(&bridge.application)).unwrap();
    let confirmed_report = serde_json::to_value(&bridge.report_observation).unwrap();
    server.stop();
    let failure = report(&mut bridge, 1000, 1100, 1050);
    assert_eq!(failure["kind"], "unavailable");
    assert_eq!(failure["uncertain"], false);
    assert_eq!(failure["requiresRefresh"], true);
    assert_eq!(
        serde_json::to_value(tasks_json(&bridge.application)).unwrap(),
        confirmed_tasks
    );
    assert_eq!(
        serde_json::to_value(tracking_json(&bridge.application)).unwrap(),
        confirmed_tracking
    );
    assert_eq!(
        serde_json::to_value(&bridge.report_observation).unwrap(),
        confirmed_report
    );
}

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode};
use chrono::{DateTime, TimeDelta, Utc};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;
use tracker_domain::{Task, TaskId, TaskName};
use tracker_protocol::{
    InactiveTaskPreviewDto, MutationDto, MutationResultDto, TaskChangeRequest, TasksDto,
    TrackingDto, WriteGuard,
};
use tracker_storage::SqliteRepository;
use uuid::Uuid;

fn at(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 0).unwrap()
}

/// SQLite and HTTP must reject the same range when truncation makes its bounds equal.
#[tokio::test]
async fn local_and_http_reports_reject_ranges_within_one_microsecond() {
    use tracker_application::{ApplicationError, TrackerApplication};

    let directory = TempDir::new().unwrap();
    let database = directory.path().join("precision.db");
    let mut application =
        TrackerApplication::load(SqliteRepository::open(&database).unwrap()).unwrap();
    let router = tracker_server::router_for_database(&database).unwrap();
    let start = DateTime::from_timestamp(100, 100).unwrap();
    let end = DateTime::from_timestamp(100, 900).unwrap();

    assert_eq!(
        application.report_totals(start, end, at(200)),
        Err(ApplicationError::InvalidReportRange)
    );
    assert_eq!(
        application.task_list_report_totals(start, end, at(200)),
        Err(ApplicationError::InvalidReportRange)
    );
    let (status, body) = call(
        &router,
        Method::GET,
        "/v1/reports/task-totals?start=1970-01-01T00:01:40.000000100Z&end=1970-01-01T00:01:40.000000900Z&now=1970-01-01T00:03:20Z",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "invalid_request");
}

/// Exercises handlers in process and decodes JSON errors as well as resource responses.
async fn call(
    router: &axum::Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(
            body.map_or_else(String::new, |value| value.to_string()),
        ))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    let body = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes).to_string()));
    (status, body)
}

fn guard() -> Value {
    json!({"request_id":Uuid::now_v7().to_string()})
}

fn merge(mut body: Value, guard: Value) -> Value {
    body.as_object_mut()
        .unwrap()
        .extend(guard.as_object().unwrap().clone());
    body
}

async fn state(router: &axum::Router) -> (TasksDto, TrackingDto) {
    let (status, body) = call(router, Method::GET, "/v1/tasks", None).await;
    assert_eq!(status, StatusCode::OK);
    let tasks: TasksDto = serde_json::from_value(body).unwrap();
    let (status, body) = call(router, Method::GET, "/v1/tracking", None).await;
    assert_eq!(status, StatusCode::OK);
    let tracking: TrackingDto = serde_json::from_value(body).unwrap();
    (tasks, tracking)
}

async fn create_fixture_task(router: &axum::Router, name: &str) -> String {
    let id = Uuid::now_v7().to_string();
    let request = merge(
        json!({"task_id": id, "name": name, "occurred_at": at(0)}),
        guard(),
    );
    let (status, body) = call(router, Method::POST, "/v1/tasks", Some(request)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    id
}

fn preview_uri(as_of: DateTime<Utc>) -> String {
    format!(
        "/v1/tasks/inactive-preview?as_of={}",
        as_of.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    )
}

fn archive_request(preview: &InactiveTaskPreviewDto) -> Value {
    merge(
        json!({
            "as_of": preview.as_of,
            "inactive_days": preview.inactive_days,
        }),
        guard(),
    )
}

#[tokio::test]
async fn inactive_preview_archives_once_and_replays_the_same_result() {
    let directory = TempDir::new().unwrap();
    let router = tracker_server::router_for_database(&directory.path().join("tracker.db")).unwrap();
    let as_of = Utc::now();
    let created_at = as_of - TimeDelta::days(20);
    let create = merge(
        json!({
            "task_id": Uuid::now_v7().to_string(),
            "name": "Old planning task",
            "occurred_at": created_at,
        }),
        guard(),
    );
    let (status, body) = call(&router, Method::POST, "/v1/tasks", Some(create)).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = call(&router, Method::GET, &preview_uri(as_of), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_keys(
        &body,
        &["as_of", "inactive_days", "count", "candidate_task_ids"],
    );
    let preview: InactiveTaskPreviewDto = serde_json::from_value(body).unwrap();
    assert_eq!(preview.count, 1);
    assert_eq!(preview.inactive_days, 14);
    assert_eq!(preview.candidate_task_ids.len(), 1);

    let request = archive_request(&preview);
    assert_eq!(request.as_object().unwrap().len(), 3);
    assert!(Uuid::parse_str(request["request_id"].as_str().unwrap()).is_ok());
    let (status, first) = call(
        &router,
        Method::POST,
        "/v1/tasks/archive-inactive",
        Some(request.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let mutation: MutationDto = serde_json::from_value(first.clone()).unwrap();
    assert_eq!(
        mutation.result,
        MutationResultDto::ArchivedInactive { count: 1 }
    );
    assert!(
        state(&router)
            .await
            .0
            .tasks
            .iter()
            .any(|item| { item.name == "Old planning task" && item.archived })
    );
    let (status, replay) = call(
        &router,
        Method::POST,
        "/v1/tasks/archive-inactive",
        Some(request.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay["result"], first["result"]);
    assert_eq!(replay["replayed"], true);

    let mut changed_request = request.clone();
    changed_request["as_of"] = json!(as_of + TimeDelta::seconds(1));
    let (status, body) = call(
        &router,
        Method::POST,
        "/v1/tasks/archive-inactive",
        Some(changed_request),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "conflict");
    assert!(state(&router).await.0.tasks[0].archived);

    let archived_id = &state(&router).await.0.tasks[0].id;
    let restore = merge(json!({"action": "restore", "occurred_at": as_of}), guard());
    let (status, body) = call(
        &router,
        Method::PATCH,
        &format!("/v1/tasks/{archived_id}"),
        Some(restore),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let restored: MutationDto = serde_json::from_value(body).unwrap();
    assert!(matches!(restored.result, MutationResultDto::Task(ref task) if !task.archived));
    assert!(!state(&router).await.0.tasks[0].archived);

    let (status, body) = call(
        &router,
        Method::POST,
        "/v1/tasks/archive-inactive",
        Some(request),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let replay: MutationDto = serde_json::from_value(body).unwrap();
    assert_eq!(replay.result, mutation.result);
    assert!(replay.replayed);
    assert!(!state(&router).await.0.tasks[0].archived);
}

#[tokio::test]
async fn inactive_preview_returns_all_candidate_ids() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("tracker.db");
    let router = tracker_server::router_for_database(&database).unwrap();
    let as_of = Utc::now();
    let outside = SqliteRepository::open(&database).unwrap();
    for index in 0..7 {
        outside
            .create_task(Task::create(
                TaskId::generate(),
                TaskName::new(&format!("Old candidate {index}")).unwrap(),
                as_of - TimeDelta::days(20),
            ))
            .unwrap();
    }
    let (status, body) = call(&router, Method::GET, &preview_uri(as_of), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let preview: InactiveTaskPreviewDto = serde_json::from_value(body).unwrap();
    assert_eq!(preview.count, 7);
    assert_eq!(preview.candidate_task_ids.len(), 7);
}

#[tokio::test]
async fn inactive_archive_uses_current_candidates_when_database_changes_after_preview() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("tracker.db");
    let router = tracker_server::router_for_database(&database).unwrap();
    let as_of = Utc::now();
    let outside = SqliteRepository::open(&database).unwrap();
    let changed_id = TaskId::generate();
    outside
        .create_task(Task::create(
            changed_id,
            TaskName::new("Previously inactive").unwrap(),
            as_of - TimeDelta::days(20),
        ))
        .unwrap();
    let (status, body) = call(&router, Method::GET, &preview_uri(as_of), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let preview: InactiveTaskPreviewDto = serde_json::from_value(body).unwrap();
    assert_eq!(preview.count, 1);

    outside
        .rename_task(
            changed_id,
            TaskName::new("Recently changed").unwrap(),
            as_of,
        )
        .unwrap();
    let candidate_ids = [TaskId::generate(), TaskId::generate()];
    for (index, id) in candidate_ids.iter().enumerate() {
        outside
            .create_task(Task::create(
                *id,
                TaskName::new(&format!("New inactive candidate {index}")).unwrap(),
                as_of - TimeDelta::days(20),
            ))
            .unwrap();
    }
    let (status, body) = call(
        &router,
        Method::POST,
        "/v1/tasks/archive-inactive",
        Some(archive_request(&preview)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let result: MutationDto = serde_json::from_value(body).unwrap();
    assert_eq!(
        result.result,
        MutationResultDto::ArchivedInactive { count: 2 }
    );
    assert_eq!(state(&router).await.0.tasks.len(), 3);
    for id in candidate_ids {
        let task = outside.find_task(id).unwrap().unwrap();
        assert!(task.is_archived());
        assert!(
            state(&router)
                .await
                .0
                .tasks
                .iter()
                .any(|item| { item.id == id.to_string() && item.archived })
        );
    }
    let changed = outside.find_task(changed_id).unwrap().unwrap();
    assert!(!changed.is_archived());
    assert_eq!(changed.name().as_str(), "Recently changed");
    assert!(state(&router).await.0.tasks.iter().any(|item| {
        item.id == changed_id.to_string() && item.name == "Recently changed" && !item.archived
    }));
}

#[tokio::test]
async fn inactive_archive_rejects_out_of_range_time_and_accepts_a_preview_from_before_another_write()
 {
    let directory = TempDir::new().unwrap();
    let router = tracker_server::router_for_database(&directory.path().join("tracker.db")).unwrap();
    let now = Utc::now();
    for as_of in [now - TimeDelta::hours(1), now + TimeDelta::hours(1)] {
        let (status, body) = call(&router, Method::GET, &preview_uri(as_of), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }

    let (status, body) = call(&router, Method::GET, &preview_uri(now), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let preview: InactiveTaskPreviewDto = serde_json::from_value(body).unwrap();
    let mut future_request = archive_request(&preview);
    future_request["as_of"] = json!(now + TimeDelta::hours(1));
    let (status, body) = call(
        &router,
        Method::POST,
        "/v1/tasks/archive-inactive",
        Some(future_request),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let create = merge(
        json!({
            "task_id": Uuid::now_v7().to_string(),
            "name": "New task",
            "occurred_at": now,
        }),
        guard(),
    );
    let (status, body) = call(&router, Method::POST, "/v1/tasks", Some(create)).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = call(
        &router,
        Method::POST,
        "/v1/tasks/archive-inactive",
        Some(archive_request(&preview)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let mutation: MutationDto = serde_json::from_value(body).unwrap();
    assert_eq!(
        mutation.result,
        MutationResultDto::ArchivedInactive { count: 0 }
    );
    assert!(!state(&router).await.0.tasks[0].archived);
}

#[tokio::test]
async fn health_create_retry_and_create_after_another_write() {
    let temp = TempDir::new().unwrap();
    let router = tracker_server::router_for_database(&temp.path().join("tracker.db")).unwrap();
    let (status, health) = call(&router, Method::GET, "/v1/health", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(health, json!({"status":"ok","protocol_version":6}));
    let before = state(&router).await;
    assert!(before.0.tasks.is_empty());
    let task_id = Uuid::now_v7().to_string();
    let request = merge(
        json!({"task_id":task_id,"name":"Write tests","occurred_at":at(100)}),
        guard(),
    );
    let (status, first) = call(&router, Method::POST, "/v1/tasks", Some(request.clone())).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let first: MutationDto = serde_json::from_value(first.clone()).unwrap();
    assert_eq!(state(&router).await.0.tasks.len(), 1);
    let (status, replay) = call(&router, Method::POST, "/v1/tasks", Some(request.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        replay["result"],
        serde_json::to_value(&first.result).unwrap()
    );
    assert_eq!(replay["replayed"], true);
    let mut different = request.clone();
    different["name"] = json!("Different");
    let (status, error) = call(&router, Method::POST, "/v1/tasks", Some(different)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["code"], "conflict");
    // A create still succeeds after another create changed the catalog.
    let later = merge(
        json!({"task_id":Uuid::now_v7().to_string(),"name":"Later","occurred_at":at(101)}),
        guard(),
    );
    let (status, body) = call(&router, Method::POST, "/v1/tasks", Some(later)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let after = state(&router).await;
    assert_eq!(after.0.tasks.len(), 2);
}

#[tokio::test]
async fn tracking_survives_restart_and_worklogs_support_edits() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("tracker.db");
    let router = tracker_server::router_for_database(&path).unwrap();
    let first_task = create_fixture_task(&router, "First project").await;
    let second_task = create_fixture_task(&router, "Second project").await;
    let first_id = Uuid::now_v7().to_string();
    let set = merge(
        json!({"task_id":first_task,"worklog_id":first_id,"expected_active":null,"occurred_at":at(100)}),
        guard(),
    );
    let (status, body) = call(&router, Method::PUT, "/v1/tracking", Some(set)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let started: MutationDto = serde_json::from_value(body).unwrap();
    assert!(
        matches!(&started.result, MutationResultDto::Worklog(worklog) if worklog.id == first_id)
    );
    assert_eq!(
        state(&router).await.1.active_worklog.as_ref().unwrap().id,
        first_id
    );
    drop(router);

    let router = tracker_server::router_for_database(&path).unwrap();
    let resumed = state(&router).await;
    assert_eq!(resumed.1.active_worklog.as_ref().unwrap().id, first_id);
    let switch_id = Uuid::now_v7().to_string();
    let switch = merge(
        json!({"task_id":second_task,"worklog_id":switch_id,"expected_active":first_id,"occurred_at":at(150)}),
        guard(),
    );
    let (status, body) = call(&router, Method::PUT, "/v1/tracking", Some(switch)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        state(&router).await.1.active_worklog.as_ref().unwrap().id,
        switch_id
    );
    let clear = merge(
        json!({"task_id":null,"worklog_id":null,"expected_active":switch_id,"occurred_at":at(200)}),
        guard(),
    );
    let (status, body) = call(&router, Method::PUT, "/v1/tracking", Some(clear)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(state(&router).await.1.active_worklog.is_none());

    let (status, body) = call(&router, Method::GET, "/v1/worklogs", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["worklogs"].as_array().unwrap().len(), 2);
    let route = format!("/v1/worklogs?task_id={first_task}");
    let (status, body) = call(&router, Method::GET, &route, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["worklogs"].as_array().unwrap().len(), 1);

    let correction = merge(
        json!({"action":"correct","expected_start":at(100),"expected_end":at(150),"replacement_start":at(90),"replacement_end":at(140),"occurred_at":at(210)}),
        guard(),
    );
    let route = format!("/v1/worklogs/{first_id}");
    let (status, body) = call(&router, Method::PATCH, &route, Some(correction)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let movement = merge(
        json!({"action":"move","expected_task_id":first_task,"expected_start":at(90),"expected_end":at(140),"destination_task_id":second_task}),
        guard(),
    );
    let (status, body) = call(&router, Method::PATCH, &route, Some(movement)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let report_uri = format!(
        "/v1/reports/task-totals?start={}&end={}&now={}",
        at(0).to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        at(300).to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        at(300).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    );
    let (status, body) = call(&router, Method::GET, &report_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total_us"], 100_000_000);
    let deletion = merge(
        json!({"expected_task_id":second_task,"expected_start":at(90),"expected_end":at(140)}),
        guard(),
    );
    let (status, body) = call(&router, Method::DELETE, &route, Some(deletion)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let deleted: MutationDto = serde_json::from_value(body).unwrap();
    assert!(
        matches!(&deleted.result, MutationResultDto::Worklog(worklog) if worklog.id == first_id && worklog.task_id == second_task)
    );
    assert!(state(&router).await.1.active_worklog.is_none());
    let (status, body) = call(&router, Method::GET, "/v1/worklogs", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["worklogs"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn task_changes_and_request_limit() {
    let temp = TempDir::new().unwrap();
    let router = tracker_server::router_for_database(&temp.path().join("tracker.db")).unwrap();
    let id = create_fixture_task(&router, "Change this task").await;
    let route = format!("/v1/tasks/{id}");
    let rename = merge(
        json!({"action":"rename","expected_name":"Change this task","name":"Renamed task","occurred_at":at(100)}),
        guard(),
    );
    let (status, body) = call(&router, Method::PATCH, &route, Some(rename)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let archive = merge(
        json!({"action":"archive","expected_name":"Renamed task","occurred_at":at(110)}),
        guard(),
    );
    let (status, body) = call(&router, Method::PATCH, &route, Some(archive)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        state(&router)
            .await
            .0
            .tasks
            .iter()
            .find(|item| item.id == id)
            .unwrap()
            .archived
    );
    let restore = merge(json!({"action":"restore","occurred_at":at(120)}), guard());
    let (status, body) = call(&router, Method::PATCH, &route, Some(restore)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        !state(&router)
            .await
            .0
            .tasks
            .iter()
            .find(|item| item.id == id)
            .unwrap()
            .archived
    );
    let large = json!({"task_id":Uuid::now_v7().to_string(),"name":"x".repeat(17_000),"occurred_at":at(100),"request_id":Uuid::now_v7().to_string()});
    let (status, _) = call(&router, Method::POST, "/v1/tasks", Some(large)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);

    let between_mutated_and_real_limit = json!({
        "task_id": Uuid::now_v7().to_string(),
        "name": "Below the server body limit",
        "occurred_at": at(130),
        "unexpected": "x".repeat(2_000),
        "request_id": Uuid::now_v7().to_string(),
    });
    let (status, body) = call(
        &router,
        Method::POST,
        "/v1/tasks",
        Some(between_mutated_and_real_limit),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "invalid_request");
}

#[tokio::test]
async fn rename_and_archive_reject_a_name_another_client_changed_after_review() {
    let temp = TempDir::new().unwrap();
    let router = tracker_server::router_for_database(&temp.path().join("tracker.db")).unwrap();
    let id = create_fixture_task(&router, "Reviewed task").await;
    let route = format!("/v1/tasks/{id}");
    let other_client = merge(
        json!({"action":"rename","expected_name":"Reviewed task","name":"Other client task","occurred_at":at(100)}),
        guard(),
    );
    let (status, body) = call(&router, Method::PATCH, &route, Some(other_client)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let current = state(&router).await;
    // Writes carry no revision, so only the expected name can reject these changes.
    let stale_changes = [
        json!({"action":"rename","expected_name":"Reviewed task","name":"Stale rename","occurred_at":at(110)}),
        json!({"action":"archive","expected_name":"Reviewed task","occurred_at":at(120)}),
    ];
    for change in stale_changes {
        let request = merge(change, guard());
        let (status, body) = call(&router, Method::PATCH, &route, Some(request)).await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(body["code"], "conflict");
        assert_eq!(state(&router).await, current);
    }
    let task = current.0.tasks.iter().find(|item| item.id == id).unwrap();
    assert_eq!(task.name, "Other client task");
    assert!(!task.archived);
}

#[tokio::test]
async fn rename_and_archive_without_an_expected_name_apply_to_the_current_name() {
    let temp = TempDir::new().unwrap();
    let router = tracker_server::router_for_database(&temp.path().join("tracker.db")).unwrap();
    let id = create_fixture_task(&router, "Reviewed task").await;
    let route = format!("/v1/tasks/{id}");
    let other_client = merge(
        json!({"action":"rename","expected_name":"Reviewed task","name":"Other client task","occurred_at":at(100)}),
        guard(),
    );
    let (status, body) = call(&router, Method::PATCH, &route, Some(other_client)).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let rename = merge(
        json!({"action":"rename","name":"Unguarded rename","occurred_at":at(110)}),
        guard(),
    );
    let (status, body) = call(&router, Method::PATCH, &route, Some(rename)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["result"]["value"]["name"], "Unguarded rename");
    assert_eq!(body["result"]["value"]["archived"], false);

    let archive = merge(json!({"action":"archive","occurred_at":at(120)}), guard());
    let (status, body) = call(&router, Method::PATCH, &route, Some(archive)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["result"]["value"]["name"], "Unguarded rename");
    assert_eq!(body["result"]["value"]["archived"], true);
    let tasks = state(&router).await.0.tasks;
    let task = tasks.iter().find(|item| item.id == id).unwrap();
    assert_eq!(task.name, "Unguarded rename");
    assert!(task.archived);
}

#[tokio::test]
async fn rename_and_archive_reject_invalid_names_and_archiving_the_active_task() {
    let temp = TempDir::new().unwrap();
    let router = tracker_server::router_for_database(&temp.path().join("tracker.db")).unwrap();
    let id = create_fixture_task(&router, "Reviewed task").await;
    let route = format!("/v1/tasks/{id}");
    let before = state(&router).await;
    let invalid_changes = [
        json!({"action":"rename","name":"   ","occurred_at":at(100)}),
        json!({"action":"rename","expected_name":"   ","name":"Renamed task","occurred_at":at(100)}),
        json!({"action":"archive","expected_name":"   ","occurred_at":at(100)}),
    ];
    for change in invalid_changes {
        let request = merge(change, guard());
        let (status, body) = call(&router, Method::PATCH, &route, Some(request)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["code"], "invalid_request");
        assert_eq!(state(&router).await, before);
    }

    let start = merge(
        json!({"task_id":id,"worklog_id":Uuid::now_v7().to_string(),"expected_active":null,"occurred_at":at(110)}),
        guard(),
    );
    let (status, body) = call(&router, Method::PUT, "/v1/tracking", Some(start)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let tracking = state(&router).await;
    let archive = merge(json!({"action":"archive","occurred_at":at(120)}), guard());
    let (status, body) = call(&router, Method::PATCH, &route, Some(archive)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "active_task");
    assert_eq!(state(&router).await, tracking);
}

#[test]
fn task_change_requests_send_expected_name_only_when_present() {
    let guard = WriteGuard {
        request_id: "00000000-0000-7000-8000-000000000001".to_owned(),
    };
    let unguarded = TaskChangeRequest::Archive {
        expected_name: None,
        occurred_at: at(100),
        guard: guard.clone(),
    };
    let guarded = TaskChangeRequest::Archive {
        expected_name: Some("Reviewed task".to_owned()),
        occurred_at: at(100),
        guard,
    };

    assert_eq!(
        serde_json::to_string(&unguarded).unwrap(),
        r#"{"action":"archive","occurred_at":"1970-01-01T00:01:40Z","request_id":"00000000-0000-7000-8000-000000000001"}"#
    );
    assert_eq!(
        serde_json::to_string(&guarded).unwrap(),
        r#"{"action":"archive","expected_name":"Reviewed task","occurred_at":"1970-01-01T00:01:40Z","request_id":"00000000-0000-7000-8000-000000000001"}"#
    );
    assert_eq!(
        serde_json::from_str::<TaskChangeRequest>(
            r#"{"action":"rename","name":"Renamed task","occurred_at":"1970-01-01T00:01:40Z","request_id":"00000000-0000-7000-8000-000000000001"}"#
        )
        .unwrap(),
        TaskChangeRequest::Rename {
            expected_name: None,
            name: "Renamed task".to_owned(),
            occurred_at: at(100),
            guard: WriteGuard {
                request_id: "00000000-0000-7000-8000-000000000001".to_owned(),
            },
        }
    );
}

#[tokio::test]
async fn concurrent_unrelated_commands_both_apply_and_a_duplicate_server_is_rejected() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("tracker.db");
    let router = tracker_server::router_for_database(&path).unwrap();
    assert!(tracker_server::router_for_database(&path).is_err());
    let first = merge(
        json!({"task_id":Uuid::now_v7().to_string(),"name":"First","occurred_at":at(100)}),
        guard(),
    );
    let second = merge(
        json!({"task_id":Uuid::now_v7().to_string(),"name":"Second","occurred_at":at(100)}),
        guard(),
    );
    let (left, right) = tokio::join!(
        call(&router, Method::POST, "/v1/tasks", Some(first)),
        call(&router, Method::POST, "/v1/tasks", Some(second))
    );
    assert_eq!(left.0, StatusCode::OK, "{}", left.1);
    assert_eq!(right.0, StatusCode::OK, "{}", right.1);
    assert_eq!(state(&router).await.0.tasks.len(), 2);
}

#[tokio::test]
async fn invalid_json_does_not_change_state() {
    let temp = TempDir::new().unwrap();
    let router = tracker_server::router_for_database(&temp.path().join("tracker.db")).unwrap();
    let before = state(&router).await;
    let unknown = merge(
        json!({"task_id":Uuid::now_v7().to_string(),"name":"Input","occurred_at":at(100),"unexpected":"value"}),
        guard(),
    );
    let (status, body) = call(&router, Method::POST, "/v1/tasks", Some(unknown)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid_request");
    assert_eq!(state(&router).await, before);
}

#[tokio::test]
async fn error_mapping_distinguishes_invalid_and_missing_tasks() {
    let temp = TempDir::new().unwrap();
    let router = tracker_server::router_for_database(&temp.path().join("tracker.db")).unwrap();
    let before = state(&router).await;

    let invalid_name = merge(
        json!({"task_id":Uuid::now_v7().to_string(),"name":"   ","occurred_at":at(100)}),
        guard(),
    );
    let (status, body) = call(&router, Method::POST, "/v1/tasks", Some(invalid_name)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid_request");
    assert_eq!(state(&router).await, before);

    let missing_id = Uuid::now_v7().to_string();
    let change = merge(
        json!({"action":"rename","expected_name":"Missing task","name":"Missing task","occurred_at":at(100)}),
        guard(),
    );
    let (status, body) = call(
        &router,
        Method::PATCH,
        &format!("/v1/tasks/{missing_id}"),
        Some(change),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "not_found");
    assert_eq!(state(&router).await, before);
}

#[tokio::test]
async fn worklog_page_routes_validate_complete_nonnegative_cursors() {
    let temp = TempDir::new().unwrap();
    let router = tracker_server::router_for_database(&temp.path().join("tracker.db")).unwrap();
    let task_id = create_fixture_task(&router, "Worklog project").await;
    let worklog_id = Uuid::now_v7().to_string();
    for suffix in [
        format!("after_id={worklog_id}"),
        "after_revision=0".to_owned(),
        format!("after_id={worklog_id}&after_revision=-1"),
        "after_id=invalid&after_revision=0".to_owned(),
        "after_start=2026-01-01T00:00:00Z".to_owned(),
        format!("after_task_id={task_id}"),
    ] {
        for prefix in [format!("task_id={task_id}&"), String::new()] {
            let route = format!("/v1/worklogs?{prefix}{suffix}");
            let (status, body) = call(&router, Method::GET, &route, None).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{route}: {body}");
            assert_eq!(body["code"], "invalid_request");
        }
    }
    for route in [
        format!("/v1/worklogs?task_id={task_id}&after_id={worklog_id}&after_revision=0"),
        format!("/v1/worklogs?after_id={worklog_id}&after_revision=0"),
    ] {
        let (status, body) = call(&router, Method::GET, &route, None).await;
        assert_eq!(status, StatusCode::CONFLICT, "{route}: {body}");
    }
}

#[tokio::test]
async fn stopping_an_already_idle_tracker_is_an_idempotent_success() {
    let temp = TempDir::new().unwrap();
    let router = tracker_server::router_for_database(&temp.path().join("tracker.db")).unwrap();
    let request = merge(
        json!({"task_id":null,"worklog_id":null,"expected_active":null,"occurred_at":at(100)}),
        guard(),
    );

    let (status, first) = call(&router, Method::PUT, "/v1/tracking", Some(request.clone())).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["result"]["kind"], "tracking_already_idle");
    assert!(first.get("snapshot").is_none());
    assert!(state(&router).await.1.active_worklog.is_none());

    let (status, retry) = call(&router, Method::PUT, "/v1/tracking", Some(request)).await;
    assert_eq!(status, StatusCode::OK, "{retry}");
    assert_eq!(retry["result"], first["result"]);
    assert_eq!(retry["replayed"], true);
    assert!(state(&router).await.0.tasks.is_empty());
}

#[tokio::test]
async fn a_new_server_database_stays_empty_after_restart() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("tracker.db");
    for _ in 0..2 {
        let router = tracker_server::router_for_database(&path).unwrap();
        let resources = state(&router).await;
        assert!(resources.0.tasks.is_empty());
        assert!(resources.1.active_worklog.is_none());
    }
    let repository = SqliteRepository::open(&path).unwrap();
    assert!(repository.list_tasks().unwrap().is_empty());
    assert!(repository.active_worklog().unwrap().is_none());
}

fn candidates_uri(as_of: DateTime<Utc>, days: u32) -> String {
    format!(
        "/v1/tasks/inactive-preview?as_of={}&inactive_days={days}",
        as_of.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    )
}

#[tokio::test]
async fn configurable_candidates_include_every_task_and_archive_the_chosen_period() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("tracker.db");
    let router = tracker_server::router_for_database(&database).unwrap();
    let outside = SqliteRepository::open(&database).unwrap();
    let as_of = DateTime::from_timestamp(Utc::now().timestamp(), 0).unwrap();
    for index in 0..8 {
        outside
            .create_task(Task::create(
                TaskId::generate(),
                TaskName::new(&format!("Planning task {index}")).unwrap(),
                as_of - TimeDelta::days(10 + index),
            ))
            .unwrap();
    }
    let running_id = TaskId::generate();
    outside
        .create_task(Task::create(
            running_id,
            TaskName::new("Running planning").unwrap(),
            as_of - TimeDelta::days(40),
        ))
        .unwrap();
    let worklog = tracker_domain::Worklog::begin(
        tracker_domain::WorklogId::generate(),
        running_id,
        as_of - TimeDelta::days(35),
    );
    outside.insert_worklog(&worklog).unwrap();
    let (status, body) = call(&router, Method::GET, &candidates_uri(as_of, 7), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_keys(
        &body,
        &["as_of", "inactive_days", "count", "candidate_task_ids"],
    );
    assert_eq!(body["inactive_days"], 7);
    let preview: InactiveTaskPreviewDto = serde_json::from_value(body).unwrap();
    assert_eq!(preview.as_of, as_of);
    assert_eq!(preview.candidate_task_ids.len(), 8);
    for id in &preview.candidate_task_ids {
        let task = outside.find_task(id.parse().unwrap()).unwrap().unwrap();
        assert!(!task.is_archived());
        assert!(task.created_at() < as_of - TimeDelta::days(7));
    }
    let (status, body) = call(&router, Method::GET, &candidates_uri(as_of, 30), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["candidate_task_ids"].as_array().unwrap().is_empty());
    let request = merge(
        json!({"as_of": preview.as_of, "inactive_days": preview.inactive_days}),
        guard(),
    );
    let (status, body) = call(
        &router,
        Method::POST,
        "/v1/tasks/archive-inactive",
        Some(request),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let mutation: MutationDto = serde_json::from_value(body).unwrap();
    assert_eq!(
        mutation.result,
        MutationResultDto::ArchivedInactive { count: 8 }
    );
    assert_eq!(
        state(&router).await.1.active_worklog.as_ref().unwrap().id,
        worklog.id().to_string()
    );
    assert_eq!(outside.active_worklog().unwrap(), Some(worklog));
    assert!(
        !outside
            .find_task(running_id)
            .unwrap()
            .unwrap()
            .is_archived()
    );
}

#[tokio::test]
async fn configurable_archive_recomputes_candidates_and_preserves_original_receipts() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("tracker.db");
    let router = tracker_server::router_for_database(&database).unwrap();
    let outside = SqliteRepository::open(&database).unwrap();
    let as_of = Utc::now();
    let old = Task::create(
        TaskId::generate(),
        TaskName::new("Old planning").unwrap(),
        as_of - TimeDelta::days(20),
    );
    outside.create_task(old.clone()).unwrap();
    let (status, body) = call(&router, Method::GET, &candidates_uri(as_of, 7), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let preview: InactiveTaskPreviewDto = serde_json::from_value(body).unwrap();
    assert_eq!(preview.candidate_task_ids.len(), 1);
    outside
        .rename_task(old.id(), TaskName::new("Updated planning").unwrap(), as_of)
        .unwrap();
    let added = Task::create(
        TaskId::generate(),
        TaskName::new("Added planning").unwrap(),
        as_of - TimeDelta::days(10),
    );
    outside.create_task(added.clone()).unwrap();
    let request = merge(
        json!({"as_of": preview.as_of, "inactive_days": preview.inactive_days}),
        guard(),
    );
    let (status, body) = call(
        &router,
        Method::POST,
        "/v1/tasks/archive-inactive",
        Some(request.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let first: MutationDto = serde_json::from_value(body).unwrap();
    assert_eq!(
        first.result,
        MutationResultDto::ArchivedInactive { count: 1 }
    );
    assert!(!outside.find_task(old.id()).unwrap().unwrap().is_archived());
    assert!(
        outside
            .find_task(added.id())
            .unwrap()
            .unwrap()
            .is_archived()
    );
    let restore = merge(json!({"action":"restore", "occurred_at": as_of}), guard());
    let (status, body) = call(
        &router,
        Method::PATCH,
        &format!("/v1/tasks/{}", added.id()),
        Some(restore),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = call(
        &router,
        Method::POST,
        "/v1/tasks/archive-inactive",
        Some(request.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let replay: MutationDto = serde_json::from_value(body).unwrap();
    assert_eq!(replay.result, first.result);
    assert!(replay.replayed);
    let restored = state(&router).await;
    assert!(
        restored
            .0
            .tasks
            .iter()
            .any(|item| item.id == added.id().to_string() && !item.archived)
    );
    let mut changed_request = request;
    changed_request["inactive_days"] = json!(30);
    let (status, body) = call(
        &router,
        Method::POST,
        "/v1/tasks/archive-inactive",
        Some(changed_request),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "conflict");
    assert_eq!(state(&router).await, restored);
}

#[tokio::test]
async fn configurable_archive_validates_days_and_time_without_writing() {
    let directory = TempDir::new().unwrap();
    let router = tracker_server::router_for_database(&directory.path().join("tracker.db")).unwrap();
    create_fixture_task(&router, "Planning").await;
    let before = state(&router).await;
    let as_of = Utc::now();
    for days in [0, u32::MAX] {
        let (status, _) = call(&router, Method::GET, &candidates_uri(as_of, days), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let request = merge(json!({"as_of": as_of, "inactive_days": days}), guard());
        let (status, body) = call(
            &router,
            Method::POST,
            "/v1/tasks/archive-inactive",
            Some(request),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["code"], "invalid_request");
        assert_eq!(state(&router).await, before);
    }
    for invalid_time in [
        as_of - TimeDelta::minutes(16),
        as_of + TimeDelta::minutes(16),
    ] {
        let (status, _) = call(&router, Method::GET, &candidates_uri(invalid_time, 7), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let request = merge(json!({"as_of": invalid_time, "inactive_days": 7}), guard());
        let (status, _) = call(
            &router,
            Method::POST,
            "/v1/tasks/archive-inactive",
            Some(request),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(state(&router).await, before);
    }
    for fields in [
        json!({"as_of":as_of}),
        json!({"as_of":as_of,"inactive_days":7,"unexpected":true}),
        json!({"as_of":as_of,"inactive_days":-1}),
    ] {
        let (status, _) = call(
            &router,
            Method::POST,
            "/v1/tasks/archive-inactive",
            Some(merge(fields, guard())),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(state(&router).await, before);
    }
}

fn assert_keys(value: &Value, expected: &[&str]) {
    let mut actual: Vec<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    actual.sort_unstable();
    let mut expected = expected.to_vec();
    expected.sort_unstable();
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn resource_reads_return_only_requested_data_and_include_task_activity() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("resources.db");
    let outside = SqliteRepository::open(&database).unwrap();
    let active_task = Task::create(
        TaskId::generate(),
        TaskName::new("Current project").unwrap(),
        at(1),
    );
    let mut archived_task = Task::create(
        TaskId::generate(),
        TaskName::new("Finished project").unwrap(),
        at(2),
    );
    archived_task.archive(at(30));
    outside.create_task(active_task.clone()).unwrap();
    outside.create_task(archived_task.clone()).unwrap();
    let running = tracker_domain::Worklog::begin(
        tracker_domain::WorklogId::generate(),
        active_task.id(),
        at(100),
    );
    outside.insert_worklog(&running).unwrap();
    let router = tracker_server::router_for_database(&database).unwrap();

    let (status, tasks) = call(&router, Method::GET, "/v1/tasks", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_keys(&tasks, &["tasks"]);
    assert_eq!(tasks["tasks"].as_array().unwrap().len(), 2);
    assert_eq!(tasks["tasks"][0]["id"], archived_task.id().to_string());
    assert_eq!(tasks["tasks"][1]["latest_work_start"], json!(at(100)));
    assert_keys(
        &tasks["tasks"][0],
        &[
            "id",
            "name",
            "archived",
            "created_at",
            "updated_at",
            "latest_work_start",
        ],
    );
    assert!(tasks["tasks"][0]["latest_work_start"].is_null());
    for (filter, id) in [("false", active_task.id()), ("true", archived_task.id())] {
        let (status, filtered) = call(
            &router,
            Method::GET,
            &format!("/v1/tasks?archived={filter}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(filtered["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(filtered["tasks"][0]["id"], id.to_string());
    }
    for (index, id) in [(0, archived_task.id()), (1, active_task.id())] {
        let (status, task) = call(&router, Method::GET, &format!("/v1/tasks/{id}"), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_keys(&task, &["task"]);
        assert_eq!(task["task"], tasks["tasks"][index]);
    }
    let (status, tracking) = call(&router, Method::GET, "/v1/tracking", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_keys(&tracking, &["active_worklog"]);
    assert_eq!(tracking["active_worklog"]["id"], running.id().to_string());
    let (status, worklog) = call(
        &router,
        Method::GET,
        &format!("/v1/worklogs/{}", running.id()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_keys(&worklog, &["worklog"]);
    assert_keys(&worklog["worklog"], &["id", "task_id", "start", "end"]);
    assert_eq!(worklog["worklog"], tracking["active_worklog"]);
    let (status, history) = call(
        &router,
        Method::GET,
        &format!("/v1/worklogs?task_id={}", active_task.id()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_keys(&history, &["worklogs", "next_cursor"]);
    assert_eq!(history["worklogs"][0], tracking["active_worklog"]);
}

#[tokio::test]
async fn resource_reads_distinguish_missing_ids_empty_history_and_invalid_parameters() {
    let directory = TempDir::new().unwrap();
    let router = tracker_server::router_for_database(&directory.path().join("missing.db")).unwrap();
    let task_id = create_fixture_task(&router, "Empty project").await;
    let (status, body) = call(
        &router,
        Method::GET,
        &format!("/v1/worklogs?task_id={task_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["worklogs"], json!([]));
    for route in [
        format!("/v1/tasks/{}", Uuid::now_v7()),
        format!("/v1/worklogs/{}", Uuid::now_v7()),
        format!("/v1/worklogs?task_id={}", Uuid::now_v7()),
    ] {
        let (status, body) = call(&router, Method::GET, &route, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{route}: {body}");
        assert_eq!(body["code"], "not_found");
    }
    for route in [
        "/v1/tasks?archived=invalid",
        "/v1/tasks?unexpected=true",
        "/v1/tasks/not-an-id",
        "/v1/worklogs/not-an-id",
        "/v1/worklogs?task_id=not-an-id",
        "/v1/worklogs?after_task_id=not-an-id",
        "/v1/worklogs?after_id=not-an-id",
        "/v1/worklogs?unexpected=true",
        "/v1/reports/task-totals",
        "/v1/tasks/inactive-preview?as_of=not-a-time",
        "/v1/tasks/inactive-preview?unexpected=true",
    ] {
        let (status, body) = call(&router, Method::GET, route, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{route}: {body}");
        assert_keys(&body, &["code", "message"]);
        assert_eq!(body["code"], "invalid_request");
    }
}

#[tokio::test]
async fn reports_echo_normalized_times_and_include_archived_and_running_work_without_metadata() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("totals.db");
    let outside = SqliteRepository::open(&database).unwrap();
    let mut completed_task = Task::create(
        TaskId::generate(),
        TaskName::new("Completed project").unwrap(),
        at(0),
    );
    let running_task = Task::create(
        TaskId::generate(),
        TaskName::new("Running project").unwrap(),
        at(1),
    );
    outside.create_task(completed_task.clone()).unwrap();
    outside.create_task(running_task.clone()).unwrap();
    let completed = tracker_domain::Worklog::new(
        tracker_domain::WorklogId::generate(),
        completed_task.id(),
        at(5),
        Some(at(15)),
    )
    .unwrap();
    let running = tracker_domain::Worklog::begin(
        tracker_domain::WorklogId::generate(),
        running_task.id(),
        at(20),
    );
    outside.insert_worklog(&completed).unwrap();
    outside.archive_task(completed_task.id(), at(20)).unwrap();
    completed_task.archive(at(20));
    outside.insert_worklog(&running).unwrap();
    let router = tracker_server::router_for_database(&database).unwrap();
    let uri = "/v1/reports/task-totals?start=1970-01-01T01:00:10%2B01:00&end=1970-01-01T01:00:30%2B01:00&now=1970-01-01T01:00:22.123456789%2B01:00";
    let (status, report) = call(&router, Method::GET, uri, None).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_keys(&report, &["start", "end", "now", "rows", "total_us"]);
    assert_eq!(report["start"], json!(at(10)));
    assert_eq!(report["end"], json!(at(30)));
    assert_eq!(
        report["now"],
        json!(at(22) + TimeDelta::microseconds(123456))
    );
    assert_eq!(report["total_us"], 7_123_456);
    let rows = report["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    for row in rows {
        assert_keys(row, &["task_id", "duration_us"]);
    }
    assert_eq!(
        rows[0],
        json!({"task_id": completed_task.id().to_string(), "duration_us": 5_000_000})
    );
    assert_eq!(
        rows[1],
        json!({"task_id": running_task.id().to_string(), "duration_us": 2_123_456})
    );
    let aggregate_uri = uri.replace("/v1/reports/task-totals", "/v1/state/daily-overview");
    let (status, aggregate) = call(&router, Method::GET, &aggregate_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{aggregate}");
    let overview: tracker_protocol::DailyOverviewDto =
        serde_json::from_value(aggregate.clone()).unwrap();
    assert_eq!(serde_json::to_value(&overview.totals).unwrap(), report);
    assert_eq!(overview.tasks.len(), 2);
    assert!(
        overview
            .tasks
            .iter()
            .any(|task| task.id == completed_task.id().to_string() && task.archived)
    );
    assert_eq!(
        overview.active_worklog.as_ref().unwrap().id,
        running.id().to_string()
    );
    assert_eq!(
        overview
            .tasks
            .iter()
            .find(|task| task.id == running_task.id().to_string())
            .unwrap()
            .latest_work_start,
        Some(at(20))
    );
    assert_keys(
        &aggregate,
        &[
            "tasks",
            "active_worklog",
            "start",
            "end",
            "now",
            "rows",
            "total_us",
        ],
    );
    let (status, early) = call(&router, Method::GET, "/v1/reports/task-totals?start=1970-01-01T00:00:00Z&end=1970-01-01T00:00:30Z&now=1970-01-01T00:00:10Z", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        early["rows"],
        json!([{"task_id": completed_task.id().to_string(), "duration_us": 10_000_000}])
    );
    let (status, empty) = call(&router, Method::GET, "/v1/reports/task-totals?start=1970-01-01T00:01:00Z&end=1970-01-01T00:02:00Z&now=1970-01-01T00:00:10Z", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(empty["rows"], json!([]));
    assert_eq!(empty["total_us"], 0);
}

#[tokio::test]
async fn write_receipts_replay_original_resources_after_subsequent_writes_and_expire_on_restart() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("receipts.db");
    let router = tracker_server::router_for_database(&database).unwrap();
    let task_id = Uuid::now_v7().to_string();
    let request = merge(
        json!({"task_id":task_id,"name":"Original project","occurred_at":at(100)}),
        guard(),
    );
    let (status, receipt) = call(&router, Method::POST, "/v1/tasks", Some(request.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert_keys(&receipt, &["request_id", "replayed", "result"]);
    assert_eq!(receipt["request_id"], request["request_id"]);
    assert_eq!(receipt["replayed"], false);
    let rename = merge(
        json!({"action":"rename","expected_name":"Original project","name":"New project","occurred_at":at(200)}),
        guard(),
    );
    let (status, renamed) = call(
        &router,
        Method::PATCH,
        &format!("/v1/tasks/{task_id}"),
        Some(rename),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(renamed["result"]["value"]["name"], "New project");
    let (status, replay) = call(&router, Method::POST, "/v1/tasks", Some(request.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert_keys(&replay, &["request_id", "replayed", "result"]);
    assert_eq!(replay["request_id"], receipt["request_id"]);
    assert_eq!(replay["result"], receipt["result"]);
    assert_eq!(replay["result"]["value"]["name"], "Original project");
    assert_eq!(replay["replayed"], true);
    assert_eq!(state(&router).await.0.tasks[0].name, "New project");
    drop(router);
    let router = tracker_server::router_for_database(&database).unwrap();
    let restarted = state(&router).await;
    // Without its receipt, the retry runs as a new create and finds the existing task.
    let (status, error) = call(&router, Method::POST, "/v1/tasks", Some(request)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    assert_eq!(error["code"], "invalid_request");
    assert!(error.get("replayed").is_none());
    assert_eq!(state(&router).await, restarted);
    assert_eq!(restarted.0.tasks.len(), 1);
    assert_eq!(restarted.0.tasks[0].name, "New project");
    let mut invalid_request = merge(
        json!({"task_id":Uuid::now_v7().to_string(),"name":"Invalid receipt","occurred_at":at(100)}),
        guard(),
    );
    invalid_request["request_id"] = json!("invalid-request-id");
    let (status, error) = call(&router, Method::POST, "/v1/tasks", Some(invalid_request)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["code"], "invalid_request");
}

#[tokio::test]
async fn history_continuations_keep_collection_scope_and_reject_invalidated_cursors() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("pages.db");
    let outside = SqliteRepository::open(&database).unwrap();
    let first = Task::create(
        TaskId::generate(),
        TaskName::new("First project").unwrap(),
        at(0),
    );
    let second = Task::create(
        TaskId::generate(),
        TaskName::new("Second project").unwrap(),
        at(0),
    );
    outside.create_task(first.clone()).unwrap();
    outside.create_task(second.clone()).unwrap();
    for index in 0..52 {
        let start = at(100 + index * 10);
        let worklog = tracker_domain::Worklog::new(
            tracker_domain::WorklogId::generate(),
            first.id(),
            start,
            Some(start + TimeDelta::seconds(5)),
        )
        .unwrap();
        outside.insert_worklog(&worklog).unwrap();
    }
    let router = tracker_server::router_for_database(&database).unwrap();
    let (_, filtered) = call(
        &router,
        Method::GET,
        &format!("/v1/worklogs?task_id={}", first.id()),
        None,
    )
    .await;
    let (_, global) = call(&router, Method::GET, "/v1/worklogs", None).await;
    assert_keys(&global, &["worklogs", "next_cursor"]);
    assert_eq!(filtered["worklogs"].as_array().unwrap().len(), 50);
    assert_eq!(global["worklogs"].as_array().unwrap().len(), 50);
    let cursor = &filtered["next_cursor"];
    assert_keys(cursor, &["id", "revision"]);
    assert_keys(&global["next_cursor"], &["id", "revision"]);
    let continuation = format!(
        "after_id={}&after_revision={}",
        cursor["id"].as_str().unwrap(),
        cursor["revision"]
    );
    let valid_filtered = format!("/v1/worklogs?task_id={}&{continuation}", first.id());
    let (_, next) = call(&router, Method::GET, &valid_filtered, None).await;
    assert_eq!(next["worklogs"].as_array().unwrap().len(), 2);
    assert!(next["next_cursor"].is_null());
    assert!(
        next["worklogs"][0]["start"].as_str().unwrap()
            < filtered["worklogs"][49]["start"].as_str().unwrap()
    );
    let global_cursor = &global["next_cursor"];
    let global_continuation = format!(
        "after_id={}&after_revision={}",
        global_cursor["id"].as_str().unwrap(),
        global_cursor["revision"]
    );
    let (_, next_global) = call(
        &router,
        Method::GET,
        &format!("/v1/worklogs?{global_continuation}"),
        None,
    )
    .await;
    assert_eq!(next_global["worklogs"].as_array().unwrap().len(), 2);
    for route in [
        format!(
            "/v1/worklogs?task_id={}&after_task_id={}&{continuation}",
            first.id(),
            second.id()
        ),
        format!("/v1/worklogs?after_task_id={}&{continuation}", first.id()),
        format!(
            "/v1/worklogs?task_id={}&after_task_id={}",
            first.id(),
            first.id()
        ),
    ] {
        let (status, body) = call(&router, Method::GET, &route, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{route}: {body}");
        assert_eq!(body["code"], "invalid_request");
    }
    let edit = merge(
        json!({"action":"correct","expected_start":filtered["worklogs"][0]["start"],"expected_end":filtered["worklogs"][0]["end"],"replacement_start":at(609),"replacement_end":at(615),"occurred_at":at(1000)}),
        guard(),
    );
    let (status, body) = call(
        &router,
        Method::PATCH,
        &format!(
            "/v1/worklogs/{}",
            filtered["worklogs"][0]["id"].as_str().unwrap()
        ),
        Some(edit),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    for route in [
        valid_filtered,
        format!("/v1/worklogs?{global_continuation}"),
    ] {
        let (status, body) = call(&router, Method::GET, &route, None).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["code"], "worklog_history_changed");
    }
}

#[tokio::test]
async fn removed_bundled_routes_and_parallel_versions_are_unavailable() {
    let directory = TempDir::new().unwrap();
    let router = tracker_server::router_for_database(&directory.path().join("routes.db")).unwrap();
    let id = create_fixture_task(&router, "Routes project").await;
    for route in [
        "/v1/snapshot".to_owned(),
        "/v1/reports".to_owned(),
        format!("/v1/tasks/{id}/worklogs"),
        "/v2/tasks".to_owned(),
    ] {
        let (status, _) = call(&router, Method::GET, &route, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{route}");
    }
    let (status, _) = call(&router, Method::GET, "/v1/tasks/inactive-candidates", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(
        &router,
        Method::POST,
        "/v1/tasks/archive-inactive-candidates",
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn aggregate_routes_load_empty_idle_state_and_validate_interval_parameters() {
    let directory = TempDir::new().unwrap();
    let router = tracker_server::router_for_database(&directory.path().join("empty.db")).unwrap();
    let (status, body) = call(&router, Method::GET, "/v1/state/task-list", None).await;
    assert_eq!(status, StatusCode::OK);
    let state: tracker_protocol::TaskListStateDto = serde_json::from_value(body.clone()).unwrap();
    assert!(state.tasks.is_empty());
    assert!(state.active_worklog.is_none());
    assert_keys(&body, &["tasks", "active_worklog"]);
    for query in [
        "",
        "?start=bad&end=bad&now=bad",
        "?start=1970-01-01T00:00:10Z&end=1970-01-01T00:00:10Z&now=1970-01-01T00:00:10Z",
        "?start=1970-01-01T00:00:00Z&end=1970-01-01T00:00:10Z",
        "?start=1970-01-01T00:00:00Z&end=1970-01-01T00:00:10Z&now=1970-01-01T00:00:05Z&extra=true",
    ] {
        let (status, _) = call(
            &router,
            Method::GET,
            &format!("/v1/state/daily-overview{query}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
    }
    let uri = "/v1/state/daily-overview?start=1970-01-01T00:00:00Z&end=1970-01-01T00:00:10Z&now=1970-01-01T00:00:05Z";
    let (status, body) = call(&router, Method::GET, uri, None).await;
    assert_eq!(status, StatusCode::OK);
    let overview: tracker_protocol::DailyOverviewDto = serde_json::from_value(body).unwrap();
    assert!(overview.tasks.is_empty());
    assert!(overview.active_worklog.is_none());
    assert!(overview.totals.rows.is_empty());
    assert_eq!(overview.totals.total_us, 0);
}

#[tokio::test]
async fn aggregate_routes_refresh_both_resources_after_database_changes() {
    use tracker_domain::{Worklog, WorklogId};
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("refresh.db");
    let outside = SqliteRepository::open(&database).unwrap();
    let router = tracker_server::router_for_database(&database).unwrap();
    let task = Task::create(
        TaskId::generate(),
        TaskName::new("New task").unwrap(),
        at(1),
    );
    outside.create_task(task.clone()).unwrap();
    let worklog = Worklog::begin(WorklogId::generate(), task.id(), at(2));
    outside.insert_worklog(&worklog).unwrap();
    for uri in [
        "/v1/state/task-list",
        "/v1/state/daily-overview?start=1970-01-01T00:00:00Z&end=1970-01-01T00:00:10Z&now=1970-01-01T00:00:05Z",
    ] {
        let (status, body) = call(&router, Method::GET, uri, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["tasks"][0]["id"], task.id().to_string());
        assert_eq!(body["tasks"][0]["latest_work_start"], json!(at(2)));
        assert_eq!(body["active_worklog"]["id"], worklog.id().to_string());
        assert_eq!(body["active_worklog"]["task_id"], body["tasks"][0]["id"]);
        if uri.contains("daily-overview") {
            assert_eq!(body["total_us"], 3_000_000);
        }
    }
}

#[tokio::test]
async fn zero_duration_commands_reject_equal_canonical_instants_without_writes() {
    let temp = TempDir::new().unwrap();
    let router = tracker_server::router_for_database(&temp.path().join("tracker.db")).unwrap();
    let first = create_fixture_task(&router, "First task").await;
    let second = create_fixture_task(&router, "Second task").await;
    let worklog_id = Uuid::now_v7().to_string();
    let start = merge(
        json!({"task_id":first,"worklog_id":worklog_id,"expected_active":null,"occurred_at":at(100)}),
        guard(),
    );
    let (status, body) = call(&router, Method::PUT, "/v1/tracking", Some(start)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let before = state(&router).await;
    let (_, history) = call(&router, Method::GET, "/v1/worklogs", None).await;
    for instant in [at(100), at(100) + TimeDelta::nanoseconds(999)] {
        for task in [None, Some(second.clone())] {
            let new_id = task.as_ref().map(|_| Uuid::now_v7().to_string());
            let request = merge(
                json!({"task_id":task,"worklog_id":new_id,"expected_active":worklog_id,"occurred_at":instant}),
                guard(),
            );
            let (status, body) = call(&router, Method::PUT, "/v1/tracking", Some(request)).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
            assert!(body["message"].as_str().unwrap().contains("later than"));
            assert_eq!(state(&router).await, before);
            assert_eq!(
                call(&router, Method::GET, "/v1/worklogs", None).await.1,
                history
            );
        }
    }
    let stop = merge(
        json!({"task_id":null,"worklog_id":null,"expected_active":worklog_id,"occurred_at":at(101)}),
        guard(),
    );
    let (status, body) = call(&router, Method::PUT, "/v1/tracking", Some(stop)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let before = state(&router).await;
    for end in [at(100), at(100) + TimeDelta::nanoseconds(999)] {
        let correction = merge(
            json!({"action":"correct","expected_start":at(100),"expected_end":at(101),"replacement_start":at(100),"replacement_end":end,"occurred_at":at(102)}),
            guard(),
        );
        let (status, body) = call(
            &router,
            Method::PATCH,
            &format!("/v1/worklogs/{worklog_id}"),
            Some(correction),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(state(&router).await, before);
        let (_, stored) = call(
            &router,
            Method::GET,
            &format!("/v1/worklogs/{worklog_id}"),
            None,
        )
        .await;
        assert_eq!(stored["worklog"]["end"], json!(at(101)));
    }
}

/// Sends a write that must succeed and returns its receipt.
async fn write(router: &axum::Router, method: Method, uri: &str, body: Value) -> MutationDto {
    let (status, response) = call(router, method, uri, Some(merge(body, guard()))).await;
    assert_eq!(status, StatusCode::OK, "{uri}: {response}");
    serde_json::from_value(response).unwrap()
}

/// Starts and stops one worklog so it becomes a completed worklog.
async fn completed_worklog(router: &axum::Router, task_id: &str, start: i64, end: i64) -> String {
    let worklog_id = Uuid::now_v7().to_string();
    let start = json!({"task_id":task_id,"worklog_id":worklog_id,"expected_active":null,"occurred_at":at(start)});
    write(router, Method::PUT, "/v1/tracking", start).await;
    let stop = json!({"task_id":null,"worklog_id":null,"expected_active":worklog_id,"occurred_at":at(end)});
    write(router, Method::PUT, "/v1/tracking", stop).await;
    worklog_id
}

/// Builds a write from a read, lets another client change an unrelated task, then sends the write.
async fn write_after_unrelated_write(
    router: &axum::Router,
    method: Method,
    uri: &str,
    body: impl FnOnce(&(TasksDto, TrackingDto)) -> Value,
) -> MutationDto {
    let read = state(router).await;
    let request = body(&read);
    let other_client = json!({
        "task_id": Uuid::now_v7().to_string(),
        "name": format!("Other client task {}", read.0.tasks.len()),
        "occurred_at": at(0),
    });
    write(router, Method::POST, "/v1/tasks", other_client).await;
    assert_eq!(state(router).await.0.tasks.len(), read.0.tasks.len() + 1);
    write(router, method, uri, request).await
}

fn task_name_in(read: &(TasksDto, TrackingDto), id: &str) -> String {
    read.0
        .tasks
        .iter()
        .find(|task| task.id == id)
        .unwrap()
        .name
        .clone()
}

fn active_worklog_in(read: &(TasksDto, TrackingDto)) -> Option<String> {
    read.1
        .active_worklog
        .as_ref()
        .map(|worklog| worklog.id.clone())
}

#[tokio::test]
async fn every_write_succeeds_after_an_unrelated_write_by_another_client() {
    let temp = TempDir::new().unwrap();
    let router = tracker_server::router_for_database(&temp.path().join("tracker.db")).unwrap();
    let other = create_fixture_task(&router, "Other task").await;
    let id = Uuid::now_v7().to_string();
    let task_route = format!("/v1/tasks/{id}");
    let worklog_id = Uuid::now_v7().to_string();
    let worklog_route = format!("/v1/worklogs/{worklog_id}");

    let created = write_after_unrelated_write(
        &router,
        Method::POST,
        "/v1/tasks",
        |_| json!({"task_id":id,"name":"Reviewed task","occurred_at":at(100)}),
    )
    .await;
    assert!(
        matches!(created.result, MutationResultDto::Task(task) if task.name == "Reviewed task")
    );
    let renamed = write_after_unrelated_write(&router, Method::PATCH, &task_route, |read| {
        json!({"action":"rename","expected_name":task_name_in(read, &id),"name":"Renamed task","occurred_at":at(110)})
    })
    .await;
    assert!(matches!(renamed.result, MutationResultDto::Task(task) if task.name == "Renamed task"));
    let archived = write_after_unrelated_write(&router, Method::PATCH, &task_route, |read| {
        json!({"action":"archive","expected_name":task_name_in(read, &id),"occurred_at":at(120)})
    })
    .await;
    assert!(matches!(archived.result, MutationResultDto::Task(task) if task.archived));
    let restored = write_after_unrelated_write(
        &router,
        Method::PATCH,
        &task_route,
        |_| json!({"action":"restore","occurred_at":at(130)}),
    )
    .await;
    assert!(matches!(restored.result, MutationResultDto::Task(task) if !task.archived));

    let started = write_after_unrelated_write(&router, Method::PUT, "/v1/tracking", |read| {
        json!({"task_id":id,"worklog_id":worklog_id,"expected_active":active_worklog_in(read),"occurred_at":at(200)})
    })
    .await;
    assert!(
        matches!(started.result, MutationResultDto::Worklog(worklog) if worklog.id == worklog_id)
    );
    let switched = write_after_unrelated_write(&router, Method::PUT, "/v1/tracking", |read| {
        json!({"task_id":other,"worklog_id":Uuid::now_v7().to_string(),"expected_active":active_worklog_in(read),"occurred_at":at(300)})
    })
    .await;
    assert!(
        matches!(switched.result, MutationResultDto::TrackingSwitched { stopped, .. } if stopped.id == worklog_id)
    );
    let cleared = write_after_unrelated_write(&router, Method::PUT, "/v1/tracking", |read| {
        json!({"task_id":null,"worklog_id":null,"expected_active":active_worklog_in(read),"occurred_at":at(400)})
    })
    .await;
    assert!(
        matches!(cleared.result, MutationResultDto::Worklog(worklog) if worklog.end == Some(at(400)))
    );

    let corrected = write_after_unrelated_write(&router, Method::PATCH, &worklog_route, |_| {
        json!({"action":"correct","expected_start":at(200),"expected_end":at(300),"replacement_start":at(210),"replacement_end":at(290),"occurred_at":at(500)})
    })
    .await;
    assert!(
        matches!(corrected.result, MutationResultDto::Worklog(worklog) if worklog.start == at(210))
    );
    let moved = write_after_unrelated_write(&router, Method::PATCH, &worklog_route, |_| {
        json!({"action":"move","expected_task_id":id,"expected_start":at(210),"expected_end":at(290),"destination_task_id":other})
    })
    .await;
    assert!(
        matches!(moved.result, MutationResultDto::Worklog(worklog) if worklog.task_id == other)
    );
    let deleted = write_after_unrelated_write(
        &router,
        Method::DELETE,
        &worklog_route,
        |_| json!({"expected_task_id":other,"expected_start":at(210),"expected_end":at(290)}),
    )
    .await;
    assert!(
        matches!(deleted.result, MutationResultDto::Worklog(worklog) if worklog.id == worklog_id)
    );
    let (status, _) = call(&router, Method::GET, &worklog_route, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let as_of = Utc::now();
    let (status, body) = call(&router, Method::GET, &preview_uri(as_of), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let preview: InactiveTaskPreviewDto = serde_json::from_value(body).unwrap();
    let archived_inactive = write_after_unrelated_write(
        &router,
        Method::POST,
        "/v1/tasks/archive-inactive",
        |_| json!({"as_of": preview.as_of, "inactive_days": preview.inactive_days}),
    )
    .await;
    // The other client's latest task also counts because it was created long ago.
    assert_eq!(
        archived_inactive.result,
        MutationResultDto::ArchivedInactive {
            count: preview.count + 1
        }
    );
}

/// Sends each write, expects the given conflict code and checks that nothing changed.
async fn assert_conflicts_without_writing(
    router: &axum::Router,
    cases: Vec<(Method, String, Value, &str)>,
) {
    let before = state(router).await;
    let (_, history) = call(router, Method::GET, "/v1/worklogs", None).await;
    for (method, uri, body, code) in cases {
        let (status, response) = call(router, method, &uri, Some(merge(body, guard()))).await;
        assert_eq!(status, StatusCode::CONFLICT, "{uri}: {response}");
        assert_eq!(response["code"], code, "{uri}: {response}");
        assert_eq!(state(router).await, before);
        assert_eq!(
            call(router, Method::GET, "/v1/worklogs", None).await.1,
            history
        );
    }
}

/// Task names on rename and archive are covered by
/// `rename_and_archive_reject_a_name_another_client_changed_after_review`.
#[tokio::test]
async fn writes_based_on_an_outdated_item_are_rejected_by_expected_values() {
    let temp = TempDir::new().unwrap();
    let router = tracker_server::router_for_database(&temp.path().join("tracker.db")).unwrap();
    let first = create_fixture_task(&router, "First task").await;
    let second = create_fixture_task(&router, "Second task").await;
    // This client reviews the worklog on the first task from 100 to 150.
    let worklog_id = completed_worklog(&router, &first, 100, 150).await;
    let route = format!("/v1/worklogs/{worklog_id}");
    // Another client corrects the times and then moves the worklog.
    write(
        &router,
        Method::PATCH,
        &route,
        json!({"action":"correct","expected_start":at(100),"expected_end":at(150),"replacement_start":at(90),"replacement_end":at(140),"occurred_at":at(160)}),
    )
    .await;
    write(
        &router,
        Method::PATCH,
        &route,
        json!({"action":"move","expected_task_id":first,"expected_start":at(90),"expected_end":at(140),"destination_task_id":second}),
    )
    .await;
    let changed = "worklog_changed";
    assert_conflicts_without_writing(
        &router,
        vec![
            (
                Method::PATCH,
                route.clone(),
                json!({"action":"move","expected_task_id":second,"expected_start":at(100),"expected_end":at(150),"destination_task_id":first}),
                changed,
            ),
            (
                Method::PATCH,
                route.clone(),
                json!({"action":"move","expected_task_id":first,"expected_start":at(90),"expected_end":at(140),"destination_task_id":first}),
                changed,
            ),
            (
                Method::PATCH,
                route.clone(),
                json!({"action":"correct","expected_start":at(100),"expected_end":at(150),"replacement_start":at(95),"replacement_end":at(145),"occurred_at":at(170)}),
                changed,
            ),
            (
                Method::DELETE,
                route.clone(),
                json!({"expected_task_id":second,"expected_start":at(100),"expected_end":at(150)}),
                changed,
            ),
            (
                Method::DELETE,
                route.clone(),
                json!({"expected_task_id":first,"expected_start":at(90),"expected_end":at(140)}),
                changed,
            ),
        ],
    )
    .await;

    // This client reviews idle tracking before another client starts a timer.
    let reviewed_id = Uuid::now_v7().to_string();
    write(
        &router,
        Method::PUT,
        "/v1/tracking",
        json!({"task_id":second,"worklog_id":reviewed_id,"expected_active":null,"occurred_at":at(200)}),
    )
    .await;
    assert_conflicts_without_writing(
        &router,
        vec![
            (
                Method::PUT,
                "/v1/tracking".to_owned(),
                json!({"task_id":first,"worklog_id":Uuid::now_v7().to_string(),"expected_active":null,"occurred_at":at(210)}),
                "conflict",
            ),
            (
                Method::PUT,
                "/v1/tracking".to_owned(),
                json!({"task_id":null,"worklog_id":null,"expected_active":null,"occurred_at":at(210)}),
                "conflict",
            ),
        ],
    )
    .await;

    // This client reviews the timer before another client switches it.
    write(
        &router,
        Method::PUT,
        "/v1/tracking",
        json!({"task_id":first,"worklog_id":Uuid::now_v7().to_string(),"expected_active":reviewed_id,"occurred_at":at(300)}),
    )
    .await;
    assert_conflicts_without_writing(
        &router,
        vec![
            (
                Method::PUT,
                "/v1/tracking".to_owned(),
                json!({"task_id":second,"worklog_id":Uuid::now_v7().to_string(),"expected_active":reviewed_id,"occurred_at":at(310)}),
                "conflict",
            ),
            (
                Method::PUT,
                "/v1/tracking".to_owned(),
                json!({"task_id":null,"worklog_id":null,"expected_active":reviewed_id,"occurred_at":at(310)}),
                "conflict",
            ),
        ],
    )
    .await;
}

/// A protocol 5 client sends `expected_revision`; the server rejects the field instead of
/// ignoring it.
#[tokio::test]
async fn writes_that_send_an_expected_revision_are_rejected_without_writing() {
    let temp = TempDir::new().unwrap();
    let router = tracker_server::router_for_database(&temp.path().join("tracker.db")).unwrap();
    let id = create_fixture_task(&router, "Reviewed task").await;
    let other = create_fixture_task(&router, "Other task").await;
    let worklog_id = completed_worklog(&router, &id, 100, 150).await;
    let task_route = format!("/v1/tasks/{id}");
    let worklog_route = format!("/v1/worklogs/{worklog_id}");
    let before = state(&router).await;
    let (_, history) = call(&router, Method::GET, "/v1/worklogs", None).await;
    let cases = [
        (
            Method::POST,
            "/v1/tasks".to_owned(),
            json!({"task_id":Uuid::now_v7().to_string(),"name":"New task","occurred_at":at(200)}),
        ),
        (
            Method::PATCH,
            task_route.clone(),
            json!({"action":"rename","expected_name":"Reviewed task","name":"Renamed task","occurred_at":at(200)}),
        ),
        (
            Method::PATCH,
            task_route.clone(),
            json!({"action":"archive","expected_name":"Reviewed task","occurred_at":at(200)}),
        ),
        (
            Method::PATCH,
            task_route.clone(),
            json!({"action":"restore","occurred_at":at(200)}),
        ),
        (
            Method::POST,
            "/v1/tasks/archive-inactive".to_owned(),
            json!({"as_of":Utc::now(),"inactive_days":7}),
        ),
        (
            Method::PUT,
            "/v1/tracking".to_owned(),
            json!({"task_id":id,"worklog_id":Uuid::now_v7().to_string(),"expected_active":null,"occurred_at":at(200)}),
        ),
        (
            Method::PUT,
            "/v1/tracking".to_owned(),
            json!({"task_id":null,"worklog_id":null,"expected_active":null,"occurred_at":at(200)}),
        ),
        (
            Method::PATCH,
            worklog_route.clone(),
            json!({"action":"correct","expected_start":at(100),"expected_end":at(150),"replacement_start":at(90),"replacement_end":at(140),"occurred_at":at(200)}),
        ),
        (
            Method::PATCH,
            worklog_route.clone(),
            json!({"action":"move","expected_task_id":id,"expected_start":at(100),"expected_end":at(150),"destination_task_id":other}),
        ),
        (
            Method::DELETE,
            worklog_route.clone(),
            json!({"expected_task_id":id,"expected_start":at(100),"expected_end":at(150)}),
        ),
    ];
    for (method, uri, body) in cases {
        let mut request = merge(body, guard());
        request["expected_revision"] = json!("00000000-0000-7000-8000-000000000000:3");
        let (status, response) = call(&router, method, &uri, Some(request)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {response}");
        assert_eq!(response["code"], "invalid_request", "{uri}: {response}");
        assert_eq!(response["message"], "Invalid JSON request", "{uri}");
        assert_eq!(state(&router).await, before);
        assert_eq!(
            call(&router, Method::GET, "/v1/worklogs", None).await.1,
            history
        );
    }
}

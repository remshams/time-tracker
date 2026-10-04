use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode};
use chrono::{DateTime, TimeDelta, Utc};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;
use tracker_domain::{Task, TaskId, TaskName};
use tracker_protocol::{InactiveTaskPreviewDto, MutationDto, MutationResultDto, SnapshotDto};
use tracker_storage::SqliteRepository;
use uuid::Uuid;

fn at(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 0).unwrap()
}

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

fn guard(revision: &str) -> Value {
    json!({"expected_revision":revision,"request_id":Uuid::now_v7().to_string()})
}

fn merge(mut body: Value, guard: Value) -> Value {
    body.as_object_mut()
        .unwrap()
        .extend(guard.as_object().unwrap().clone());
    body
}

async fn state(router: &axum::Router) -> SnapshotDto {
    let (status, body) = call(router, Method::GET, "/v1/snapshot", None).await;
    assert_eq!(status, StatusCode::OK);
    serde_json::from_value(body).unwrap()
}

async fn create_fixture_task(router: &axum::Router, name: &str) -> String {
    let before = state(router).await;
    let id = Uuid::now_v7().to_string();
    let request = merge(
        json!({"task_id": id, "name": name, "occurred_at": at(0)}),
        guard(&before.revision),
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
            "candidate_fingerprint": preview.candidate_fingerprint,
        }),
        guard(&preview.revision),
    )
}

#[tokio::test]
async fn inactive_preview_archives_once_and_replays_the_same_result() {
    let directory = TempDir::new().unwrap();
    let router = tracker_server::router_for_database(&directory.path().join("tracker.db")).unwrap();
    let as_of = Utc::now();
    let created_at = as_of - TimeDelta::days(20);
    let before = state(&router).await;
    let create = merge(
        json!({
            "task_id": Uuid::now_v7().to_string(),
            "name": "Old planning task",
            "occurred_at": created_at,
        }),
        guard(&before.revision),
    );
    let (status, body) = call(&router, Method::POST, "/v1/tasks", Some(create)).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = call(&router, Method::GET, &preview_uri(as_of), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let preview: InactiveTaskPreviewDto = serde_json::from_value(body).unwrap();
    assert_eq!(preview.count, 1);
    assert_eq!(preview.sample_names, ["Old planning task"]);
    assert_eq!(preview.candidate_fingerprint.len(), 64);

    let request = archive_request(&preview);
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
        mutation
            .snapshot
            .task_items
            .iter()
            .any(|item| { item.task.name == "Old planning task" && item.task.archived })
    );
    let (status, replay) = call(
        &router,
        Method::POST,
        "/v1/tasks/archive-inactive",
        Some(request),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay, first);
}

#[tokio::test]
async fn inactive_preview_bounds_sample_without_limiting_candidate_count() {
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
    assert_eq!(preview.sample_names.len(), 5);
}

#[tokio::test]
async fn inactive_archive_rejects_a_changed_candidate_set_without_writing() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("tracker.db");
    let router = tracker_server::router_for_database(&database).unwrap();
    let as_of = Utc::now();
    let (status, body) = call(&router, Method::GET, &preview_uri(as_of), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let preview: InactiveTaskPreviewDto = serde_json::from_value(body).unwrap();

    let outside = SqliteRepository::open(&database).unwrap();
    outside
        .create_task(Task::create(
            TaskId::generate(),
            TaskName::new("Changed outside server").unwrap(),
            as_of - TimeDelta::days(20),
        ))
        .unwrap();
    let (status, body) = call(
        &router,
        Method::POST,
        "/v1/tasks/archive-inactive",
        Some(archive_request(&preview)),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "conflict");
    assert!(
        !outside
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|task| task.name().as_str() == "Changed outside server")
            .unwrap()
            .is_archived()
    );
}

#[tokio::test]
async fn inactive_archive_rejects_a_tampered_fingerprint_without_writing() {
    let directory = TempDir::new().unwrap();
    let database = directory.path().join("tracker.db");
    let router = tracker_server::router_for_database(&database).unwrap();
    let as_of = Utc::now();
    let outside = SqliteRepository::open(&database).unwrap();
    outside
        .create_task(Task::create(
            TaskId::generate(),
            TaskName::new("Old candidate").unwrap(),
            as_of - TimeDelta::days(20),
        ))
        .unwrap();
    let (status, body) = call(&router, Method::GET, &preview_uri(as_of), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let preview: InactiveTaskPreviewDto = serde_json::from_value(body).unwrap();
    assert_eq!(preview.count, 1);

    let mut request = archive_request(&preview);
    request["candidate_fingerprint"] = json!("0".repeat(64));
    let (status, body) = call(
        &router,
        Method::POST,
        "/v1/tasks/archive-inactive",
        Some(request),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "conflict");
    for fingerprint in ["a".repeat(63), "g".repeat(64)] {
        let mut request = archive_request(&preview);
        request["candidate_fingerprint"] = json!(fingerprint);
        let (status, body) = call(
            &router,
            Method::POST,
            "/v1/tasks/archive-inactive",
            Some(request),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["code"], "invalid_request");
    }
    assert!(
        !outside
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|task| task.name().as_str() == "Old candidate")
            .unwrap()
            .is_archived()
    );
}

#[tokio::test]
async fn inactive_archive_rejects_stale_revision_and_out_of_range_time() {
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
        guard(&preview.revision),
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
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "stale_revision");
}

#[tokio::test]
async fn health_create_retry_and_stale_revision() {
    let temp = TempDir::new().unwrap();
    let router = tracker_server::router_for_database(&temp.path().join("tracker.db")).unwrap();
    let (status, health) = call(&router, Method::GET, "/v1/health", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(health, json!({"status":"ok","protocol_version":1}));
    let before = state(&router).await;
    assert!(before.task_items.is_empty());
    let task_id = Uuid::now_v7().to_string();
    let request = merge(
        json!({"task_id":task_id,"name":"Write tests","occurred_at":at(100)}),
        guard(&before.revision),
    );
    let (status, first) = call(&router, Method::POST, "/v1/tasks", Some(request.clone())).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let first: MutationDto = serde_json::from_value(first.clone()).unwrap();
    assert_eq!(first.snapshot.task_items.len(), 1);
    let (status, replay) = call(&router, Method::POST, "/v1/tasks", Some(request.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay, serde_json::to_value(&first).unwrap());
    let mut different = request.clone();
    different["name"] = json!("Different");
    let (status, error) = call(&router, Method::POST, "/v1/tasks", Some(different)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["code"], "conflict");
    let stale = merge(
        json!({"task_id":Uuid::now_v7().to_string(),"name":"Stale","occurred_at":at(101)}),
        guard(&before.revision),
    );
    let (status, error) = call(&router, Method::POST, "/v1/tasks", Some(stale)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["code"], "stale_revision");
    assert_eq!(state(&router).await.task_items.len(), 1);
}

#[tokio::test]
async fn tracking_survives_restart_and_worklogs_support_edits() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("tracker.db");
    let router = tracker_server::router_for_database(&path).unwrap();
    let first_task = create_fixture_task(&router, "First project").await;
    let second_task = create_fixture_task(&router, "Second project").await;
    let initial = state(&router).await;
    let first_id = Uuid::now_v7().to_string();
    let set = merge(
        json!({"task_id":first_task,"worklog_id":first_id,"expected_active":null,"occurred_at":at(100)}),
        guard(&initial.revision),
    );
    let (status, body) = call(&router, Method::PUT, "/v1/tracking", Some(set)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let started: MutationDto = serde_json::from_value(body).unwrap();
    assert_eq!(
        started.snapshot.active_worklog.as_ref().unwrap().id,
        first_id
    );
    drop(router);

    let router = tracker_server::router_for_database(&path).unwrap();
    let resumed = state(&router).await;
    assert_eq!(resumed.active_worklog.as_ref().unwrap().id, first_id);
    let switch_id = Uuid::now_v7().to_string();
    let switch = merge(
        json!({"task_id":second_task,"worklog_id":switch_id,"expected_active":first_id,"occurred_at":at(150)}),
        guard(&resumed.revision),
    );
    let (status, body) = call(&router, Method::PUT, "/v1/tracking", Some(switch)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let switched: MutationDto = serde_json::from_value(body).unwrap();
    assert_eq!(
        switched.snapshot.active_worklog.as_ref().unwrap().id,
        switch_id
    );
    let clear = merge(
        json!({"task_id":null,"worklog_id":null,"expected_active":switch_id,"occurred_at":at(200)}),
        guard(&switched.snapshot.revision),
    );
    let (status, body) = call(&router, Method::PUT, "/v1/tracking", Some(clear)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let stopped: MutationDto = serde_json::from_value(body).unwrap();
    assert!(stopped.snapshot.active_worklog.is_none());

    let (status, body) = call(&router, Method::GET, "/v1/worklogs", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["worklogs"].as_array().unwrap().len(), 2);
    let route = format!("/v1/tasks/{first_task}/worklogs");
    let (status, body) = call(&router, Method::GET, &route, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["worklogs"].as_array().unwrap().len(), 1);

    let correction = merge(
        json!({"action":"correct","expected_start":at(100),"expected_end":at(150),"replacement_start":at(90),"replacement_end":at(140),"occurred_at":at(210)}),
        guard(&stopped.snapshot.revision),
    );
    let route = format!("/v1/worklogs/{first_id}");
    let (status, body) = call(&router, Method::PATCH, &route, Some(correction)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let corrected: MutationDto = serde_json::from_value(body).unwrap();
    let movement = merge(
        json!({"action":"move","expected_task_id":first_task,"expected_start":at(90),"expected_end":at(140),"destination_task_id":second_task}),
        guard(&corrected.snapshot.revision),
    );
    let (status, body) = call(&router, Method::PATCH, &route, Some(movement)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let moved: MutationDto = serde_json::from_value(body).unwrap();
    let report_uri = format!(
        "/v1/reports?start={}&end={}&now={}",
        at(0).to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        at(300).to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        at(300).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    );
    let (status, body) = call(&router, Method::GET, &report_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total_us"], 100_000_000);
    let deletion = merge(
        json!({"expected_task_id":second_task,"expected_start":at(90),"expected_end":at(140)}),
        guard(&moved.snapshot.revision),
    );
    let (status, body) = call(&router, Method::DELETE, &route, Some(deletion)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let deleted: MutationDto = serde_json::from_value(body).unwrap();
    assert!(deleted.snapshot.active_worklog.is_none());
    let (status, body) = call(&router, Method::GET, "/v1/worklogs", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["worklogs"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn task_changes_and_request_limit() {
    let temp = TempDir::new().unwrap();
    let router = tracker_server::router_for_database(&temp.path().join("tracker.db")).unwrap();
    let id = create_fixture_task(&router, "Change this task").await;
    let initial = state(&router).await;
    let route = format!("/v1/tasks/{id}");
    let rename = merge(
        json!({"action":"rename","name":"Renamed task","occurred_at":at(100)}),
        guard(&initial.revision),
    );
    let (status, body) = call(&router, Method::PATCH, &route, Some(rename)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let renamed: MutationDto = serde_json::from_value(body).unwrap();
    let archive = merge(
        json!({"action":"archive","occurred_at":at(110)}),
        guard(&renamed.snapshot.revision),
    );
    let (status, body) = call(&router, Method::PATCH, &route, Some(archive)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let archived: MutationDto = serde_json::from_value(body).unwrap();
    assert!(
        archived
            .snapshot
            .task_items
            .iter()
            .find(|item| item.task.id == id)
            .unwrap()
            .task
            .archived
    );
    let restore = merge(
        json!({"action":"restore","occurred_at":at(120)}),
        guard(&archived.snapshot.revision),
    );
    let (status, body) = call(&router, Method::PATCH, &route, Some(restore)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let restored: MutationDto = serde_json::from_value(body).unwrap();
    assert!(
        !restored
            .snapshot
            .task_items
            .iter()
            .find(|item| item.task.id == id)
            .unwrap()
            .task
            .archived
    );
    let large = json!({"task_id":Uuid::now_v7().to_string(),"name":"x".repeat(17_000),"occurred_at":at(100),"expected_revision":restored.snapshot.revision,"request_id":Uuid::now_v7().to_string()});
    let (status, _) = call(&router, Method::POST, "/v1/tasks", Some(large)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);

    let between_mutated_and_real_limit = json!({
        "task_id": Uuid::now_v7().to_string(),
        "name": "Below the server body limit",
        "occurred_at": at(130),
        "unexpected": "x".repeat(2_000),
        "expected_revision": restored.snapshot.revision,
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
async fn concurrent_commands_and_duplicate_server_are_rejected_safely() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("tracker.db");
    let router = tracker_server::router_for_database(&path).unwrap();
    assert!(tracker_server::router_for_database(&path).is_err());
    let before = state(&router).await;
    let first = merge(
        json!({"task_id":Uuid::now_v7().to_string(),"name":"First","occurred_at":at(100)}),
        guard(&before.revision),
    );
    let second = merge(
        json!({"task_id":Uuid::now_v7().to_string(),"name":"Second","occurred_at":at(100)}),
        guard(&before.revision),
    );
    let (left, right) = tokio::join!(
        call(&router, Method::POST, "/v1/tasks", Some(first)),
        call(&router, Method::POST, "/v1/tasks", Some(second))
    );
    let statuses = [left.0, right.0];
    assert!(statuses.contains(&StatusCode::OK));
    assert!(statuses.contains(&StatusCode::CONFLICT));
    assert_eq!(state(&router).await.task_items.len(), 1);
}

#[tokio::test]
async fn invalid_json_does_not_change_state() {
    let temp = TempDir::new().unwrap();
    let router = tracker_server::router_for_database(&temp.path().join("tracker.db")).unwrap();
    let before = state(&router).await;
    let unknown = merge(
        json!({"task_id":Uuid::now_v7().to_string(),"name":"Input","occurred_at":at(100),"unexpected":"value"}),
        guard(&before.revision),
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
        guard(&before.revision),
    );
    let (status, body) = call(&router, Method::POST, "/v1/tasks", Some(invalid_name)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid_request");
    assert_eq!(state(&router).await, before);

    let missing_id = Uuid::now_v7().to_string();
    let change = merge(
        json!({"action":"rename","name":"Missing task","occurred_at":at(100)}),
        guard(&before.revision),
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
    let start = at(100).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let worklog_id = Uuid::now_v7().to_string();

    for route in [
        format!("/v1/tasks/{task_id}/worklogs?after_start={start}"),
        format!("/v1/worklogs?after_start={start}"),
    ] {
        let (status, body) = call(&router, Method::GET, &route, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{route}");
        assert_eq!(body["code"], "invalid_request", "{route}");
    }

    for route in [
        format!(
            "/v1/tasks/{task_id}/worklogs?after_start={start}&after_id={worklog_id}&after_revision=-1"
        ),
        format!("/v1/worklogs?after_start={start}&after_id={worklog_id}&after_revision=-1"),
    ] {
        let (status, body) = call(&router, Method::GET, &route, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{route}");
        assert_eq!(body["code"], "invalid_request", "{route}");
    }

    for route in [
        format!(
            "/v1/tasks/{task_id}/worklogs?after_start={start}&after_id={worklog_id}&after_revision=0"
        ),
        format!("/v1/worklogs?after_start={start}&after_id={worklog_id}&after_revision=0"),
    ] {
        let (status, _) = call(&router, Method::GET, &route, None).await;
        assert_eq!(status, StatusCode::OK, "{route}");
    }
}

#[tokio::test]
async fn stopping_an_already_idle_tracker_is_an_idempotent_success() {
    let temp = TempDir::new().unwrap();
    let router = tracker_server::router_for_database(&temp.path().join("tracker.db")).unwrap();
    let before = state(&router).await;
    let request = merge(
        json!({"task_id":null,"worklog_id":null,"expected_active":null,"occurred_at":at(100)}),
        guard(&before.revision),
    );

    let (status, first) = call(&router, Method::PUT, "/v1/tracking", Some(request.clone())).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["result"]["kind"], "tracking_already_idle");
    assert_eq!(first["snapshot"]["active_worklog"], Value::Null);

    let (status, retry) = call(&router, Method::PUT, "/v1/tracking", Some(request)).await;
    assert_eq!(status, StatusCode::OK, "{retry}");
    assert_eq!(retry, first);
    assert!(state(&router).await.task_items.is_empty());
}

#[tokio::test]
async fn a_new_server_database_stays_empty_after_restart() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("tracker.db");
    for _ in 0..2 {
        let router = tracker_server::router_for_database(&path).unwrap();
        let snapshot = state(&router).await;
        assert!(snapshot.task_items.is_empty());
        assert!(snapshot.active_worklog.is_none());
    }
    let repository = SqliteRepository::open(&path).unwrap();
    assert!(repository.list_tasks().unwrap().is_empty());
    assert!(repository.active_worklog().unwrap().is_none());
}

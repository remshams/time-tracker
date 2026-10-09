use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode};
use chrono::{DateTime, TimeDelta, Utc};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;
use tracker_domain::{Task, TaskId, TaskName};
use tracker_protocol::{
    InactiveTaskPreviewDto, MutationDto, MutationResultDto, TasksDto, TrackingDto,
};
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

async fn state(router: &axum::Router) -> (TasksDto, TrackingDto) {
    let (status, body) = call(router, Method::GET, "/v1/tasks", None).await;
    assert_eq!(status, StatusCode::OK);
    let tasks: TasksDto = serde_json::from_value(body).unwrap();
    let (status, body) = call(router, Method::GET, "/v1/tracking", None).await;
    assert_eq!(status, StatusCode::OK);
    let tracking: TrackingDto = serde_json::from_value(body).unwrap();
    assert_eq!(tasks.revision, tracking.revision);
    (tasks, tracking)
}

async fn create_fixture_task(router: &axum::Router, name: &str) -> String {
    let before = state(router).await;
    let id = Uuid::now_v7().to_string();
    let request = merge(
        json!({"task_id": id, "name": name, "occurred_at": at(0)}),
        guard(&before.0.revision),
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
        guard(&before.0.revision),
    );
    let (status, body) = call(&router, Method::POST, "/v1/tasks", Some(create)).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = call(&router, Method::GET, &preview_uri(as_of), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body.as_object().unwrap().len(), 5);
    assert!(body.get("candidate_fingerprint").is_none());
    let preview: InactiveTaskPreviewDto = serde_json::from_value(body).unwrap();
    assert_eq!(preview.count, 1);
    assert_eq!(preview.inactive_days, 14);
    assert_eq!(preview.candidate_task_ids.len(), 1);

    let request = archive_request(&preview);
    assert_eq!(request.as_object().unwrap().len(), 4);
    assert_eq!(request["expected_revision"], preview.revision);
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
    assert_eq!(replay["applied_revision"], first["applied_revision"]);
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
    assert_eq!(state(&router).await.0.revision, mutation.applied_revision);

    let archived_id = &state(&router).await.0.tasks[0].id;
    let restore = merge(
        json!({"action": "restore", "occurred_at": as_of}),
        guard(&mutation.applied_revision),
    );
    let (status, body) = call(
        &router,
        Method::PATCH,
        &format!("/v1/tasks/{archived_id}"),
        Some(restore),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let restored: MutationDto = serde_json::from_value(body).unwrap();
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
    assert_eq!(replay.applied_revision, mutation.applied_revision);
    assert!(replay.replayed);
    assert_ne!(replay.applied_revision, restored.applied_revision);
    assert_eq!(state(&router).await.0.revision, restored.applied_revision);
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
    assert_ne!(result.applied_revision, preview.revision);
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
    assert_eq!(health, json!({"status":"ok","protocol_version":3}));
    let before = state(&router).await;
    assert!(before.0.tasks.is_empty());
    let task_id = Uuid::now_v7().to_string();
    let request = merge(
        json!({"task_id":task_id,"name":"Write tests","occurred_at":at(100)}),
        guard(&before.0.revision),
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
    assert_eq!(replay["applied_revision"], first.applied_revision);
    assert_eq!(replay["replayed"], true);
    let mut different = request.clone();
    different["name"] = json!("Different");
    let (status, error) = call(&router, Method::POST, "/v1/tasks", Some(different)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["code"], "conflict");
    let stale = merge(
        json!({"task_id":Uuid::now_v7().to_string(),"name":"Stale","occurred_at":at(101)}),
        guard(&before.0.revision),
    );
    let (status, error) = call(&router, Method::POST, "/v1/tasks", Some(stale)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["code"], "stale_revision");
    assert_eq!(state(&router).await.0.tasks.len(), 1);
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
        guard(&initial.0.revision),
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
        guard(&resumed.0.revision),
    );
    let (status, body) = call(&router, Method::PUT, "/v1/tracking", Some(switch)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let switched: MutationDto = serde_json::from_value(body).unwrap();
    assert_eq!(
        state(&router).await.1.active_worklog.as_ref().unwrap().id,
        switch_id
    );
    let clear = merge(
        json!({"task_id":null,"worklog_id":null,"expected_active":switch_id,"occurred_at":at(200)}),
        guard(&switched.applied_revision),
    );
    let (status, body) = call(&router, Method::PUT, "/v1/tracking", Some(clear)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let stopped: MutationDto = serde_json::from_value(body).unwrap();
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
        guard(&stopped.applied_revision),
    );
    let route = format!("/v1/worklogs/{first_id}");
    let (status, body) = call(&router, Method::PATCH, &route, Some(correction)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let corrected: MutationDto = serde_json::from_value(body).unwrap();
    let movement = merge(
        json!({"action":"move","expected_task_id":first_task,"expected_start":at(90),"expected_end":at(140),"destination_task_id":second_task}),
        guard(&corrected.applied_revision),
    );
    let (status, body) = call(&router, Method::PATCH, &route, Some(movement)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let moved: MutationDto = serde_json::from_value(body).unwrap();
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
        guard(&moved.applied_revision),
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
    let initial = state(&router).await;
    let route = format!("/v1/tasks/{id}");
    let rename = merge(
        json!({"action":"rename","name":"Renamed task","occurred_at":at(100)}),
        guard(&initial.0.revision),
    );
    let (status, body) = call(&router, Method::PATCH, &route, Some(rename)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let renamed: MutationDto = serde_json::from_value(body).unwrap();
    let archive = merge(
        json!({"action":"archive","occurred_at":at(110)}),
        guard(&renamed.applied_revision),
    );
    let (status, body) = call(&router, Method::PATCH, &route, Some(archive)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let archived: MutationDto = serde_json::from_value(body).unwrap();
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
    let restore = merge(
        json!({"action":"restore","occurred_at":at(120)}),
        guard(&archived.applied_revision),
    );
    let (status, body) = call(&router, Method::PATCH, &route, Some(restore)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let restored: MutationDto = serde_json::from_value(body).unwrap();
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
    let large = json!({"task_id":Uuid::now_v7().to_string(),"name":"x".repeat(17_000),"occurred_at":at(100),"expected_revision":restored.applied_revision,"request_id":Uuid::now_v7().to_string()});
    let (status, _) = call(&router, Method::POST, "/v1/tasks", Some(large)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);

    let between_mutated_and_real_limit = json!({
        "task_id": Uuid::now_v7().to_string(),
        "name": "Below the server body limit",
        "occurred_at": at(130),
        "unexpected": "x".repeat(2_000),
        "expected_revision": restored.applied_revision,
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
        guard(&before.0.revision),
    );
    let second = merge(
        json!({"task_id":Uuid::now_v7().to_string(),"name":"Second","occurred_at":at(100)}),
        guard(&before.0.revision),
    );
    let (left, right) = tokio::join!(
        call(&router, Method::POST, "/v1/tasks", Some(first)),
        call(&router, Method::POST, "/v1/tasks", Some(second))
    );
    let statuses = [left.0, right.0];
    assert!(statuses.contains(&StatusCode::OK));
    assert!(statuses.contains(&StatusCode::CONFLICT));
    assert_eq!(state(&router).await.0.tasks.len(), 1);
}

#[tokio::test]
async fn invalid_json_does_not_change_state() {
    let temp = TempDir::new().unwrap();
    let router = tracker_server::router_for_database(&temp.path().join("tracker.db")).unwrap();
    let before = state(&router).await;
    let unknown = merge(
        json!({"task_id":Uuid::now_v7().to_string(),"name":"Input","occurred_at":at(100),"unexpected":"value"}),
        guard(&before.0.revision),
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
        guard(&before.0.revision),
    );
    let (status, body) = call(&router, Method::POST, "/v1/tasks", Some(invalid_name)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid_request");
    assert_eq!(state(&router).await, before);

    let missing_id = Uuid::now_v7().to_string();
    let change = merge(
        json!({"action":"rename","name":"Missing task","occurred_at":at(100)}),
        guard(&before.0.revision),
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
        format!("/v1/worklogs?task_id={task_id}&after_start={start}"),
        format!("/v1/worklogs?after_start={start}"),
    ] {
        let (status, body) = call(&router, Method::GET, &route, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{route}");
        assert_eq!(body["code"], "invalid_request", "{route}");
    }

    for route in [
        format!(
            "/v1/worklogs?task_id={task_id}&after_start={start}&after_id={worklog_id}&after_revision=-1"
        ),
        format!("/v1/worklogs?after_start={start}&after_id={worklog_id}&after_revision=-1"),
    ] {
        let (status, body) = call(&router, Method::GET, &route, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{route}");
        assert_eq!(body["code"], "invalid_request", "{route}");
    }

    for route in [
        format!(
            "/v1/worklogs?task_id={task_id}&after_start={start}&after_id={worklog_id}&after_revision=0&after_task_id={task_id}"
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
        guard(&before.0.revision),
    );

    let (status, first) = call(&router, Method::PUT, "/v1/tracking", Some(request.clone())).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["result"]["kind"], "tracking_already_idle");
    assert!(first.get("snapshot").is_none());
    assert!(state(&router).await.1.active_worklog.is_none());

    let (status, retry) = call(&router, Method::PUT, "/v1/tracking", Some(request)).await;
    assert_eq!(status, StatusCode::OK, "{retry}");
    assert_eq!(retry["result"], first["result"]);
    assert_eq!(retry["applied_revision"], first["applied_revision"]);
    assert_eq!(retry["replayed"], true);
    assert!(state(&router).await.0.tasks.is_empty());
}

#[tokio::test]
async fn a_new_server_database_stays_empty_after_restart() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("tracker.db");
    for _ in 0..2 {
        let router = tracker_server::router_for_database(&path).unwrap();
        let snapshot = state(&router).await;
        assert!(snapshot.0.tasks.is_empty());
        assert!(snapshot.1.active_worklog.is_none());
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
    assert_eq!(body.as_object().unwrap().len(), 5);
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
        guard(&preview.revision),
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
        guard(&preview.revision),
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
    let restore = merge(
        json!({"action":"restore", "occurred_at": as_of}),
        guard(&first.applied_revision),
    );
    let (status, body) = call(
        &router,
        Method::PATCH,
        &format!("/v1/tasks/{}", added.id()),
        Some(restore),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let restored: MutationDto = serde_json::from_value(body).unwrap();
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
    assert_eq!(replay.applied_revision, first.applied_revision);
    assert!(replay.replayed);
    assert_ne!(replay.applied_revision, restored.applied_revision);
    assert_eq!(state(&router).await.0.revision, restored.applied_revision);
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
    assert_eq!(state(&router).await.0.revision, restored.applied_revision);
}

#[tokio::test]
async fn configurable_archive_validates_days_time_and_revision_without_writing() {
    let directory = TempDir::new().unwrap();
    let router = tracker_server::router_for_database(&directory.path().join("tracker.db")).unwrap();
    create_fixture_task(&router, "Planning").await;
    let before = state(&router).await;
    let as_of = Utc::now();
    for days in [0, u32::MAX] {
        let (status, _) = call(&router, Method::GET, &candidates_uri(as_of, days), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let request = merge(
            json!({"as_of": as_of, "inactive_days": days}),
            guard(&before.0.revision),
        );
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
        let request = merge(
            json!({"as_of": invalid_time, "inactive_days": 7}),
            guard(&before.0.revision),
        );
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
    let request = merge(json!({"as_of": as_of, "inactive_days": 7}), guard("stale"));
    let (status, body) = call(
        &router,
        Method::POST,
        "/v1/tasks/archive-inactive",
        Some(request),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "stale_revision");
    assert_eq!(state(&router).await, before);
    for fields in [
        json!({"as_of":as_of}),
        json!({"as_of":as_of,"inactive_days":7,"unexpected":true}),
        json!({"as_of":as_of,"inactive_days":-1}),
    ] {
        let (status, _) = call(
            &router,
            Method::POST,
            "/v1/tasks/archive-inactive",
            Some(merge(fields, guard(&before.0.revision))),
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
    assert_keys(&tasks, &["tasks", "revision"]);
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
        assert_eq!(filtered["revision"], tasks["revision"]);
        assert_eq!(filtered["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(filtered["tasks"][0]["id"], id.to_string());
    }
    for (index, id) in [(0, archived_task.id()), (1, active_task.id())] {
        let (status, task) = call(&router, Method::GET, &format!("/v1/tasks/{id}"), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_keys(&task, &["task", "revision"]);
        assert_eq!(task["task"], tasks["tasks"][index]);
        assert_eq!(task["revision"], tasks["revision"]);
    }
    let (status, tracking) = call(&router, Method::GET, "/v1/tracking", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_keys(&tracking, &["active_worklog", "revision"]);
    assert_eq!(tracking["active_worklog"]["id"], running.id().to_string());
    assert_eq!(tracking["revision"], tasks["revision"]);
    let (status, worklog) = call(
        &router,
        Method::GET,
        &format!("/v1/worklogs/{}", running.id()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_keys(&worklog, &["worklog", "revision"]);
    assert_keys(&worklog["worklog"], &["id", "task_id", "start", "end"]);
    assert_eq!(worklog["worklog"], tracking["active_worklog"]);
    assert_eq!(worklog["revision"], tasks["revision"]);
    let (status, history) = call(
        &router,
        Method::GET,
        &format!("/v1/worklogs?task_id={}", active_task.id()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_keys(&history, &["worklogs", "next_cursor", "revision"]);
    assert_eq!(history["worklogs"][0], tracking["active_worklog"]);
    assert_eq!(history["revision"], tasks["revision"]);
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
    assert_keys(
        &report,
        &["start", "end", "now", "rows", "total_us", "revision"],
    );
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
    let before = state(&router).await;
    let task_id = Uuid::now_v7().to_string();
    let request = merge(
        json!({"task_id":task_id,"name":"Original project","occurred_at":at(100)}),
        guard(&before.0.revision),
    );
    let (status, receipt) = call(&router, Method::POST, "/v1/tasks", Some(request.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert_keys(
        &receipt,
        &["request_id", "applied_revision", "replayed", "result"],
    );
    assert_eq!(receipt["request_id"], request["request_id"]);
    assert_eq!(receipt["replayed"], false);
    let rename = merge(
        json!({"action":"rename","name":"New project","occurred_at":at(200)}),
        guard(receipt["applied_revision"].as_str().unwrap()),
    );
    let (status, renamed) = call(
        &router,
        Method::PATCH,
        &format!("/v1/tasks/{task_id}"),
        Some(rename),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(receipt["applied_revision"], renamed["applied_revision"]);
    let (status, replay) = call(&router, Method::POST, "/v1/tasks", Some(request.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert_keys(
        &replay,
        &["request_id", "applied_revision", "replayed", "result"],
    );
    assert_eq!(replay["request_id"], receipt["request_id"]);
    assert_eq!(replay["applied_revision"], receipt["applied_revision"]);
    assert_eq!(replay["result"], receipt["result"]);
    assert_eq!(replay["result"]["value"]["name"], "Original project");
    assert_eq!(replay["replayed"], true);
    assert_eq!(state(&router).await.0.tasks[0].name, "New project");
    drop(router);
    let router = tracker_server::router_for_database(&database).unwrap();
    let (status, error) = call(&router, Method::POST, "/v1/tasks", Some(request)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error["code"], "stale_revision");
    let mut invalid_request = merge(
        json!({"task_id":Uuid::now_v7().to_string(),"name":"Invalid receipt","occurred_at":at(100)}),
        guard(&state(&router).await.0.revision),
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
    assert_keys(&global, &["worklogs", "next_cursor", "revision"]);
    assert_eq!(filtered["worklogs"].as_array().unwrap().len(), 50);
    assert_eq!(global["worklogs"].as_array().unwrap().len(), 50);
    let cursor = &filtered["next_cursor"];
    assert_keys(cursor, &["task_id", "start", "id", "revision"]);
    assert_eq!(cursor["task_id"], first.id().to_string());
    assert!(global["next_cursor"]["task_id"].is_null());
    let continuation = format!(
        "after_start={}&after_id={}&after_revision={}",
        cursor["start"].as_str().unwrap(),
        cursor["id"].as_str().unwrap(),
        cursor["revision"]
    );
    let valid_filtered = format!(
        "/v1/worklogs?task_id={}&after_task_id={}&{continuation}",
        first.id(),
        first.id()
    );
    let (_, next) = call(&router, Method::GET, &valid_filtered, None).await;
    assert_eq!(next["worklogs"].as_array().unwrap().len(), 2);
    assert!(next["next_cursor"].is_null());
    assert!(
        next["worklogs"][0]["start"].as_str().unwrap()
            < filtered["worklogs"][49]["start"].as_str().unwrap()
    );
    let global_cursor = &global["next_cursor"];
    let global_continuation = format!(
        "after_start={}&after_id={}&after_revision={}",
        global_cursor["start"].as_str().unwrap(),
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
        format!("/v1/worklogs?task_id={}&{continuation}", first.id()),
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
        guard(filtered["revision"].as_str().unwrap()),
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

use super::commands::command;

#[test]
fn confirmation_keeps_the_preview_window_when_other_tasks_age_into_eligibility() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tt.db");
    let old = command(
        &path,
        "2026-01-01T00:00:00Z",
        &["tasks", "create", "Old task"],
    )
    .unwrap();
    let younger = command(
        &path,
        "2026-01-20T00:00:00Z",
        &["tasks", "create", "Younger task"],
    )
    .unwrap();
    let preview_at = "2026-02-01T00:00:00Z";
    let preview = command(&path, preview_at, &["tasks", "preview-inactive"]).unwrap();
    assert_eq!(preview["count"], 1);
    assert_eq!(preview["tasks"][0]["id"], old["id"]);
    let token = preview["preview"].to_string();
    let confirmation_at = "2026-02-05T00:00:00Z";
    let archived = command(
        &path,
        confirmation_at,
        &["tasks", "archive-inactive", "--preview", &token, "--yes"],
    )
    .unwrap();
    assert_eq!(archived["archived_count"], 1);
    let task = command(
        &path,
        confirmation_at,
        &["tasks", "get", old["id"].as_str().unwrap()],
    )
    .unwrap();
    assert_eq!(task["archived"], true);
    assert_eq!(task["updated_at"], preview_at);
    assert_eq!(
        command(
            &path,
            confirmation_at,
            &["tasks", "get", younger["id"].as_str().unwrap()],
        )
        .unwrap()["archived"],
        false
    );
}

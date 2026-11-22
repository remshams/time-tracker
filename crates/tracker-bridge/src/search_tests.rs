use super::*;
use crate::test_support::*;

fn search(bridge: &mut Bridge, query: &str) -> Value {
    let query = CString::new(query).unwrap();
    // SAFETY: The bridge and string remain live, with exclusive bridge access.
    response(unsafe { tt_bridge_search_tasks(bridge, query.as_ptr()) })
}

#[test]
fn search_returns_active_task_ids_from_the_cached_catalog_in_rank_order() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("search.db");
    let mut bridge = open_fixture(&path).unwrap();
    let tasks = bridge.application.tasks(TaskOrdering::default());
    archive_cached_task(&mut bridge.application, tasks[2].task.id(), at(0)).unwrap();
    let expected = bridge
        .application
        .tasks(TaskOrdering::RecentlyWorked)
        .into_iter()
        .filter(|item| !item.task.is_archived())
        .map(|item| item.task.id().to_string())
        .collect::<Vec<_>>();
    assert_eq!(expected.len(), tasks.len() - 1);
    assert_eq!(search(&mut bridge, "")["data"], json!(expected));
    let name = tasks[1].task.name().as_str().to_uppercase();
    assert_eq!(
        search(&mut bridge, &name)["data"],
        json!([tasks[1].task.id().to_string()])
    );
    assert_eq!(search(&mut bridge, "missing")["data"], json!([]));

    let mut other = open_at(&path).unwrap();
    other
        .application
        .create_task(TaskName::new("Fresh task").unwrap(), at(1))
        .unwrap_or_else(|error| panic!("{}", error.message));
    assert_eq!(search(&mut bridge, "")["data"], json!(expected));
    bridge
        .application
        .refresh_resources()
        .unwrap_or_else(|error| panic!("{}", error.message));
    assert_eq!(
        search(&mut bridge, "fresh")["data"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn search_rejects_null_and_non_utf8_queries_and_a_missing_bridge() {
    let directory = tempfile::tempdir().unwrap();
    let mut bridge = open_fixture(&directory.path().join("inputs.db")).unwrap();
    let non_utf8 = CString::new(vec![255]).unwrap();
    // SAFETY: All non-null pointers reference live C strings, with exclusive bridge access.
    unsafe {
        for invalid in [ptr::null(), non_utf8.as_ptr()] {
            assert_eq!(
                response(tt_bridge_search_tasks(&mut bridge, invalid))["error"],
                "Invalid task search query"
            );
        }
        let query = CString::new("").unwrap();
        assert_eq!(
            response(tt_bridge_search_tasks(ptr::null_mut(), query.as_ptr()))["error"],
            "Database is not open"
        );
    }
}

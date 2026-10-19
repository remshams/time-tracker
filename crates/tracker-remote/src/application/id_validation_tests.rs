use super::*;
#[test]
fn report_and_preview_ids_are_unique_after_uuid_parsing() {
    let id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    let at = DateTime::UNIX_EPOCH;
    let report = ReportDto {
        start: at,
        end: at + TimeDelta::seconds(10),
        now: at,
        rows: vec![
            tracker_protocol::ReportRowDto {
                task_id: id.to_owned(),
                duration_us: 1,
            },
            tracker_protocol::ReportRowDto {
                task_id: id.to_ascii_uppercase(),
                duration_us: 1,
            },
        ],
        total_us: 2,
    };
    assert!(validate_report(&report).is_err());
    let preview = InactiveTaskPreviewDto {
        as_of: at,
        inactive_days: 14,
        count: 2,
        candidate_task_ids: vec![id.to_owned(), id.to_ascii_uppercase()],
    };
    assert!(validate_inactive_preview(&preview).is_err());
}

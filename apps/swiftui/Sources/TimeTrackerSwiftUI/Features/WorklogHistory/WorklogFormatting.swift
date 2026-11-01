import Foundation
import TrackerClient

func localTimestamp(_ value: String) -> String {
    guard let date = parseTimestamp(value) else { return value }
    return date.formatted(date: .omitted, time: .shortened)
}

func localDay(_ value: String) -> String {
    guard let date = parseTimestamp(value) else { return value }
    return date.formatted(date: .abbreviated, time: .omitted)
}

func worklogDuration(_ worklog: WorklogItem) -> TimeInterval {
    guard let start = parseTimestamp(worklog.start), let end = parseTimestamp(worklog.end) else { return 0 }
    return max(0, end.timeIntervalSince(start))
}

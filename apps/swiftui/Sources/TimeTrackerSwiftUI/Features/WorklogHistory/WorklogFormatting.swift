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

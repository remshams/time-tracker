import AppKit
import XCTest

@MainActor
extension TrackerUITestCase {
    @discardableResult
    func completedToday(_ task: FixtureTask, duration: TimeInterval = 61) throws -> FixtureWorklog {
        let now = Date()
        let startOfDay = Calendar.current.startOfDay(for: now)
        let available = now.timeIntervalSince(startOfDay)
        let end = now.addingTimeInterval(-min(60, available / 4))
        return try fixture.completed(task, start: end.addingTimeInterval(-min(duration, available / 2)), end: end)
    }

    func exactTodayDuration(_ task: FixtureTask) throws -> String {
        let now = Date()
        let day = try XCTUnwrap(Calendar.current.dateInterval(of: .day, for: now))
        let report = try fixture.reports(start: day.start, end: day.end, at: now)
        let seconds = (report.rows.first { $0.task.id == task.id }?.durationMicroseconds ?? 0) / 1_000_000
        if seconds >= 3600 { return "\(seconds / 3600)h \(seconds / 60 % 60)m \(seconds % 60)s" }
        if seconds >= 60 { return "\(seconds / 60)m \(seconds % 60)s" }
        return "\(seconds)s"
    }
}

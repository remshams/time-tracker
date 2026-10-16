import Foundation
import TrackerClient

@MainActor
struct SystemTrackerClock: TrackerClock {
    var now: Date { Date() }
    var uptime: TimeInterval { ProcessInfo.processInfo.systemUptime }
}

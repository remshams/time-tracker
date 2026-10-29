import Foundation
import TrackerClient

struct SystemTrackerClock: TrackerClock {
    var now: Date { Date() }
    var uptime: TimeInterval { ProcessInfo.processInfo.systemUptime }
}

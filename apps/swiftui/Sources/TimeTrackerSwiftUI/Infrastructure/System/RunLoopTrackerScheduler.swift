import Foundation
import TrackerClient

@MainActor
struct RunLoopTrackerScheduler: TrackerScheduler {
    func schedule(
        after interval: TimeInterval, repeating: Bool, tolerance: TimeInterval,
        action: @escaping @MainActor () -> Void
    ) -> any TrackerCancellation {
        let timer = Timer(timeInterval: interval, repeats: repeating) { _ in
            MainActor.assumeIsolated { action() }
        }
        timer.tolerance = tolerance
        RunLoop.main.add(timer, forMode: .common)
        return TimerCancellation(timer: timer)
    }
}

private final class TimerCancellation: TrackerCancellation {
    private var timer: Timer?
    init(timer: Timer) { self.timer = timer }
    func cancel() { timer?.invalidate(); timer = nil }
    deinit { timer?.invalidate() }
}

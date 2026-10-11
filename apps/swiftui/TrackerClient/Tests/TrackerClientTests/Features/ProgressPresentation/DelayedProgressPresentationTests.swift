import XCTest
@testable import TrackerClient

@MainActor
final class DelayedProgressPresentationTests: XCTestCase {
    func testFastOperationNeverShowsProgress() async {
        let clock = ProgressClock()
        let progress = DelayedProgressPresentation(sleep: { try await clock.sleep($0) })
        var values: [Bool] = []
        progress.update(isBusy: true) { values.append($0) }
        await clock.waitForRequests(1)
        XCTAssertFalse(values.contains(true))
        progress.update(isBusy: false) { values.append($0) }
        await clock.release(0)
        XCTAssertEqual(clock.cancelled[0], true)
        XCTAssertFalse(values.contains(true))
    }

    func testBusyOperationShowsOnlyAfterThePresentationDelay() async {
        let clock = ProgressClock()
        let progress = DelayedProgressPresentation(sleep: { try await clock.sleep($0) })
        var visible = false
        progress.update(isBusy: true) { visible = $0 }
        await clock.waitForRequests(1)
        XCTAssertEqual(clock.delays, [.milliseconds(180)])
        XCTAssertFalse(visible)
        await clock.release(0)
        XCTAssertTrue(visible)
        progress.update(isBusy: false) { visible = $0 }
        XCTAssertFalse(visible)
    }

    func testCancelledAndReplacedRequestCannotRevealItsOldIndicator() async {
        let clock = ProgressClock()
        let progress = DelayedProgressPresentation(sleep: { try await clock.sleep($0) })
        var visible = false
        progress.update(isBusy: true) { visible = $0 }
        await clock.waitForRequests(1)
        progress.update(isBusy: false) { visible = $0 }
        progress.update(isBusy: true) { visible = $0 }
        await clock.waitForRequests(2)
        await clock.release(0)
        XCTAssertEqual(clock.cancelled[0], true)
        XCTAssertFalse(visible)
        await clock.release(1)
        XCTAssertTrue(visible)
        progress.cancel()
    }

    func testRepeatedBusyUpdateDoesNotRestartTheDelay() async {
        let clock = ProgressClock()
        let progress = DelayedProgressPresentation(sleep: { try await clock.sleep($0) })
        var visible = false
        progress.update(isBusy: true) { visible = $0 }
        await clock.waitForRequests(1)
        progress.update(isBusy: true) { visible = $0 }
        await clock.release(0)
        XCTAssertEqual(clock.delays.count, 1)
        XCTAssertTrue(visible)
        progress.cancel()
    }

    func testDismissalCancelsPendingIndicatorAndReopeningStartsNewDelay() async {
        let clock = ProgressClock()
        let progress = DelayedProgressPresentation(sleep: { try await clock.sleep($0) })
        var visible = false
        progress.update(isBusy: true) { visible = $0 }
        await clock.waitForRequests(1)
        progress.cancel()
        await clock.release(0)
        XCTAssertEqual(clock.cancelled[0], true)
        XCTAssertFalse(visible)
        progress.update(isBusy: true) { visible = $0 }
        await clock.waitForRequests(2)
        XCTAssertFalse(visible)
        await clock.release(1)
        XCTAssertTrue(visible)
        progress.cancel()
    }

    func testDisposalCancelsPendingIndicatorWithoutRetainingPresentation() async {
        let clock = ProgressClock()
        var progress: DelayedProgressPresentation? = DelayedProgressPresentation(sleep: { try await clock.sleep($0) })
        weak var weakProgress = progress
        var visible = false
        progress?.update(isBusy: true) { visible = $0 }
        await clock.waitForRequests(1)
        progress = nil
        XCTAssertNil(weakProgress)
        await clock.release(0)
        XCTAssertEqual(clock.cancelled[0], true)
        XCTAssertFalse(visible)
    }
}

@MainActor
private final class ProgressClock {
    private var pending: [Int: CheckedContinuation<Void, Never>] = [:]
    private(set) var delays: [Duration] = []
    private(set) var cancelled: [Int: Bool] = [:]

    func sleep(_ delay: Duration) async throws {
        let index = delays.count
        delays.append(delay)
        await withCheckedContinuation { pending[index] = $0 }
        cancelled[index] = Task.isCancelled
    }

    func waitForRequests(_ count: Int) async {
        for _ in 0..<1000 {
            if delays.count >= count { return }
            await Task.yield()
        }
        XCTFail("Expected \(count) scheduled presentation delays, got \(delays.count).")
    }

    func release(_ index: Int) async {
        pending.removeValue(forKey: index)?.resume()
        for _ in 0..<20 { await Task.yield() }
    }

}

import XCTest
@testable import TrackerClient

@MainActor
final class DelayedProgressPresentationTests: XCTestCase {
    func testFastOperationNeverShowsProgress() async {
        let clock = ProgressClock()
        let progress = DelayedProgressPresentation(sleep: { try await clock.sleep($0) })
        var values: [Bool] = []
        let shown = expectation(description: "Fast operation stays hidden")
        shown.isInverted = true
        progress.update(isBusy: true) {
            values.append($0); if $0 { shown.fulfill() }
        }
        await clock.waitForRequests(1)
        XCTAssertFalse(values.contains(true))
        progress.update(isBusy: false) { values.append($0) }
        await clock.release(0)
        XCTAssertEqual(clock.cancelled[0], true)
        await fulfillment(of: [shown], timeout: 0.1)
        XCTAssertFalse(values.contains(true))
    }

    func testBusyOperationShowsOnlyAfterThePresentationDelay() async {
        let clock = ProgressClock()
        let progress = DelayedProgressPresentation(sleep: { try await clock.sleep($0) })
        var visible = false
        let shown = expectation(description: "Busy operation becomes visible")
        progress.update(isBusy: true) {
            visible = $0; if $0 { shown.fulfill() }
        }
        await clock.waitForRequests(1)
        XCTAssertEqual(clock.delays, [.milliseconds(180)])
        XCTAssertFalse(visible)
        await clock.release(0)
        await fulfillment(of: [shown], timeout: 1)
        XCTAssertTrue(visible)
        progress.update(isBusy: false) { visible = $0 }
        XCTAssertFalse(visible)
    }

    func testCancelledAndReplacedRequestCannotRevealItsOldIndicator() async {
        let clock = ProgressClock()
        let progress = DelayedProgressPresentation(sleep: { try await clock.sleep($0) })
        var visible = false
        let stale = expectation(description: "Replaced request stays hidden")
        stale.isInverted = true
        let shown = expectation(description: "Replacement request becomes visible")
        progress.update(isBusy: true) {
            visible = $0; if $0 { stale.fulfill() }
        }
        await clock.waitForRequests(1)
        progress.update(isBusy: false) { visible = $0 }
        progress.update(isBusy: true) {
            visible = $0; if $0 { shown.fulfill() }
        }
        await clock.waitForRequests(2)
        await clock.release(0)
        XCTAssertEqual(clock.cancelled[0], true)
        await fulfillment(of: [stale], timeout: 0.1)
        XCTAssertFalse(visible)
        await clock.release(1)
        await fulfillment(of: [shown], timeout: 1)
        XCTAssertTrue(visible)
        progress.cancel()
    }

    func testRepeatedBusyUpdateDoesNotRestartTheDelay() async {
        let clock = ProgressClock()
        let progress = DelayedProgressPresentation(sleep: { try await clock.sleep($0) })
        var visible = false
        let shown = expectation(description: "Busy operation becomes visible")
        progress.update(isBusy: true) {
            visible = $0; if $0 { shown.fulfill() }
        }
        await clock.waitForRequests(1)
        progress.update(isBusy: true) { visible = $0 }
        await clock.release(0)
        XCTAssertEqual(clock.delays.count, 1)
        await fulfillment(of: [shown], timeout: 1)
        XCTAssertTrue(visible)
        progress.cancel()
    }

    func testDismissalCancelsPendingIndicatorAndReopeningStartsNewDelay() async {
        let clock = ProgressClock()
        let progress = DelayedProgressPresentation(sleep: { try await clock.sleep($0) })
        var visible = false
        let cancelled = expectation(description: "Dismissed request stays hidden")
        cancelled.isInverted = true
        let shown = expectation(description: "Reopened request becomes visible")
        progress.update(isBusy: true) {
            visible = $0; if $0 { cancelled.fulfill() }
        }
        await clock.waitForRequests(1)
        progress.cancel()
        await clock.release(0)
        XCTAssertEqual(clock.cancelled[0], true)
        await fulfillment(of: [cancelled], timeout: 0.1)
        XCTAssertFalse(visible)
        progress.update(isBusy: true) {
            visible = $0; if $0 { shown.fulfill() }
        }
        await clock.waitForRequests(2)
        XCTAssertFalse(visible)
        await clock.release(1)
        await fulfillment(of: [shown], timeout: 1)
        XCTAssertTrue(visible)
        progress.cancel()
    }

    func testDisposalCancelsPendingIndicatorWithoutRetainingPresentation() async {
        let clock = ProgressClock()
        var progress: DelayedProgressPresentation? = DelayedProgressPresentation(sleep: { try await clock.sleep($0) })
        weak var weakProgress = progress
        var visible = false
        let shown = expectation(description: "Disposed presentation stays hidden")
        shown.isInverted = true
        progress?.update(isBusy: true) {
            visible = $0; if $0 { shown.fulfill() }
        }
        await clock.waitForRequests(1)
        progress = nil
        XCTAssertNil(weakProgress)
        await clock.release(0)
        XCTAssertEqual(clock.cancelled[0], true)
        await fulfillment(of: [shown], timeout: 0.1)
        XCTAssertFalse(visible)
    }
}

@MainActor
private final class ProgressClock {
    private var pending: [Int: CheckedContinuation<Void, Never>] = [:]
    private var resumed: [Int: XCTestExpectation] = [:]
    private var requested: [(Int, XCTestExpectation)] = []
    private(set) var delays: [Duration] = []
    private(set) var cancelled: [Int: Bool] = [:]

    func sleep(_ delay: Duration) async throws {
        let index = delays.count
        resumed[index] = XCTestExpectation(description: "Delay \(index) resumed")
        delays.append(delay)
        await withCheckedContinuation { continuation in
            pending[index] = continuation
            requested.filter { $0.0 <= delays.count }.forEach { $0.1.fulfill() }
            requested.removeAll { $0.0 <= delays.count }
        }
        cancelled[index] = Task.isCancelled
        resumed[index]?.fulfill()
    }

    func waitForRequests(_ count: Int) async {
        if delays.count >= count { return }
        let scheduled = XCTestExpectation(description: "Scheduled \(count) presentation delays")
        requested.append((count, scheduled))
        let result = await XCTWaiter.fulfillment(of: [scheduled], timeout: 1)
        XCTAssertEqual(result, .completed)
    }

    func release(_ index: Int) async {
        guard let continuation = pending.removeValue(forKey: index), let resumed = resumed[index] else {
            XCTFail("No pending presentation delay at index \(index).")
            return
        }
        continuation.resume()
        let result = await XCTWaiter.fulfillment(of: [resumed], timeout: 1)
        XCTAssertEqual(result, .completed)
    }
}

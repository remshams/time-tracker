import Foundation
import XCTest
@testable import TrackerClient

@MainActor
final class TrackingAutomationTests: XCTestCase {
    private let running = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
    private let idle = TaskListResources(tasks: [firstTask, secondTask], active: nil)

    private func finishHistory(_ fixture: Fixture) async throws {
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        history.succeed(emptyPage)
    }

    private func acknowledgePause(_ fixture: Fixture, at date: Date? = nil) async throws {
        fixture.trackingAutomation.setScreenLocked(true, at: date ?? fixture.clock.now)
        let pause = try await fixture.client.next()
        XCTAssertEqual(
            pause.operation,
            .pause(
                worklog: activeWorklog.id,
                at: formatTimestamp(date ?? fixture.clock.now)))
        pause.paused(idle)
        try await finishHistory(fixture)
        try await fixture.settled()
    }

    func testPreferenceDefaultsOffAndPersistsOnlyChanges() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(running)
        XCTAssertFalse(fixture.trackingAutomation.enabled)
        fixture.trackingAutomation.setScreenLocked(true, at: fixture.clock.now)
        fixture.trackingAutomation.setScreenLocked(false, at: fixture.clock.now)
        XCTAssertEqual(fixture.client.operations.count, 2)
        fixture.trackingAutomation.setPauseOnScreenLock(true)
        XCTAssertFalse(fixture.client.hasPendingRequests)
        fixture.trackingAutomation.setPauseOnScreenLock(true)
        fixture.trackingAutomation.setPauseOnScreenLock(false)
        XCTAssertEqual(
            fixture.preferences.writes, [TrackingPreferences(pauseOnScreenLock: true), TrackingPreferences()])
        XCTAssertNil(fixture.trackingAutomation.statusText)
    }

    func testPauseAndResumeUseEventTimestampsAndSameTask() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        let lock = fixture.clock.now
        try await acknowledgePause(fixture, at: lock)
        XCTAssertEqual(fixture.trackingAutomation.statusText, "Paused while screen is locked")
        fixture.clock.now.addTimeInterval(90)
        let unlock = fixture.clock.now
        fixture.trackingAutomation.setScreenLocked(false, at: unlock)
        fixture.clock.now.addTimeInterval(10)
        let resume = try await fixture.client.next()
        XCTAssertEqual(resume.operation, .resume(task: firstTask.id, at: formatTimestamp(unlock)))
        resume.succeed(running)
        try await finishHistory(fixture)
        try await fixture.settled()
        XCTAssertNil(fixture.trackingAutomation.statusText)
    }

    func testTurningThePreferenceOffClearsTheShownPausedStatus() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        try await acknowledgePause(fixture)
        XCTAssertEqual(fixture.trackingAutomation.statusText, "Paused while screen is locked")
        fixture.trackingAutomation.setPauseOnScreenLock(false)
        XCTAssertNil(fixture.trackingAutomation.statusText)
    }

    func testIdleLockNeverStartsTrackingOnUnlock() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(emptyTaskListResources)
        fixture.trackingAutomation.setScreenLocked(true, at: fixture.clock.now)
        fixture.trackingAutomation.setScreenLocked(false, at: fixture.clock.now)
        XCTAssertEqual(fixture.client.operations, [.open(.local)])
        XCTAssertNil(fixture.trackingAutomation.statusText)
    }

    func testUnlockDuringPauseResumesAfterConfirmationWithTheUnlockTime() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.trackingAutomation.setScreenLocked(true, at: fixture.clock.now)
        let pause = try await fixture.client.next()
        fixture.clock.now.addTimeInterval(90)
        let unlockedAt = fixture.clock.now
        fixture.trackingAutomation.setScreenLocked(false, at: unlockedAt)
        fixture.trackingAutomation.setScreenLocked(false, at: unlockedAt)
        fixture.clock.now.addTimeInterval(30)
        pause.paused(idle)
        let resume = try await fixture.client.next()
        XCTAssertEqual(resume.operation, .resume(task: firstTask.id, at: formatTimestamp(unlockedAt)))
        resume.succeed(running)
        try await fixture.settled()
        XCTAssertEqual(fixture.features.tracking.active, activeWorklog)
        XCTAssertNil(fixture.features.tracking.recordedError)
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .resume = $0 { return true }; return false
            }.count, 1)
    }

    func testPausingClearsAnEarlierTrackingError() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.features.tracking.startTracking(taskID: secondTask.id)
        try await fixture.client.next().fail(BridgeFailure(message: "Start rejected"))
        try await fixture.client.next().succeed(running)
        try await fixture.settled()
        XCTAssertEqual(fixture.features.tracking.recordedError, "Start rejected")

        try await acknowledgePause(fixture)
        XCTAssertNil(fixture.features.tracking.recordedError)
        XCTAssertNil(fixture.features.tracking.error)
    }

    func testUnlockDuringUnsuccessfulPauseNeverResumes() async throws {
        for didFail in [false, true] {
            let fixture = Fixture(pauseOnScreenLock: true)
            defer { fixture.cleanup() }
            try await fixture.start(running)
            fixture.trackingAutomation.setScreenLocked(true, at: fixture.clock.now)
            let pause = try await fixture.client.next()
            fixture.trackingAutomation.setScreenLocked(false, at: fixture.clock.now)
            if didFail {
                pause.fail(BridgeFailure(message: "Pause rejected"))
                let reload = try await fixture.client.next()
                XCTAssertEqual(reload.operation, .refresh(.local))
                reload.succeed(running)
            } else {
                pause.paused(running, didStop: false)
            }
            try await fixture.settled()
            XCTAssertFalse(
                fixture.client.operations.contains {
                    if case .resume = $0 { return true }; return false
                })
            XCTAssertEqual(fixture.features.tracking.recordedError, didFail ? "Pause rejected" : nil)
        }
    }

    func testDisablingThePreferenceWhileLockedNeverPausesARunningTask() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(idle)
        fixture.trackingAutomation.setScreenLocked(true, at: fixture.clock.now)
        fixture.sync.refresh()
        let startedElsewhere = try await fixture.client.next()
        XCTAssertEqual(startedElsewhere.operation, .refresh(.local))
        startedElsewhere.succeed(running)
        try await finishHistory(fixture)
        try await fixture.settled()
        XCTAssertEqual(fixture.features.tracking.active, activeWorklog)

        fixture.trackingAutomation.setPauseOnScreenLock(false)
        fixture.sync.refresh()
        let next = try await fixture.client.next()
        XCTAssertEqual(next.operation, .refresh(.local), "Disabling the preference must not pause tracking.")
        next.succeed(running)
        try await fixture.settled()
        XCTAssertEqual(fixture.features.tracking.active, activeWorklog)
    }

    func testManualStopAfterUnlockDuringPauseCancelsTheAutomaticResume() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.trackingAutomation.setScreenLocked(true, at: fixture.clock.now)
        let pause = try await fixture.client.next()
        fixture.trackingAutomation.setScreenLocked(false, at: fixture.clock.now)
        fixture.features.tracking.stopTracking(worklogID: activeWorklog.id)
        let stop = try await fixture.client.next()
        XCTAssertEqual(stop.operation, .stop(worklog: activeWorklog.id, at: formatTimestamp(fixture.clock.now)))
        pause.paused(idle)
        try await finishHistory(fixture)
        stop.succeed(idle)
        try await fixture.settled()
        XCTAssertNil(fixture.features.tracking.active)
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .resume = $0 { return true }; return false
            }, "A manual stop must win over the automatic resume.")
    }

    func testCancelledPendingPauseCannotGrantResumeAfterUnlock() {
        let store = TrackingAutomationStore(
            repository: MemoryTrackingPreferences(TrackingPreferences(pauseOnScreenLock: true)))
        XCTAssertTrue(store.lock())
        XCTAssertTrue(store.beginPause())
        XCTAssertNil(store.unlock(at: Date(timeIntervalSince1970: 90)))
        store.cancel()
        XCTAssertNil(store.paused(taskID: firstTask.id, didStop: true))
        XCTAssertNil(store.pausedStatusText)
    }

    func testRelockingDuringPauseKeepsTheLatestUnlockTime() {
        let store = TrackingAutomationStore(
            repository: MemoryTrackingPreferences(TrackingPreferences(pauseOnScreenLock: true)))
        XCTAssertTrue(store.lock())
        XCTAssertTrue(store.beginPause())
        XCTAssertNil(store.unlock(at: Date(timeIntervalSince1970: 90)))
        XCTAssertTrue(store.lock())
        XCTAssertFalse(store.beginPause())
        XCTAssertNil(store.paused(taskID: firstTask.id, didStop: true))
        XCTAssertEqual(store.pausedStatusText, "Paused while screen is locked")
        let latestUnlock = Date(timeIntervalSince1970: 180)
        let resume = store.unlock(at: latestUnlock)
        XCTAssertEqual(resume?.taskID, firstTask.id)
        XCTAssertEqual(resume?.date, latestUnlock)
    }

    func testNewWorklogAfterLockIsNotPaused() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(idle)
        let lock = fixture.clock.now
        fixture.sync.refresh()
        let taskListRequest = try await fixture.client.next()
        fixture.trackingAutomation.setScreenLocked(true, at: lock)
        let later = WorklogItem(
            id: "later-worklog", taskId: firstTask.id,
            start: formatTimestamp(lock.addingTimeInterval(1)), end: nil)
        taskListRequest.succeed(TaskListResources(tasks: idle.catalog.value, active: later))
        try await finishHistory(fixture)
        try await fixture.settled()
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .pause = $0 { return true }; return false
            })
    }

    func testAlreadyIdlePauseDoesNotGrantResumeOwnership() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.trackingAutomation.setScreenLocked(true, at: fixture.clock.now)
        let pause = try await fixture.client.next()
        pause.paused(idle, didStop: false)
        try await finishHistory(fixture)
        try await fixture.settled()
        fixture.trackingAutomation.setScreenLocked(false, at: fixture.clock.now)
        XCTAssertNil(fixture.trackingAutomation.statusText)
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .resume = $0 { return true }; return false
            })
    }

    func testDisableWhilePauseIsInflightAcceptsResourcesWithoutResuming() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.trackingAutomation.setScreenLocked(true, at: fixture.clock.now)
        let pause = try await fixture.client.next()
        fixture.trackingAutomation.setPauseOnScreenLock(false)
        fixture.trackingAutomation.setScreenLocked(false, at: fixture.clock.now)
        pause.paused(idle)
        try await finishHistory(fixture)
        try await fixture.settled()
        XCTAssertNil(fixture.features.tracking.active)
        XCTAssertNil(fixture.trackingAutomation.statusText)
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .resume = $0 { return true }; return false
            })
    }

    func testEnableWhileLockedUsesEnableTime() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.trackingAutomation.setScreenLocked(true, at: fixture.clock.now)
        fixture.clock.now.addTimeInterval(10)
        fixture.trackingAutomation.setPauseOnScreenLock(true)
        let pause = try await fixture.client.next()
        XCTAssertEqual(pause.operation, .pause(worklog: activeWorklog.id, at: formatTimestamp(fixture.clock.now)))
        pause.paused(idle)
        try await finishHistory(fixture)
        try await fixture.settled()
    }

    func testPauseFailureReloadsAndNeverResumes() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.trackingAutomation.setScreenLocked(true, at: fixture.clock.now)
        let pause = try await fixture.client.next()
        pause.fail(BridgeFailure(message: "Response lost", uncertain: true))
        let reconcile = try await fixture.client.next()
        XCTAssertEqual(reconcile.operation, .refresh(fixture.features.connection.settings))
        reconcile.succeed(idle)
        try await finishHistory(fixture)
        try await fixture.settled()
        fixture.trackingAutomation.setScreenLocked(false, at: fixture.clock.now)
        XCTAssertEqual(
            fixture.features.tracking.recordedError,
            "Could not confirm the change. Check the reloaded data before trying again. Response lost")
        XCTAssertNil(fixture.trackingAutomation.statusText)
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .resume = $0 { return true }; return false
            })
    }

    func testPauseFailureShowsItsErrorWhenTheFollowingReloadAlsoFails() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: activeWorklog))
        fixture.trackingAutomation.setScreenLocked(true, at: fixture.clock.now)
        try await fixture.client.next().fail(BridgeFailure(message: "Pause rejected"))
        try await fixture.client.next().fail(BridgeFailure(message: "Server offline", kind: "unavailable"))
        try await fixture.settled()
        XCTAssertEqual(fixture.features.tracking.error, "Pause rejected")
    }

    func testSleepDoesNotPreventPauseAndWakeAloneDoesNotResume() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.sync.sleep()
        fixture.trackingAutomation.setScreenLocked(true, at: fixture.clock.now)
        let pause = try await fixture.client.next()
        pause.paused(idle)
        try await finishHistory(fixture)
        try await fixture.settled()
        fixture.sync.wake()
        let refresh = try await fixture.client.next()
        XCTAssertEqual(refresh.operation, .refresh(.local))
        refresh.succeed(idle)
        try await fixture.settled()
        XCTAssertEqual(fixture.trackingAutomation.statusText, "Paused while screen is locked")
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .resume = $0 { return true }; return false
            })
    }

    func testUncertainResumeReloadsWithoutRetryingWrite() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        try await acknowledgePause(fixture)
        fixture.trackingAutomation.setScreenLocked(false, at: fixture.clock.now)
        let resume = try await fixture.client.next()
        resume.fail(BridgeFailure(message: "Resume response lost", uncertain: true))
        let reconcile = try await fixture.client.next()
        reconcile.succeed(running)
        try await finishHistory(fixture)
        try await fixture.settled()
        XCTAssertEqual(
            fixture.features.tracking.recordedError,
            "Could not confirm the change. Check the reloaded data before trying again. Resume response lost")
        XCTAssertEqual(fixture.features.tracking.active, activeWorklog)
        XCTAssertNil(fixture.trackingAutomation.statusText)
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .resume = $0 { return true }; return false
            }.count, 1)
    }

    func testFailedReloadLeavesStaleStateAndNoResumeIntent() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.trackingAutomation.setScreenLocked(true, at: fixture.clock.now)
        let taskListRequest = try await fixture.client.next()
        taskListRequest.fail(BridgeFailure(message: "Offline", kind: "unavailable"))
        let retry = try await fixture.client.next()
        retry.fail(BridgeFailure(message: "Still offline", kind: "unavailable"))
        try await fixture.settled()
        fixture.trackingAutomation.setScreenLocked(false, at: fixture.clock.now)
        XCTAssertTrue(fixture.features.connection.stale)
        XCTAssertEqual(fixture.features.tracking.recordedError, "Offline")
        XCTAssertNil(fixture.trackingAutomation.statusText)
    }

    func testManualStartCancelsPreviousResumeOwnership() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        try await acknowledgePause(fixture)
        fixture.features.tracking.startTracking(taskID: secondTask.id)
        let start = try await fixture.client.next()
        let started = WorklogItem(
            id: "manual-worklog", taskId: secondTask.id,
            start: formatTimestamp(fixture.clock.now), end: nil)
        start.succeed(TaskListResources(tasks: idle.catalog.value, active: started))
        try await fixture.settled()
        fixture.trackingAutomation.setScreenLocked(false, at: fixture.clock.now)
        XCTAssertNil(fixture.trackingAutomation.statusText)
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .resume = $0 { return true }; return false
            })
    }

    func testObservedCompetingTimerPermanentlyCancelsResumeOwnership() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        try await acknowledgePause(fixture)
        fixture.sync.refresh()
        let refresh = try await fixture.client.next()
        refresh.succeed(running)
        try await finishHistory(fixture)
        try await fixture.settled()
        XCTAssertNil(fixture.trackingAutomation.statusText)
        fixture.sync.refresh()
        let idleRefresh = try await fixture.client.next()
        idleRefresh.succeed(idle)
        try await finishHistory(fixture)
        try await fixture.settled()
        fixture.trackingAutomation.setScreenLocked(false, at: fixture.clock.now)
        XCTAssertFalse(fixture.client.hasPendingRequests)
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .resume = $0 { return true }; return false
            })
    }

    func testEnablingWhileLockedAndIdleDoesNotBeginRequest() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(emptyTaskListResources)
        fixture.trackingAutomation.setScreenLocked(true, at: fixture.clock.now)
        fixture.trackingAutomation.setPauseOnScreenLock(true)
        XCTAssertFalse(fixture.client.hasPendingRequests)
        XCTAssertEqual(fixture.client.operations, [.open(.local)])
    }

    func testDuplicateLockAndUnlockEventsSendOnePauseAndOneResume() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        let lock = fixture.clock.now
        fixture.trackingAutomation.setScreenLocked(true, at: lock)
        fixture.trackingAutomation.setScreenLocked(true, at: lock.addingTimeInterval(1))
        let pause = try await fixture.client.next()
        XCTAssertEqual(pause.operation, .pause(worklog: activeWorklog.id, at: formatTimestamp(lock)))
        pause.paused(idle)
        try await finishHistory(fixture)
        try await fixture.settled()
        let unlock = lock.addingTimeInterval(30)
        fixture.trackingAutomation.setScreenLocked(false, at: unlock)
        fixture.trackingAutomation.setScreenLocked(false, at: unlock.addingTimeInterval(1))
        let resume = try await fixture.client.next()
        XCTAssertEqual(resume.operation, .resume(task: firstTask.id, at: formatTimestamp(unlock)))
        resume.succeed(running)
        try await finishHistory(fixture)
        try await fixture.settled()
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .pause = $0 { return true }; return false
            }.count, 1)
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .resume = $0 { return true }; return false
            }.count, 1)
    }

    func testUnlockWhileSleepingSendsResumeWithoutWaitingForWake() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        try await acknowledgePause(fixture)
        fixture.sync.sleep()
        fixture.clock.now.addTimeInterval(30)
        let unlock = fixture.clock.now
        fixture.trackingAutomation.setScreenLocked(false, at: unlock)
        let resume = try await fixture.client.next()
        XCTAssertEqual(resume.operation, .resume(task: firstTask.id, at: formatTimestamp(unlock)))
        resume.succeed(running)
        try await finishHistory(fixture)
        try await fixture.settled()
        XCTAssertEqual(fixture.features.tracking.active, activeWorklog)
        XCTAssertNil(fixture.trackingAutomation.statusText)
        XCTAssertTrue(fixture.scheduler.active.isEmpty)
    }

    func testConnectionChangeClearsPausedTaskAndKeepsAnOpenEditor() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        try await acknowledgePause(fixture)
        fixture.taskCreation.open()
        fixture.taskCreation.setName("Draft on the old connection")
        let connecting = Task { await fixture.features.connection.connect(serverSettings) }
        let connect = try await fixture.client.next()
        XCTAssertEqual(connect.operation, .connect(serverSettings))
        let secondConnect = try await fixture.taskValue(Task { await fixture.features.connection.connect(.local) })
        XCTAssertFalse(secondConnect)
        connect.succeed(idle)
        try await finishHistory(fixture)
        let connected = try await fixture.taskValue(connecting)
        XCTAssertTrue(connected)
        try await fixture.settled()
        XCTAssertTrue(fixture.taskCreation.isPresented)
        XCTAssertEqual(fixture.taskCreation.name, "Draft on the old connection")
        XCTAssertNil(fixture.trackingAutomation.statusText)
        fixture.trackingAutomation.setScreenLocked(false, at: fixture.clock.now)
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .resume = $0 { return true }; return false
            })
    }

    func testChangingThePreferenceNotifiesTheWindowOnce() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(running)
        let content = ObservationProbe { ViewReads.content(fixture.features) }

        fixture.trackingAutomation.setPauseOnScreenLock(true)
        XCTAssertTrue(content.consume())
        XCTAssertTrue(fixture.trackingAutomation.enabled)

        fixture.trackingAutomation.setPauseOnScreenLock(true)
        XCTAssertFalse(content.consume(), "Repeating the preference must not redraw the window.")
    }

    func testTrackingErrorsAndAutomaticPauseUpdateTheStores() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        let tracking = fixture.features.tracking

        fixture.features.tracking.startTracking(taskID: secondTask.id)
        let write = try await fixture.client.next()
        write.fail(BridgeFailure(message: "Response lost", kind: "unavailable", uncertain: true))
        let confirmation = try await fixture.client.next()
        confirmation.succeed(running)
        try await fixture.settled()
        XCTAssertEqual(
            tracking.error, "Could not confirm the change. Check the reloaded data before trying again. Response lost")
        fixture.features.tracking.dismissError()
        XCTAssertNil(tracking.error)

        fixture.trackingAutomation.setScreenLocked(true, at: fixture.clock.now)
        let pause = try await fixture.client.next()
        pause.paused(idle)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.trackingAutomation.statusText, "Paused while screen is locked")
        XCTAssertNil(tracking.active)

        fixture.trackingAutomation.setScreenLocked(false, at: fixture.clock.now)
        XCTAssertNil(fixture.trackingAutomation.statusText, "Unlocking must clear the status before the resume.")
        let resume = try await fixture.client.next()
        XCTAssertEqual(resume.operation, .resume(task: firstTask.id, at: formatTimestamp(fixture.clock.now)))
        resume.succeed(running)
        try await fixture.client.next().succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(tracking.active, activeWorklog)
    }
}

@MainActor
private extension TrackingAutomationStore {
    /// Builds the store for tests of its bookkeeping, which send no pause or resume request.
    convenience init(repository: any TrackingPreferencesRepository) {
        self.init(repository: repository, client: FakeClient(), clock: FakeClock(), tracking: TrackingStore())
    }
}

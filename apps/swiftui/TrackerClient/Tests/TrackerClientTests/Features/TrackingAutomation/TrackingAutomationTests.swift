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
        fixture.session.screenLocked(at: date ?? fixture.clock.now)
        let pause = try await fixture.client.next()
        XCTAssertEqual(
            pause.operation,
            .pause(
                worklog: activeWorklog.id,
                at: commandTimestamp(date ?? fixture.clock.now)))
        pause.paused(idle)
        try await finishHistory(fixture)
        try await fixture.settled()
    }

    func testPreferenceDefaultsOffAndPersistsOnlyChanges() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(running)
        XCTAssertFalse(fixture.session.pauseOnScreenLock)
        fixture.session.screenLocked(at: fixture.clock.now)
        fixture.session.screenUnlocked(at: fixture.clock.now)
        XCTAssertEqual(fixture.client.operations.count, 2)
        fixture.session.setPauseOnScreenLock(true)
        XCTAssertFalse(fixture.client.hasPendingRequests)
        fixture.session.setPauseOnScreenLock(true)
        fixture.session.setPauseOnScreenLock(false)
        XCTAssertEqual(
            fixture.preferences.writes, [TrackingPreferences(pauseOnScreenLock: true), TrackingPreferences()])
        XCTAssertNil(fixture.session.autoPauseStatusText)
    }

    func testPauseAndResumeUseEventTimestampsAndSameTask() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        let lock = fixture.clock.now
        try await acknowledgePause(fixture, at: lock)
        XCTAssertEqual(fixture.session.autoPauseStatusText, "Paused while screen is locked")
        fixture.clock.now.addTimeInterval(90)
        let unlock = fixture.clock.now
        fixture.session.screenUnlocked(at: unlock)
        fixture.clock.now.addTimeInterval(10)
        let resume = try await fixture.client.next()
        XCTAssertEqual(resume.operation, .resume(task: firstTask.id, at: commandTimestamp(unlock)))
        resume.succeed(running)
        try await finishHistory(fixture)
        try await fixture.settled()
        XCTAssertNil(fixture.session.autoPauseStatusText)
    }

    func testIdleLockNeverStartsTrackingOnUnlock() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(emptyTaskListResources)
        fixture.session.screenLocked(at: fixture.clock.now)
        fixture.session.screenUnlocked(at: fixture.clock.now)
        XCTAssertEqual(fixture.client.operations, [.open(.local)])
        XCTAssertNil(fixture.session.autoPauseStatusText)
    }

    func testUnlockDuringPauseResumesAfterConfirmationWithTheUnlockTime() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.session.screenLocked(at: fixture.clock.now)
        let pause = try await fixture.client.next()
        fixture.clock.now.addTimeInterval(90)
        let unlockedAt = fixture.clock.now
        fixture.session.screenUnlocked(at: unlockedAt)
        fixture.session.screenUnlocked(at: unlockedAt)
        fixture.clock.now.addTimeInterval(30)
        pause.paused(idle)
        let resume = try await fixture.client.next()
        XCTAssertEqual(resume.operation, .resume(task: firstTask.id, at: commandTimestamp(unlockedAt)))
        resume.succeed(running)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.active, activeWorklog)
        XCTAssertNil(fixture.session.trackingError)
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .resume = $0 { return true }; return false
            }.count, 1)
    }

    func testUnlockDuringUnsuccessfulPauseNeverResumes() async throws {
        for didFail in [false, true] {
            let fixture = Fixture(pauseOnScreenLock: true)
            defer { fixture.cleanup() }
            try await fixture.start(running)
            fixture.session.screenLocked(at: fixture.clock.now)
            let pause = try await fixture.client.next()
            fixture.session.screenUnlocked(at: fixture.clock.now)
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
            XCTAssertEqual(fixture.session.trackingError, didFail ? "Pause rejected" : nil)
        }
    }

    func testDisablingThePreferenceWhileLockedNeverPausesARunningTask() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(idle)
        fixture.session.screenLocked(at: fixture.clock.now)
        fixture.session.refresh()
        let startedElsewhere = try await fixture.client.next()
        XCTAssertEqual(startedElsewhere.operation, .refresh(.local))
        startedElsewhere.succeed(running)
        try await finishHistory(fixture)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.active, activeWorklog)

        fixture.session.setPauseOnScreenLock(false)
        fixture.session.refresh()
        let next = try await fixture.client.next()
        XCTAssertEqual(next.operation, .refresh(.local), "Disabling the preference must not pause tracking.")
        next.succeed(running)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.active, activeWorklog)
    }

    func testManualStopAfterUnlockDuringPauseCancelsTheAutomaticResume() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.session.screenLocked(at: fixture.clock.now)
        let pause = try await fixture.client.next()
        fixture.session.screenUnlocked(at: fixture.clock.now)
        fixture.session.stopTracking(worklogID: activeWorklog.id)
        let stop = try await fixture.client.next()
        XCTAssertEqual(stop.operation, .stop(worklog: activeWorklog.id, at: commandTimestamp(fixture.clock.now)))
        pause.paused(idle)
        try await finishHistory(fixture)
        stop.succeed(idle)
        try await fixture.settled()
        XCTAssertNil(fixture.session.active)
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .resume = $0 { return true }; return false
            }, "A manual stop must win over the automatic resume.")
    }

    func testCancelledPendingPauseCannotGrantResumeAfterUnlock() {
        let state = TrackingAutomationState(preferences: TrackingPreferences(pauseOnScreenLock: true))
        XCTAssertTrue(state.lock())
        XCTAssertTrue(state.beginPause())
        XCTAssertNil(state.unlock(at: Date(timeIntervalSince1970: 90)))
        state.cancel()
        XCTAssertNil(state.paused(taskID: firstTask.id, didStop: true))
        XCTAssertNil(state.statusText)
    }

    func testRelockingDuringPauseKeepsTheLatestUnlockTime() {
        let state = TrackingAutomationState(preferences: TrackingPreferences(pauseOnScreenLock: true))
        XCTAssertTrue(state.lock())
        XCTAssertTrue(state.beginPause())
        XCTAssertNil(state.unlock(at: Date(timeIntervalSince1970: 90)))
        XCTAssertTrue(state.lock())
        XCTAssertFalse(state.beginPause())
        XCTAssertNil(state.paused(taskID: firstTask.id, didStop: true))
        XCTAssertEqual(state.statusText, "Paused while screen is locked")
        let latestUnlock = Date(timeIntervalSince1970: 180)
        let resume = state.unlock(at: latestUnlock)
        XCTAssertEqual(resume?.taskID, firstTask.id)
        XCTAssertEqual(resume?.date, latestUnlock)
    }

    func testNewWorklogAfterLockIsNotPaused() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(idle)
        let lock = fixture.clock.now
        fixture.session.refresh()
        let taskListRequest = try await fixture.client.next()
        fixture.session.screenLocked(at: lock)
        let later = WorklogItem(
            id: "later-worklog", taskId: firstTask.id,
            start: commandTimestamp(lock.addingTimeInterval(1)), end: nil)
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
        fixture.session.screenLocked(at: fixture.clock.now)
        let pause = try await fixture.client.next()
        pause.paused(idle, didStop: false)
        try await finishHistory(fixture)
        try await fixture.settled()
        fixture.session.screenUnlocked(at: fixture.clock.now)
        XCTAssertNil(fixture.session.autoPauseStatusText)
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .resume = $0 { return true }; return false
            })
    }

    func testDisableWhilePauseIsInflightAcceptsResourcesWithoutResuming() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.session.screenLocked(at: fixture.clock.now)
        let pause = try await fixture.client.next()
        fixture.session.setPauseOnScreenLock(false)
        fixture.session.screenUnlocked(at: fixture.clock.now)
        pause.paused(idle)
        try await finishHistory(fixture)
        try await fixture.settled()
        XCTAssertNil(fixture.session.active)
        XCTAssertNil(fixture.session.autoPauseStatusText)
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .resume = $0 { return true }; return false
            })
    }

    func testEnableWhileLockedUsesEnableTime() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.session.screenLocked(at: fixture.clock.now)
        fixture.clock.now.addTimeInterval(10)
        fixture.session.setPauseOnScreenLock(true)
        let pause = try await fixture.client.next()
        XCTAssertEqual(pause.operation, .pause(worklog: activeWorklog.id, at: commandTimestamp(fixture.clock.now)))
        pause.paused(idle)
        try await finishHistory(fixture)
        try await fixture.settled()
    }

    func testPauseFailureReloadsAndNeverResumes() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.session.screenLocked(at: fixture.clock.now)
        let pause = try await fixture.client.next()
        pause.fail(BridgeFailure(message: "Response lost", uncertain: true))
        let reconcile = try await fixture.client.next()
        XCTAssertEqual(reconcile.operation, .refresh(fixture.session.connectionSettings))
        reconcile.succeed(idle)
        try await finishHistory(fixture)
        try await fixture.settled()
        fixture.session.screenUnlocked(at: fixture.clock.now)
        XCTAssertEqual(
            fixture.session.trackingError,
            "Could not confirm the change. Check the reloaded data before trying again. Response lost")
        XCTAssertNil(fixture.session.autoPauseStatusText)
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .resume = $0 { return true }; return false
            })
    }

    func testSleepDoesNotPreventPauseAndWakeAloneDoesNotResume() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.session.sleep()
        fixture.session.screenLocked(at: fixture.clock.now)
        let pause = try await fixture.client.next()
        pause.paused(idle)
        try await finishHistory(fixture)
        try await fixture.settled()
        fixture.session.wake()
        let refresh = try await fixture.client.next()
        XCTAssertEqual(refresh.operation, .refresh(.local))
        refresh.succeed(idle)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.autoPauseStatusText, "Paused while screen is locked")
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
        fixture.session.screenUnlocked(at: fixture.clock.now)
        let resume = try await fixture.client.next()
        resume.fail(BridgeFailure(message: "Resume response lost", uncertain: true))
        let reconcile = try await fixture.client.next()
        reconcile.succeed(running)
        try await finishHistory(fixture)
        try await fixture.settled()
        XCTAssertEqual(
            fixture.session.trackingError,
            "Could not confirm the change. Check the reloaded data before trying again. Resume response lost")
        XCTAssertEqual(fixture.session.active, activeWorklog)
        XCTAssertNil(fixture.session.autoPauseStatusText)
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .resume = $0 { return true }; return false
            }.count, 1)
    }

    func testFailedReloadLeavesStaleStateAndNoResumeIntent() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.session.screenLocked(at: fixture.clock.now)
        let taskListRequest = try await fixture.client.next()
        taskListRequest.fail(BridgeFailure(message: "Offline", kind: "unavailable"))
        let retry = try await fixture.client.next()
        retry.fail(BridgeFailure(message: "Still offline", kind: "unavailable"))
        try await fixture.settled()
        fixture.session.screenUnlocked(at: fixture.clock.now)
        XCTAssertTrue(fixture.session.isStale)
        XCTAssertEqual(fixture.session.trackingError, "Offline")
        XCTAssertNil(fixture.session.autoPauseStatusText)
    }

    func testManualStartCancelsPreviousResumeOwnership() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        try await acknowledgePause(fixture)
        fixture.session.startTracking(taskID: secondTask.id)
        let start = try await fixture.client.next()
        let started = WorklogItem(
            id: "manual-worklog", taskId: secondTask.id,
            start: commandTimestamp(fixture.clock.now), end: nil)
        start.succeed(TaskListResources(tasks: idle.catalog.value, active: started))
        try await fixture.settled()
        fixture.session.screenUnlocked(at: fixture.clock.now)
        XCTAssertNil(fixture.session.autoPauseStatusText)
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
        fixture.session.refresh()
        let refresh = try await fixture.client.next()
        refresh.succeed(running)
        try await finishHistory(fixture)
        try await fixture.settled()
        XCTAssertNil(fixture.session.autoPauseStatusText)
        fixture.session.refresh()
        let idleRefresh = try await fixture.client.next()
        idleRefresh.succeed(idle)
        try await finishHistory(fixture)
        try await fixture.settled()
        fixture.session.screenUnlocked(at: fixture.clock.now)
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
        fixture.session.screenLocked(at: fixture.clock.now)
        fixture.session.setPauseOnScreenLock(true)
        XCTAssertFalse(fixture.client.hasPendingRequests)
        XCTAssertEqual(fixture.client.operations, [.open(.local)])
    }

    func testDuplicateLockAndUnlockEventsSendOnePauseAndOneResume() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        let lock = fixture.clock.now
        fixture.session.screenLocked(at: lock)
        fixture.session.screenLocked(at: lock.addingTimeInterval(1))
        let pause = try await fixture.client.next()
        XCTAssertEqual(pause.operation, .pause(worklog: activeWorklog.id, at: commandTimestamp(lock)))
        pause.paused(idle)
        try await finishHistory(fixture)
        try await fixture.settled()
        let unlock = lock.addingTimeInterval(30)
        fixture.session.screenUnlocked(at: unlock)
        fixture.session.screenUnlocked(at: unlock.addingTimeInterval(1))
        let resume = try await fixture.client.next()
        XCTAssertEqual(resume.operation, .resume(task: firstTask.id, at: commandTimestamp(unlock)))
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
        fixture.session.sleep()
        fixture.clock.now.addTimeInterval(30)
        let unlock = fixture.clock.now
        fixture.session.screenUnlocked(at: unlock)
        let resume = try await fixture.client.next()
        XCTAssertEqual(resume.operation, .resume(task: firstTask.id, at: commandTimestamp(unlock)))
        resume.succeed(running)
        try await finishHistory(fixture)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.active, activeWorklog)
        XCTAssertNil(fixture.session.autoPauseStatusText)
        XCTAssertTrue(fixture.scheduler.active.isEmpty)
    }

    func testConnectionChangeClearsPausedTaskAndKeepsAnOpenEditor() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        try await acknowledgePause(fixture)
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("Draft on the old connection")
        let connecting = Task { await fixture.session.connect(serverSettings) }
        let connect = try await fixture.client.next()
        XCTAssertEqual(connect.operation, .connect(serverSettings))
        let secondConnect = await fixture.session.connect(.local)
        XCTAssertFalse(secondConnect)
        connect.succeed(idle)
        try await finishHistory(fixture)
        let connected = try await fixture.taskValue(connecting)
        XCTAssertTrue(connected)
        try await fixture.settled()
        XCTAssertTrue(fixture.session.taskCreation.isPresented)
        XCTAssertEqual(fixture.session.taskCreation.name, "Draft on the old connection")
        XCTAssertNil(fixture.session.autoPauseStatusText)
        fixture.session.screenUnlocked(at: fixture.clock.now)
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .resume = $0 { return true }; return false
            })
    }

}

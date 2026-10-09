import Foundation
import XCTest
@testable import TrackerClient

@MainActor
final class TrackingAutomationTests: XCTestCase {
    private let running = TaskListResources(tasks: [firstTask, secondTask], active: activeWorklog)
    private let idle = TaskListResources(tasks: [firstTask, secondTask], active: nil)

    private func acknowledgePause(_ fixture: Fixture, at date: Date? = nil) async throws {
        fixture.session.screenLocked(at: date ?? fixture.clock.now)
        let snapshot = try await fixture.client.next()
        XCTAssertEqual(snapshot.operation, .taskList)
        snapshot.succeed(running)
        let pause = try await fixture.client.next()
        XCTAssertEqual(
            pause.operation,
            .pause(
                worklog: activeWorklog.id,
                at: commandTimestamp(date ?? fixture.clock.now)))
        pause.paused(idle)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        history.succeed(emptyPage)
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
        XCTAssertFalse(fixture.session.isBusy)
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
        let snapshot = try await fixture.client.next()
        XCTAssertEqual(snapshot.operation, .taskList)
        snapshot.succeed(idle)
        let resume = try await fixture.client.next()
        XCTAssertEqual(resume.operation, .resume(task: firstTask.id, at: commandTimestamp(unlock)))
        resume.succeed(running)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertNil(fixture.session.autoPauseStatusText)
    }

    func testIdleLockNeverStartsTrackingOnUnlock() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(emptySnapshot)
        fixture.session.screenLocked(at: fixture.clock.now)
        fixture.session.screenUnlocked(at: fixture.clock.now)
        XCTAssertEqual(fixture.client.operations, [.open(.local)])
        XCTAssertNil(fixture.session.autoPauseStatusText)
    }

    func testUnlockWaitsForPendingPauseAndDuplicateEventsDoNotDuplicateWrites() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.session.screenLocked(at: fixture.clock.now)
        fixture.session.screenLocked(at: fixture.clock.now.addingTimeInterval(1))
        let snapshot = try await fixture.client.next()
        snapshot.succeed(running)
        let pause = try await fixture.client.next()
        fixture.clock.now.addTimeInterval(20)
        let unlock = fixture.clock.now
        fixture.session.screenUnlocked(at: unlock)
        fixture.session.screenUnlocked(at: unlock.addingTimeInterval(1))
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .resume = $0 { return true }; return false
            })
        pause.paused(idle)
        let reconcile = try await fixture.client.next()
        XCTAssertEqual(reconcile.operation, .taskList)
        reconcile.succeed(idle)
        let resume = try await fixture.client.next()
        XCTAssertEqual(resume.operation, .resume(task: firstTask.id, at: commandTimestamp(unlock)))
        resume.succeed(running)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
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

    func testPauseHasPriorityOverHistoryAfterInflightStart() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(idle)
        fixture.session.startTracking(taskID: firstTask.id)
        let start = try await fixture.client.next()
        fixture.session.screenLocked(at: fixture.clock.now)
        start.succeed(running)
        let reconcile = try await fixture.client.next()
        XCTAssertEqual(reconcile.operation, .taskList)
        reconcile.succeed(running)
        let pause = try await fixture.client.next()
        XCTAssertEqual(pause.operation, .pause(worklog: activeWorklog.id, at: commandTimestamp(fixture.clock.now)))
        pause.paused(idle)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
    }

    func testKnownWorklogReplacementIsNotPaused() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.session.screenLocked(at: fixture.clock.now)
        let snapshot = try await fixture.client.next()
        let other = WorklogItem(id: "external-worklog", taskId: secondTask.id, start: activeWorklog.start, end: nil)
        snapshot.succeed(TaskListResources(tasks: idle.catalog.value, active: other))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .pause = $0 { return true }; return false
            })
    }

    func testNewWorklogAfterLockIsNotPaused() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(idle)
        let lock = fixture.clock.now
        fixture.session.refresh()
        let snapshot = try await fixture.client.next()
        fixture.session.screenLocked(at: lock)
        let later = WorklogItem(
            id: "later-worklog", taskId: firstTask.id,
            start: commandTimestamp(lock.addingTimeInterval(1)), end: nil)
        snapshot.succeed(TaskListResources(tasks: idle.catalog.value, active: later))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
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
        let snapshot = try await fixture.client.next()
        snapshot.succeed(running)
        let pause = try await fixture.client.next()
        pause.paused(idle, didStop: false)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        fixture.session.screenUnlocked(at: fixture.clock.now)
        XCTAssertNil(fixture.session.autoPauseStatusText)
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .resume = $0 { return true }; return false
            })
    }

    func testCompetingTimerCancelsResume() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        try await acknowledgePause(fixture)
        fixture.session.screenUnlocked(at: fixture.clock.now)
        let snapshot = try await fixture.client.next()
        snapshot.succeed(running)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertNil(fixture.session.autoPauseStatusText)
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .resume = $0 { return true }; return false
            })
    }

    func testArchivedTaskCancelsResume() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        try await acknowledgePause(fixture)
        fixture.session.screenUnlocked(at: fixture.clock.now)
        let snapshot = try await fixture.client.next()
        let archived = TaskItem(id: firstTask.id, name: firstTask.name, archived: true, latestStart: nil)
        snapshot.succeed(TaskListResources(tasks: [archived, secondTask], active: nil))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertNil(fixture.session.autoPauseStatusText)
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .resume = $0 { return true }; return false
            })
    }

    func testDisableWhilePauseIsInflightAcceptsSnapshotWithoutResuming() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.session.screenLocked(at: fixture.clock.now)
        let snapshot = try await fixture.client.next()
        snapshot.succeed(running)
        let pause = try await fixture.client.next()
        fixture.session.setPauseOnScreenLock(false)
        fixture.session.screenUnlocked(at: fixture.clock.now)
        pause.paused(idle)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
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
        let snapshot = try await fixture.client.next()
        snapshot.succeed(running)
        let pause = try await fixture.client.next()
        XCTAssertEqual(pause.operation, .pause(worklog: activeWorklog.id, at: commandTimestamp(fixture.clock.now)))
        pause.paused(idle)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
    }

    func testPauseFailureReconcilesAndNeverResumes() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.session.screenLocked(at: fixture.clock.now)
        let snapshot = try await fixture.client.next()
        snapshot.succeed(running)
        let pause = try await fixture.client.next()
        pause.fail(BridgeFailure(message: "Response lost", uncertain: true))
        let reconcile = try await fixture.client.next()
        XCTAssertEqual(reconcile.operation, .taskList)
        reconcile.succeed(idle)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        fixture.session.screenUnlocked(at: fixture.clock.now)
        XCTAssertEqual(fixture.session.trackingError, "Response lost")
        XCTAssertNil(fixture.session.autoPauseStatusText)
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .resume = $0 { return true }; return false
            })
    }

    func testLockBeforeStartupIsDrainedAfterOpen() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        fixture.session.screenLocked(at: fixture.clock.now)
        fixture.session.start()
        let open = try await fixture.client.next()
        open.succeed(running)
        let snapshot = try await fixture.client.next()
        XCTAssertEqual(snapshot.operation, .taskList)
        snapshot.succeed(running)
        let pause = try await fixture.client.next()
        pause.paused(idle)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.autoPauseStatusText, "Paused while screen is locked")
    }

    func testSleepDoesNotPreventPauseAndWakeAloneDoesNotResume() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.session.sleep()
        fixture.session.screenLocked(at: fixture.clock.now)
        let snapshot = try await fixture.client.next()
        snapshot.succeed(running)
        let pause = try await fixture.client.next()
        pause.paused(idle)
        try await fixture.settled()
        fixture.session.wake()
        let refresh = try await fixture.client.next()
        XCTAssertEqual(refresh.operation, .refresh(.local))
        refresh.succeed(idle)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.autoPauseStatusText, "Paused while screen is locked")
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .resume = $0 { return true }; return false
            })
    }

    func testReentrantDisableAtBusyNotificationPreventsPauseWrite() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.session.onChange = {
            if fixture.session.isBusy { fixture.session.setPauseOnScreenLock(false) }
        }
        fixture.session.screenLocked(at: fixture.clock.now)
        fixture.session.onChange = nil
        let snapshot = try await fixture.client.next()
        snapshot.succeed(running)
        try await fixture.settled()
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .pause = $0 { return true }; return false
            })
    }
    func testRelockDuringResumePausesAcknowledgedNewWorklog() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        try await acknowledgePause(fixture)
        fixture.clock.now.addTimeInterval(30)
        fixture.session.screenUnlocked(at: fixture.clock.now)
        let snapshot = try await fixture.client.next()
        snapshot.succeed(idle)
        let resume = try await fixture.client.next()
        let resumed = WorklogItem(
            id: "resumed-worklog", taskId: firstTask.id,
            start: commandTimestamp(fixture.clock.now), end: nil)
        fixture.clock.now.addTimeInterval(10)
        let relock = fixture.clock.now
        fixture.session.screenLocked(at: relock)
        let resumedSnapshot = TaskListResources(tasks: idle.catalog.value, active: resumed)
        resume.succeed(resumedSnapshot)
        let reconcile = try await fixture.client.next()
        XCTAssertEqual(reconcile.operation, .taskList)
        reconcile.succeed(resumedSnapshot)
        let pause = try await fixture.client.next()
        XCTAssertEqual(pause.operation, .pause(worklog: resumed.id, at: commandTimestamp(relock)))
        pause.paused(idle)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.autoPauseStatusText, "Paused while screen is locked")
    }

    func testLockDuringOwnTaskSwitchPausesNewTask() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.session.startTracking(taskID: secondTask.id)
        let start = try await fixture.client.next()
        fixture.session.screenLocked(at: fixture.clock.now)
        let switched = WorklogItem(
            id: "switched-worklog", taskId: secondTask.id,
            start: commandTimestamp(fixture.clock.now), end: nil)
        let snapshot = TaskListResources(tasks: idle.catalog.value, active: switched)
        start.succeed(snapshot)
        let reconcile = try await fixture.client.next()
        reconcile.succeed(snapshot)
        let pause = try await fixture.client.next()
        XCTAssertEqual(pause.operation, .pause(worklog: switched.id, at: commandTimestamp(fixture.clock.now)))
        pause.paused(idle)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
    }

    func testConnectionIntentCancelsPauseOwnershipEvenIfBusy() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.session.screenLocked(at: fixture.clock.now)
        let snapshot = try await fixture.client.next()
        snapshot.succeed(running)
        let pause = try await fixture.client.next()
        let connecting = Task { await fixture.session.connect(serverSettings) }
        let changed = try await fixture.taskValue(connecting)
        XCTAssertFalse(changed)
        pause.paused(idle)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        fixture.session.screenUnlocked(at: fixture.clock.now)
        XCTAssertNil(fixture.session.autoPauseStatusText)
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .resume = $0 { return true }; return false
            })
    }

    func testShutdownIgnoresLatePauseAndEvents() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.session.screenLocked(at: fixture.clock.now)
        let snapshot = try await fixture.client.next()
        snapshot.succeed(running)
        let pause = try await fixture.client.next()
        fixture.session.shutdown()
        pause.paused(idle)
        fixture.session.screenUnlocked(at: fixture.clock.now)
        fixture.session.setPauseOnScreenLock(false)
        XCTAssertEqual(fixture.session.active, activeWorklog)
        XCTAssertNil(fixture.session.autoPauseStatusText)
        XCTAssertTrue(fixture.session.pauseOnScreenLock)
        XCTAssertFalse(fixture.session.isBusy)
    }

    func testUnlockWhileSleepingResumesOnlyAfterWake() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        try await acknowledgePause(fixture)
        fixture.session.sleep()
        fixture.clock.now.addTimeInterval(30)
        let unlock = fixture.clock.now
        fixture.session.screenUnlocked(at: unlock)
        XCTAssertFalse(fixture.session.isBusy)
        fixture.session.wake()
        let refresh = try await fixture.client.next()
        refresh.succeed(idle)
        let snapshot = try await fixture.client.next()
        XCTAssertEqual(snapshot.operation, .taskList)
        snapshot.succeed(idle)
        let resume = try await fixture.client.next()
        XCTAssertEqual(resume.operation, .resume(task: firstTask.id, at: commandTimestamp(unlock)))
        resume.succeed(running)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
    }

    func testMalformedActiveStartCannotBePaused() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        let malformed = WorklogItem(id: activeWorklog.id, taskId: firstTask.id, start: "invalid", end: nil)
        let snapshot = TaskListResources(tasks: idle.catalog.value, active: malformed)
        try await fixture.start(snapshot)
        fixture.session.screenLocked(at: fixture.clock.now)
        let reconcile = try await fixture.client.next()
        reconcile.succeed(snapshot)
        try await fixture.settled()
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .pause = $0 { return true }; return false
            })
        XCTAssertEqual(fixture.session.trackingError, "Cannot pause tracking because its start time is invalid.")
    }

    func testUncertainResumeReconcilesWithoutRetryingWrite() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        try await acknowledgePause(fixture)
        fixture.session.screenUnlocked(at: fixture.clock.now)
        let snapshot = try await fixture.client.next()
        snapshot.succeed(idle)
        let resume = try await fixture.client.next()
        resume.fail(BridgeFailure(message: "Resume response lost", uncertain: true))
        let reconcile = try await fixture.client.next()
        reconcile.succeed(running)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.trackingError, "Resume response lost")
        XCTAssertEqual(fixture.session.active, activeWorklog)
        XCTAssertNil(fixture.session.autoPauseStatusText)
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .resume = $0 { return true }; return false
            }.count, 1)
    }

    func testFailedReconciliationLeavesStaleStateAndNoResumeIntent() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.session.screenLocked(at: fixture.clock.now)
        let snapshot = try await fixture.client.next()
        snapshot.fail(BridgeFailure(message: "Offline", kind: "unavailable"))
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

    func testLockDuringConnectionChangeCannotPauseNewSourceWithOverlappingIDs() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        let connecting = Task { await fixture.session.connect(serverSettings) }
        let connect = try await fixture.client.next()
        fixture.session.screenLocked(at: fixture.clock.now)
        connect.succeed(running)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        history.succeed(emptyPage)
        let connected = try await fixture.taskValue(connecting)
        XCTAssertTrue(connected)
        try await fixture.settled()
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .pause = $0 { return true }; return false
            })
        XCTAssertNil(fixture.session.autoPauseStatusText)
    }

    func testObservedCompetingTimerPermanentlyCancelsResumeOwnership() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        try await acknowledgePause(fixture)
        fixture.session.refresh()
        let refresh = try await fixture.client.next()
        refresh.succeed(running)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertNil(fixture.session.autoPauseStatusText)
        fixture.session.refresh()
        let idleRefresh = try await fixture.client.next()
        idleRefresh.succeed(idle)
        let idleHistory = try await fixture.client.next()
        idleHistory.succeed(emptyPage)
        try await fixture.settled()
        fixture.session.screenUnlocked(at: fixture.clock.now)
        XCTAssertFalse(fixture.session.isBusy)
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .resume = $0 { return true }; return false
            })
    }

    func testEnablingWhileLockedRecognizesInflightOwnStart() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(idle)
        fixture.session.startTracking(taskID: firstTask.id)
        let start = try await fixture.client.next()
        fixture.session.screenLocked(at: fixture.clock.now)
        fixture.clock.now.addTimeInterval(10)
        let enabledAt = fixture.clock.now
        fixture.session.setPauseOnScreenLock(true)
        start.succeed(running)
        let snapshot = try await fixture.client.next()
        snapshot.succeed(running)
        let pause = try await fixture.client.next()
        XCTAssertEqual(pause.operation, .pause(worklog: activeWorklog.id, at: commandTimestamp(enabledAt)))
        pause.paused(idle)
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.autoPauseStatusText, "Paused while screen is locked")
    }

    func testKnownWorklogCorrectedToAfterLockReportsFailure() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        let lock = fixture.clock.now
        fixture.session.screenLocked(at: lock)
        let snapshot = try await fixture.client.next()
        let corrected = WorklogItem(
            id: activeWorklog.id, taskId: activeWorklog.taskId,
            start: commandTimestamp(lock.addingTimeInterval(1)), end: nil)
        snapshot.succeed(TaskListResources(tasks: idle.catalog.value, active: corrected))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.trackingError, "Cannot pause tracking before the worklog start time.")
        XCTAssertNil(fixture.session.autoPauseStatusText)
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .pause = $0 { return true }; return false
            })
    }

    func testEnablingWhileLockedAndIdleDoesNotBeginRequest() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(emptySnapshot)
        fixture.session.screenLocked(at: fixture.clock.now)
        fixture.session.setPauseOnScreenLock(true)
        XCTAssertFalse(fixture.session.isBusy)
        XCTAssertEqual(fixture.client.operations, [.open(.local)])
    }

    func testUncertainPauseFailureCancelsRelockActionFromSameOwnershipGeneration() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.session.screenLocked(at: fixture.clock.now)
        let snapshot = try await fixture.client.next()
        snapshot.succeed(running)
        let pause = try await fixture.client.next()
        fixture.clock.now.addTimeInterval(10)
        fixture.session.screenUnlocked(at: fixture.clock.now)
        fixture.clock.now.addTimeInterval(10)
        fixture.session.screenLocked(at: fixture.clock.now)
        fixture.session.select(secondTask.id)
        pause.fail(BridgeFailure(message: "Pause response lost", uncertain: true))
        let reconcile = try await fixture.client.next()
        XCTAssertEqual(reconcile.operation, .taskList)
        reconcile.succeed(running)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: secondTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.trackingError, "Pause response lost")
        XCTAssertNil(fixture.session.autoPauseStatusText)
        fixture.session.screenUnlocked(at: fixture.clock.now)
        XCTAssertFalse(fixture.session.isBusy)
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .pause = $0 { return true }; return false
            }.count, 1)
        XCTAssertFalse(
            fixture.client.operations.contains {
                if case .resume = $0 { return true }; return false
            })
    }

    func testOldPauseFailurePreservesExplicitlyReenabledLockAction() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.session.screenLocked(at: fixture.clock.now)
        let snapshot = try await fixture.client.next()
        snapshot.succeed(running)
        let pause = try await fixture.client.next()
        fixture.session.setPauseOnScreenLock(false)
        fixture.clock.now.addTimeInterval(10)
        let enabledAt = fixture.clock.now
        fixture.session.setPauseOnScreenLock(true)
        pause.fail(BridgeFailure(message: "Old pause response lost", uncertain: true))
        let reconcile = try await fixture.client.next()
        XCTAssertEqual(reconcile.operation, .taskList)
        reconcile.succeed(running)
        let newSnapshot = try await fixture.client.next()
        XCTAssertEqual(newSnapshot.operation, .taskList)
        newSnapshot.succeed(running)
        let newPause = try await fixture.client.next()
        XCTAssertEqual(newPause.operation, .pause(worklog: activeWorklog.id, at: commandTimestamp(enabledAt)))
        newPause.paused(idle)
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.autoPauseStatusText, "Paused while screen is locked")
        XCTAssertNil(fixture.session.trackingError)
        XCTAssertEqual(
            fixture.client.operations.filter {
                if case .pause = $0 { return true }; return false
            }.count, 2)
    }

    func testBackgroundHistoryQueuesManualIntentAndLockCancelsItBeforeRefresh() async throws {
        let fixture = Fixture(pauseOnScreenLock: true)
        defer { fixture.cleanup() }
        try await fixture.start(running)
        fixture.session.retryHistory()
        guard fixture.session.isBusy else {
            XCTFail("A history fetch must use the session operation gate.")
            return
        }
        let history = try await fixture.client.next()
        XCTAssertEqual(history.operation, .history(task: firstTask.id, cursor: nil))
        let operationCount = fixture.client.operations.count
        fixture.session.startTracking(taskID: secondTask.id)
        fixture.session.stopTracking(worklogID: activeWorklog.id)
        let connecting = Task { await fixture.session.connect(serverSettings) }
        let changed = try await fixture.taskValue(connecting)
        XCTAssertFalse(changed)
        fixture.session.refresh()
        fixture.session.screenLocked(at: fixture.clock.now)
        XCTAssertTrue(fixture.session.isBusy)
        XCTAssertFalse(fixture.session.canStartSelectedTask)
        XCTAssertTrue(fixture.session.canStopTracking)
        XCTAssertNotNil(fixture.session.trackingError)
        XCTAssertEqual(fixture.client.operations.count, operationCount)
        history.succeed(emptyPage)
        let snapshot = try await fixture.client.next()
        XCTAssertEqual(snapshot.operation, .taskList)
        snapshot.succeed(running)
        let pause = try await fixture.client.next()
        XCTAssertEqual(pause.operation, .pause(worklog: activeWorklog.id, at: commandTimestamp(fixture.clock.now)))
        pause.paused(idle)
        let refresh = try await fixture.client.next()
        XCTAssertEqual(refresh.operation, .refresh(.local))
        refresh.succeed(idle)
        let newHistory = try await fixture.client.next()
        XCTAssertEqual(newHistory.operation, .history(task: firstTask.id, cursor: nil))
        newHistory.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.autoPauseStatusText, "Paused while screen is locked")
    }

}

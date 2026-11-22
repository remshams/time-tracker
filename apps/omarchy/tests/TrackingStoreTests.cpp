#include <QJsonObject>
#include <QSignalSpy>
#include <QTest>

#include "FakeTrackerGateway.h"
#include "Features/Tracking/TrackingStore.h"
#include "TestRegistry.h"
#include "TestSupport.h"

using namespace TestSupport;

namespace {

WorklogItem running(const QString &id, const QString &taskId, const QString &start = "2026-11-10T11:00:00.000Z") {
    return *WorklogItem::fromJson(worklogJson(id, taskId, start));
}

const BridgeReply stopped = BridgeReply::success(QJsonObject{{"didStop", true}});

// A store that already knows tracking is idle.
struct Fixture {
    FakeTrackerGateway gateway;
    TestClock test{"2026-11-10T12:00:00.000Z"};
    TrackingStore store{&gateway, test.clock()};
    Fixture() { store.accept(std::nullopt); }
};

} // namespace

class TrackingStoreTests : public QObject {
    Q_OBJECT
private slots:
    void startsUnknownAndUnavailable() {
        FakeTrackerGateway gateway;
        TestClock test;
        TrackingStore store(&gateway, test.clock());
        QVERIFY(!store.known());
        QVERIFY(!store.running());
        QVERIFY(!store.available());
        QVERIFY(!store.pending());
        QVERIFY(!store.stale());
        QCOMPARE(store.elapsedSeconds(), 0);
        QVERIFY(store.runningTaskId().isEmpty());
        QVERIFY(!store.runningSince().isValid());
        QVERIFY(store.error().isEmpty());
    }

    void commandsDoNothingWhileTheStateIsUnknown() {
        FakeTrackerGateway gateway;
        TestClock test;
        TrackingStore store(&gateway, test.clock());
        store.start("t1");
        store.stop();
        store.toggle("t1");
        QVERIFY(gateway.calls.isEmpty());
    }

    // Accepting the state

    void acceptingIdleMakesTheStoreKnownAndAvailable() {
        FakeTrackerGateway gateway;
        TestClock test;
        TrackingStore store(&gateway, test.clock());
        QSignalSpy state(&store, &TrackingStore::stateChanged);
        QSignalSpy elapsed(&store, &TrackingStore::elapsedChanged);
        QSignalSpy availability(&store, &TrackingStore::availabilityChanged);
        store.accept(std::nullopt);
        QVERIFY(store.known());
        QVERIFY(!store.running());
        QVERIFY(store.available());
        QCOMPARE(state.count(), 1);
        QCOMPARE(elapsed.count(), 1);
        QCOMPARE(availability.count(), 1);
    }

    void acceptingTheSameStateAgainStaysQuiet() {
        Fixture f;
        QSignalSpy state(&f.store, &TrackingStore::stateChanged);
        QSignalSpy availability(&f.store, &TrackingStore::availabilityChanged);
        f.store.accept(std::nullopt);
        f.store.accept(std::nullopt);
        QCOMPARE(state.count(), 0);
        f.store.accept(running("w1", "t1"));
        QCOMPARE(state.count(), 1);
        f.store.accept(running("w1", "t1"));
        QCOMPARE(state.count(), 1);
        QCOMPARE(availability.count(), 0);
    }

    void aDifferentWorklogChangesTheState() {
        Fixture f;
        f.store.accept(running("w1", "t1", "2026-11-10T11:00:00.000Z"));
        QSignalSpy state(&f.store, &TrackingStore::stateChanged);
        f.store.accept(running("w2", "t1", "2026-11-10T11:00:00.000Z"));
        QCOMPARE(state.count(), 1);
        f.store.accept(running("w2", "t1", "2026-11-10T11:30:00.000Z"));
        QCOMPARE(state.count(), 2);
        f.store.accept(running("w2", "t2", "2026-11-10T11:30:00.000Z"));
        // Only the worklog identity and start count; the task cannot change for one worklog.
        QCOMPARE(state.count(), 2);
    }

    void startingAndStoppingChangeTheState() {
        Fixture f;
        QSignalSpy state(&f.store, &TrackingStore::stateChanged);
        f.store.accept(running("w1", "t1"));
        QVERIFY(f.store.running());
        QCOMPARE(f.store.runningTaskId(), QStringLiteral("t1"));
        QCOMPARE(f.store.runningSince(), utc("2026-11-10T11:00:00.000Z"));
        QCOMPARE(f.store.runningWorklog()->id, QStringLiteral("w1"));
        f.store.accept(std::nullopt);
        QVERIFY(!f.store.running());
        QVERIFY(f.store.runningTaskId().isEmpty());
        QCOMPARE(state.count(), 2);
    }

    void elapsedSecondsCountFromTheStart() {
        Fixture f;
        f.store.accept(running("w1", "t1", "2026-11-10T11:58:30.000Z"));
        QCOMPARE(f.store.elapsedSeconds(), 90);
        f.test.set("2026-11-10T12:00:31.000Z");
        QCOMPARE(f.store.elapsedSeconds(), 121);
    }

    void elapsedSecondsNeverGoNegative() {
        Fixture f;
        f.store.accept(running("w1", "t1", "2026-11-10T12:05:00.000Z"));
        QCOMPARE(f.store.elapsedSeconds(), 0);
    }

    void tickingAnnouncesElapsedTimeOnlyWhileRunning() {
        Fixture f;
        QSignalSpy elapsed(&f.store, &TrackingStore::elapsedChanged);
        f.test.clock()->tick();
        QCOMPARE(elapsed.count(), 0);
        f.store.accept(running("w1", "t1"));
        const int before = elapsed.count();
        f.test.clock()->tick();
        QCOMPARE(elapsed.count(), before + 1);
    }

    // Starting

    void startSendsTheTaskTheExpectationAndTheClockTime() {
        Fixture f;
        f.store.start("t1");
        QCOMPARE(f.gateway.count("startTracking"), 1);
        QCOMPARE(f.gateway.lastArgs("startTracking"), QStringList({"t1", "", "2026-11-10T12:00:00.000Z"}));
    }

    void startWhileRunningExpectsTheRunningWorklog() {
        Fixture f;
        f.store.accept(running("w1", "t1"));
        f.store.start("t2");
        QCOMPARE(f.gateway.lastArgs("startTracking"), QStringList({"t2", "w1", "2026-11-10T12:00:00.000Z"}));
    }

    void startIgnoresAnEmptyTaskAndTheRunningTask() {
        Fixture f;
        f.store.start(QString());
        QVERIFY(f.gateway.calls.isEmpty());
        f.store.accept(running("w1", "t1"));
        f.store.start("t1");
        QVERIFY(f.gateway.calls.isEmpty());
    }

    void startIsPendingUntilTheAnswer() {
        Fixture f;
        QSignalSpy pending(&f.store, &TrackingStore::pendingChanged);
        QSignalSpy availability(&f.store, &TrackingStore::availabilityChanged);
        f.store.start("t1");
        QVERIFY(f.store.pending());
        QVERIFY(!f.store.available());
        QCOMPARE(pending.count(), 1);
        QCOMPARE(availability.count(), 1);
        // A second command waits for the first.
        f.store.start("t2");
        f.store.stop();
        QCOMPARE(f.gateway.calls.size(), 1);
        QVERIFY(f.gateway.respond("startTracking", BridgeReply::success()));
        QVERIFY(!f.store.pending());
        QVERIFY(f.store.available());
        QCOMPARE(pending.count(), 2);
        QCOMPARE(availability.count(), 2);
    }

    void aFinishedWriteRequestsARefresh() {
        Fixture f;
        QSignalSpy refresh(&f.store, &TrackingStore::refreshRequested);
        f.store.start("t1");
        QCOMPARE(refresh.count(), 0);
        f.gateway.respond("startTracking", BridgeReply::success());
        QCOMPARE(refresh.count(), 1);
        QVERIFY(f.store.error().isEmpty());
    }

    void aFailedStartShowsTheErrorAndStillRequestsARefresh() {
        Fixture f;
        QSignalSpy refresh(&f.store, &TrackingStore::refreshRequested);
        QSignalSpy error(&f.store, &TrackingStore::errorChanged);
        f.store.start("t1");
        f.gateway.respond("startTracking", BridgeReply::failure("Task is archived"));
        QCOMPARE(f.store.error(), QStringLiteral("Task is archived"));
        QCOMPARE(error.count(), 1);
        QCOMPARE(refresh.count(), 1);
        QVERIFY(!f.store.pending());
    }

    void anUncertainStartSaysItCouldNotBeConfirmed() {
        Fixture f;
        f.store.start("t1");
        BridgeReply reply = BridgeReply::failure("timeout");
        reply.uncertain = true;
        f.gateway.respond("startTracking", reply);
        QCOMPARE(f.store.error(),
                 QStringLiteral("The tracking change could not be confirmed. Check the timer before trying again."));
    }

    void aNewCommandClearsTheOldError() {
        Fixture f;
        f.store.start("t1");
        f.gateway.respond("startTracking", BridgeReply::failure("Task is archived"));
        QSignalSpy error(&f.store, &TrackingStore::errorChanged);
        f.store.start("t1");
        QVERIFY(f.store.error().isEmpty());
        QCOMPARE(error.count(), 1);
    }

    void aLateAnswerAfterTheStoreIsGoneIsIgnored() {
        FakeTrackerGateway gateway;
        TestClock test;
        auto *starting = new TrackingStore(&gateway, test.clock());
        starting->accept(std::nullopt);
        starting->start("t1");
        delete starting;
        QVERIFY(gateway.respond("startTracking", BridgeReply::success()));
        auto *stopping = new TrackingStore(&gateway, test.clock());
        stopping->accept(running("w1", "t1"));
        stopping->stop();
        delete stopping;
        QVERIFY(gateway.respond("stopTracking", stopped));
    }

    // Stopping

    void stopSendsTheRunningWorklogAndTheClockTime() {
        Fixture f;
        f.store.accept(running("w1", "t1"));
        f.store.stop();
        QCOMPARE(f.gateway.lastArgs("stopTracking"), QStringList({"w1", "2026-11-10T12:00:00.000Z"}));
        QVERIFY(f.store.pending());
    }

    void stopWhileIdleDoesNothing() {
        Fixture f;
        f.store.stop();
        QVERIFY(f.gateway.calls.isEmpty());
    }

    void aFailedStopShowsTheError() {
        Fixture f;
        f.store.accept(running("w1", "t1"));
        QSignalSpy refresh(&f.store, &TrackingStore::refreshRequested);
        f.store.stop();
        f.gateway.respond("stopTracking", BridgeReply::failure("Worklog not found"));
        QCOMPARE(f.store.error(), QStringLiteral("Worklog not found"));
        QCOMPARE(refresh.count(), 1);
    }

    void anUncertainStopSaysItCouldNotBeConfirmed() {
        Fixture f;
        f.store.accept(running("w1", "t1"));
        f.store.stop();
        BridgeReply reply = BridgeReply::failure("timeout");
        reply.uncertain = true;
        f.gateway.respond("stopTracking", reply);
        QCOMPARE(f.store.error(),
                 QStringLiteral("The stop could not be confirmed. Check the timer before trying again."));
    }

    void theStoppedTaskIsTheOneRunningWhenTheUserActed() {
        Fixture f;
        f.store.accept(running("w1", "t1"));
        f.store.stop();
        // A refresh lands before the answer and shows another task.
        f.store.accept(running("w2", "t2"));
        f.gateway.respond("stopTracking", stopped);
    }

    // Toggle

    void toggleStopsWhenRunningAndStartsWhenIdle() {
        Fixture f;
        f.store.toggle("t1");
        QCOMPARE(f.gateway.count("startTracking"), 1);
        f.gateway.respond("startTracking", BridgeReply::success());
        f.store.accept(running("w1", "t1"));
        f.store.toggle("t1");
        QCOMPARE(f.gateway.count("stopTracking"), 1);
        QCOMPARE(f.gateway.count("startTracking"), 1);
    }

    void toggleOfAnotherTaskWhileRunningStopsTheRunningOne() {
        Fixture f;
        f.store.accept(running("w1", "t1"));
        f.store.toggle("t2");
        QCOMPARE(f.gateway.count("stopTracking"), 1);
        QCOMPARE(f.gateway.count("startTracking"), 0);
    }

    // Stale

    void staleBlocksCommandsUntilFresh() {
        Fixture f;
        QSignalSpy stale(&f.store, &TrackingStore::staleChanged);
        QSignalSpy availability(&f.store, &TrackingStore::availabilityChanged);
        f.store.setStale(true);
        QVERIFY(f.store.stale());
        QVERIFY(!f.store.available());
        QCOMPARE(stale.count(), 1);
        QCOMPARE(availability.count(), 1);
        f.store.start("t1");
        QVERIFY(f.gateway.calls.isEmpty());
        f.store.setStale(true);
        QCOMPARE(stale.count(), 1);
        f.store.setStale(false);
        QVERIFY(f.store.available());
        QCOMPARE(stale.count(), 2);
        QCOMPARE(availability.count(), 2);
        f.store.start("t1");
        QCOMPARE(f.gateway.calls.size(), 1);
    }

    void staleWhilePendingDoesNotChangeAvailability() {
        Fixture f;
        f.store.start("t1");
        QSignalSpy availability(&f.store, &TrackingStore::availabilityChanged);
        f.store.setStale(true);
        QCOMPARE(availability.count(), 0);
    }

    // Reset

    void resetForgetsTheStateAndTheError() {
        Fixture f;
        f.store.start("t1");
        f.gateway.respond("startTracking", BridgeReply::failure("Task is archived"));
        f.store.accept(running("w1", "t1"));
        QSignalSpy state(&f.store, &TrackingStore::stateChanged);
        QSignalSpy elapsed(&f.store, &TrackingStore::elapsedChanged);
        QSignalSpy availability(&f.store, &TrackingStore::availabilityChanged);
        QSignalSpy error(&f.store, &TrackingStore::errorChanged);
        f.store.reset();
        QVERIFY(!f.store.known());
        QVERIFY(!f.store.running());
        QVERIFY(!f.store.available());
        QVERIFY(f.store.error().isEmpty());
        QCOMPARE(state.count(), 1);
        QCOMPARE(elapsed.count(), 1);
        QCOMPARE(availability.count(), 1);
        QCOMPARE(error.count(), 1);
    }

    void resetOfAnUnavailableStoreKeepsAvailabilityQuiet() {
        FakeTrackerGateway gateway;
        TestClock test;
        TrackingStore store(&gateway, test.clock());
        QSignalSpy availability(&store, &TrackingStore::availabilityChanged);
        QSignalSpy error(&store, &TrackingStore::errorChanged);
        store.reset();
        QCOMPARE(availability.count(), 0);
        QCOMPARE(error.count(), 0);
    }
};

REGISTER_TEST(TrackingStoreTests)
#include "TrackingStoreTests.moc"

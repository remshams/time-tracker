#include <QJsonArray>
#include <QSettings>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>

#include "App/TrackerSync.h"
#include "FakeTrackerGateway.h"
#include "Features/Connection/ConnectionStore.h"
#include "Features/DailyTotals/DailyTotalsStore.h"
#include "Features/TaskCatalog/TaskCatalogStore.h"
#include "Features/Tracking/TrackingStore.h"
#include "Features/WorklogHistory/WorklogHistoryStore.h"
#include "TestRegistry.h"
#include "TestSupport.h"

using namespace TestSupport;

namespace {

const QJsonArray tasks{taskJson("a", "Alpha", false, "2026-11-10T09:00:00.000Z"), taskJson("b", "Beta")};

BridgeReply overview(const QJsonValue &running = QJsonValue::Null) {
    return BridgeReply::success(overviewJson(tasks, running, "2026-11-10T12:00:00.000Z", {{"a", 600000000}}));
}

// The clock shows 12:00 UTC. The poll runs rarely unless a test asks for a short one.
struct Fixture {
    QTemporaryDir dir;
    FakeTrackerGateway gateway;
    TestClock test{"2026-11-10T12:00:00.000Z"};
    QSettings settings{dir.filePath("settings.ini"), QSettings::IniFormat};
    ConnectionStore connection{&gateway, &settings};
    TaskCatalogStore catalog{test.clock()};
    TrackingStore tracking{&gateway, test.clock()};
    DailyTotalsStore totals{&catalog, &tracking, test.clock()};
    WorklogHistoryStore history{&gateway, &catalog, test.clock()};
    TrackerSync sync;

    explicit Fixture(int pollMilliseconds = 3600000)
        : sync(&gateway, test.clock(), {&connection, &catalog, &tracking, &totals, &history}, pollMilliseconds) {}

    // Answers the open refresh with `reply`.
    void answer(const BridgeReply &reply) { QVERIFY(gateway.respond("refreshOverview", reply)); }
};

} // namespace

class TrackerSyncTests : public QObject {
    Q_OBJECT
private slots:
    // One refresh

    void refreshAsksForTheCurrentDay() {
        Fixture f;
        f.sync.refresh();
        QCOMPARE(f.gateway.count("refreshOverview"), 1);
        // The local day in the test zone (UTC+1) runs from 23:00 UTC to 23:00 UTC.
        QCOMPARE(f.gateway.lastArgs("refreshOverview"),
                 QStringList({"2026-11-09T23:00:00.000Z", "2026-11-10T23:00:00.000Z", "2026-11-10T12:00:00.000Z"}));
    }

    void aRefreshFeedsEveryStore() {
        Fixture f;
        f.sync.refresh();
        f.answer(overview(worklogJson("w1", "a", "2026-11-10T11:00:00.000Z")));
        QVERIFY(f.catalog.loaded());
        QCOMPARE(f.catalog.tasks().size(), 2);
        QVERIFY(f.tracking.known());
        QVERIFY(f.tracking.running());
        QCOMPARE(f.tracking.runningTaskId(), QStringLiteral("a"));
        QVERIFY(f.totals.known());
        // The report counted the running worklog up to its cutoff, which is now.
        QCOMPARE(f.totals.secondsFor("a"), 600);
        QVERIFY(!f.tracking.stale());
        QCOMPARE(f.connection.status(), QStringLiteral("connected"));
    }

    void aRefreshWithIdleTrackingMakesTheStateKnown() {
        Fixture f;
        f.sync.refresh();
        f.answer(overview());
        QVERIFY(f.tracking.known());
        QVERIFY(!f.tracking.running());
    }

    void aRefreshWithMissingPartsLeavesThemUntouched() {
        Fixture f;
        f.sync.refresh();
        f.answer(BridgeReply::success(QJsonObject()));
        QVERIFY(!f.catalog.loaded());
        QVERIFY(!f.tracking.known());
        QVERIFY(!f.totals.known());
        // The read still reached the data source.
        QCOMPARE(f.connection.status(), QStringLiteral("connected"));
    }

    void aMissingReportKeepsTheLastTotalsAsCached() {
        Fixture f;
        f.sync.refresh();
        f.answer(overview());
        QVERIFY(f.totals.known());
        f.sync.refresh();
        f.answer(BridgeReply::success(overviewJson(tasks, QJsonValue::Null, QString())));
        QVERIFY(f.totals.known());
        QVERIFY(f.totals.cached());
        QVERIFY(f.catalog.loaded());
    }

    void theFirstSelectionLoadsItsOwnHistoryOnce() {
        Fixture f;
        f.sync.refresh(true);
        f.answer(overview());
        QCOMPARE(f.gateway.count("history"), 1);
        QCOMPARE(f.gateway.lastArgs("history"), QStringList({"a", ""}));
    }

    // History

    void writesReloadTheHistoryOfTheSelectedTask() {
        Fixture f;
        f.sync.refresh();
        f.answer(overview());
        f.gateway.respond("history", BridgeReply::success(historyJson({})));
        QCOMPARE(f.gateway.count("history"), 1);
        f.sync.refresh(true);
        f.answer(overview());
        QCOMPARE(f.gateway.count("history"), 2);
        QCOMPARE(f.gateway.lastArgs("history"), QStringList({"a", ""}));
    }

    void plainRefreshesLeaveTheHistoryAlone() {
        Fixture f;
        f.sync.refresh();
        f.answer(overview());
        QCOMPARE(f.gateway.count("history"), 1);
        f.sync.refresh(false);
        f.answer(overview());
        QCOMPARE(f.gateway.count("history"), 1);
    }

    void aFinishedTrackingWriteRefreshesWithHistory() {
        Fixture f;
        f.tracking.accept(std::nullopt);
        f.tracking.start("a");
        f.gateway.respond("startTracking", BridgeReply::success());
        QCOMPARE(f.gateway.count("refreshOverview"), 1);
        f.answer(overview());
        // The first answer selects a task, which loads its history on its own.
        f.gateway.respond("history", BridgeReply::success(historyJson({})));
        f.tracking.start("b");
        f.gateway.respond("startTracking", BridgeReply::success());
        f.answer(overview());
        QCOMPARE(f.gateway.count("history"), 2);
    }

    // Coalescing

    void overlappingRefreshesRunOneMoreTime() {
        Fixture f;
        f.sync.refresh();
        f.sync.refresh();
        f.sync.refresh();
        QCOMPARE(f.gateway.count("refreshOverview"), 1);
        f.answer(overview());
        QCOMPARE(f.gateway.count("refreshOverview"), 2);
        f.answer(overview());
        QCOMPARE(f.gateway.count("refreshOverview"), 2);
    }

    void aRequestedHistoryReloadSurvivesLaterPlainRequests() {
        Fixture f;
        f.sync.refresh(false);
        f.sync.refresh(true);
        f.sync.refresh(false);
        f.answer(overview());
        // The selection changed during the first answer, which loaded the history once.
        f.gateway.respond("history", BridgeReply::success(historyJson({})));
        QCOMPARE(f.gateway.count("history"), 1);
        f.answer(overview());
        QCOMPARE(f.gateway.count("history"), 2);
    }

    void aSecondRunWithoutAHistoryRequestLeavesTheHistoryAlone() {
        Fixture f;
        f.sync.refresh(false);
        f.sync.refresh(false);
        f.answer(overview());
        f.gateway.respond("history", BridgeReply::success(historyJson({})));
        f.answer(overview());
        QCOMPARE(f.gateway.count("history"), 1);
    }

    void theHistoryRequestOfOneRunDoesNotLeakIntoTheNext() {
        Fixture f;
        f.sync.refresh(false);
        f.sync.refresh(true);
        f.answer(overview());
        f.gateway.respond("history", BridgeReply::success(historyJson({})));
        f.answer(overview());
        QCOMPARE(f.gateway.count("history"), 2);
        // A fresh overlap without the flag must not reload.
        f.sync.refresh(false);
        f.sync.refresh(false);
        f.answer(overview());
        f.answer(overview());
        QCOMPARE(f.gateway.count("history"), 2);
    }

    // Failure

    void aFailedRefreshMarksTheStateStaleAndTheSourceUnavailable() {
        Fixture f;
        f.sync.refresh();
        f.answer(overview());
        f.sync.refresh();
        f.answer(BridgeReply::failure("Server unreachable"));
        QVERIFY(f.tracking.stale());
        QVERIFY(!f.tracking.available());
        QCOMPARE(f.connection.status(), QStringLiteral("unavailable"));
        QCOMPARE(f.connection.statusMessage(), QStringLiteral("Server unreachable"));
        // The last confirmed state stays visible.
        QVERIFY(f.catalog.loaded());
        QVERIFY(f.totals.known());
    }

    void aRefreshAfterAFailureRecovers() {
        Fixture f;
        f.sync.refresh();
        f.answer(BridgeReply::failure("Server unreachable"));
        f.sync.refresh();
        f.answer(overview());
        QVERIFY(!f.tracking.stale());
        QCOMPARE(f.connection.status(), QStringLiteral("connected"));
        QVERIFY(f.connection.statusMessage().isEmpty());
    }

    void aQueuedRefreshStillRunsAfterAFailure() {
        Fixture f;
        f.sync.refresh();
        f.sync.refresh();
        f.answer(BridgeReply::failure("Server unreachable"));
        QCOMPARE(f.gateway.count("refreshOverview"), 2);
    }

    void aLateAnswerAfterTheSyncIsGoneIsIgnored() {
        FakeTrackerGateway gateway;
        QTemporaryDir dir;
        TestClock test;
        QSettings settings(dir.filePath("settings.ini"), QSettings::IniFormat);
        ConnectionStore connection(&gateway, &settings);
        TaskCatalogStore catalog(test.clock());
        TrackingStore tracking(&gateway, test.clock());
        DailyTotalsStore totals(&catalog, &tracking, test.clock());
        WorklogHistoryStore history(&gateway, &catalog, test.clock());
        auto *sync = new TrackerSync(&gateway, test.clock(), {&connection, &catalog, &tracking, &totals, &history});
        sync->refresh();
        delete sync;
        QVERIFY(gateway.respond("refreshOverview", overview()));
        QVERIFY(!catalog.loaded());
    }

    // Connecting

    void startOpensTheSavedConnectionAndThenReads() {
        Fixture f;
        f.sync.start();
        QCOMPARE(f.gateway.count("connectTo"), 1);
        QCOMPARE(f.gateway.count("refreshOverview"), 0);
        f.gateway.respond("connectTo", BridgeReply::success());
        QCOMPARE(f.gateway.count("refreshOverview"), 1);
    }

    void aNewConnectionForgetsTheOldSourceAndReadsTheNewOne() {
        Fixture f;
        f.sync.refresh();
        f.answer(overview(worklogJson("w1", "a", "2026-11-10T11:00:00.000Z")));
        f.gateway.respond("history",
                          BridgeReply::success(historyJson({worklogJson("w1", "a", "2026-11-10T11:00:00.000Z")})));
        QVERIFY(f.tracking.known());
        QVERIFY(f.totals.known());
        QCOMPARE(f.history.count(), 1);
        f.connection.setDraftRemote(true);
        f.connection.setDraftServerUrl("http://a:1");
        f.connection.applyDraft();
        f.gateway.respond("connectTo", BridgeReply::success());
        QVERIFY(!f.tracking.known());
        QVERIFY(!f.totals.known());
        QVERIFY(f.history.taskId().isEmpty());
        QCOMPARE(f.history.count(), 0);
        QVERIFY(!f.catalog.loaded());
        QVERIFY(f.catalog.tasks().isEmpty());
        QCOMPARE(f.gateway.count("refreshOverview"), 2);
    }

    void aNewSourceWithTheSameTasksLoadsTheSelectedHistoryAgain() {
        Fixture f;
        f.sync.refresh();
        f.answer(overview());
        f.gateway.respond("history", BridgeReply::success(historyJson({})));
        QCOMPARE(f.gateway.count("history"), 1);
        f.connection.applyDraft();
        f.gateway.respond("connectTo", BridgeReply::success());
        f.answer(overview());
        // The new source has the same task IDs; its history is read, not left blank.
        QCOMPARE(f.gateway.count("history"), 2);
        QCOMPARE(f.history.taskId(), f.catalog.selectedTaskId());
    }

    // Other triggers

    void aNewDayRefreshesWithHistory() {
        Fixture f;
        f.sync.refresh();
        f.answer(overview());
        f.gateway.respond("history", BridgeReply::success(historyJson({})));
        f.test.advanceTo("2026-11-10T23:30:00.000Z");
        QCOMPARE(f.gateway.count("refreshOverview"), 2);
        f.answer(overview());
        QCOMPARE(f.gateway.count("history"), 2);
        // The refresh asked for the new day.
        QCOMPARE(f.gateway.lastArgs("refreshOverview").first(), QStringLiteral("2026-11-10T23:00:00.000Z"));
    }

    // Polling

    void thePollReadsWhileConnected() {
        Fixture f(20);
        f.sync.start();
        f.gateway.respond("connectTo", BridgeReply::success());
        f.answer(overview());
        QTRY_VERIFY(f.gateway.count("refreshOverview") >= 2);
        QCOMPARE(f.gateway.count("connectTo"), 1);
    }

    void thePollReconnectsWhileNeverConnected() {
        Fixture f(20);
        f.gateway.autoReplies.insert("connectTo", BridgeReply::failure("Connection refused"));
        f.sync.start();
        QTRY_VERIFY(f.gateway.count("connectTo") >= 3);
        QCOMPARE(f.gateway.count("refreshOverview"), 0);
        QCOMPARE(f.connection.status(), QStringLiteral("unavailable"));
    }

    void thePollReadsInsteadOfReconnectingOnceTheStateIsKnown() {
        Fixture f(20);
        f.sync.start();
        f.gateway.respond("connectTo", BridgeReply::success());
        // Every later read fails, which leaves the source unavailable but the state known.
        f.gateway.autoReplies.insert("refreshOverview", BridgeReply::failure("Server unreachable"));
        f.answer(overview());
        QTRY_VERIFY(f.gateway.count("refreshOverview") >= 4);
        QCOMPARE(f.connection.status(), QStringLiteral("unavailable"));
        QVERIFY(f.tracking.known());
        QCOMPARE(f.gateway.count("connectTo"), 1);
    }

    void thePollWaitsWhileSettingsConnect() {
        Fixture f(20);
        f.gateway.autoReplies.insert("connectTo", BridgeReply::failure("Connection refused"));
        f.sync.start();
        f.gateway.autoReplies.remove("connectTo");
        // Settings now connects elsewhere; reopening the saved target could switch back.
        f.connection.setDraftRemote(true);
        f.connection.setDraftServerUrl("http://a:1");
        f.connection.applyDraft();
        const int before = f.gateway.count("connectTo");
        QTest::qWait(100);
        QCOMPARE(f.gateway.count("connectTo"), before);
        QVERIFY(f.connection.draftPending());
    }

    void aPollReloadsTheHistoryWhenAnotherClientChangesTracking() {
        Fixture f;
        f.sync.refresh();
        f.answer(overview(worklogJson("w1", "a", "2026-11-10T11:00:00.000Z")));
        f.gateway.respond("history", BridgeReply::success(historyJson({})));
        QCOMPARE(f.gateway.count("history"), 1);
        // Nothing changed: the history stays.
        f.sync.refresh();
        f.answer(overview(worklogJson("w1", "a", "2026-11-10T11:00:00.000Z")));
        QCOMPARE(f.gateway.count("history"), 1);
        // The bar widget paused and resumed: a new worklog runs.
        f.sync.refresh();
        f.answer(overview(worklogJson("w2", "a", "2026-11-10T11:30:00.000Z")));
        QCOMPARE(f.gateway.count("history"), 2);
    }

    void thePollSkipsReadsWhileTheFirstConnectionOpens() {
        Fixture f(20);
        f.sync.start();
        QTest::qWait(100);
        QCOMPARE(f.gateway.count("refreshOverview"), 0);
        QCOMPARE(f.gateway.count("connectTo"), 1);
    }

    void aFailedReadMarksTheTotalsAsCached() {
        Fixture f;
        f.sync.refresh();
        f.answer(overview());
        f.sync.refresh();
        f.answer(BridgeReply::failure("Server unreachable"));
        QVERIFY(f.totals.known());
        QVERIFY(f.totals.cached());
    }

    void thePollDoesNotRunBeforeStart() {
        Fixture f(10);
        QTest::qWait(60);
        QVERIFY(f.gateway.calls.isEmpty());
    }
};

REGISTER_TEST(TrackerSyncTests)
#include "TrackerSyncTests.moc"

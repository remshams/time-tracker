#include <QSignalSpy>
#include <QTest>

#include "FakeTrackerGateway.h"
#include "Features/DailyTotals/DailyTotalsStore.h"
#include "Features/TaskCatalog/TaskCatalogStore.h"
#include "Features/Tracking/TrackingStore.h"
#include "Shared/TaskColor.h"
#include "TestRegistry.h"
#include "TestSupport.h"

using namespace TestSupport;

namespace {

TaskItem item(const QString &id, const QString &name) {
    TaskItem task;
    task.id = id;
    task.name = name;
    return task;
}

DailyReport report(const QString &cutoff, const QHash<QString, qint64> &microseconds) {
    DailyReport result;
    result.cutoff = utc(cutoff);
    result.microseconds = microseconds;
    return result;
}

constexpr qint64 second = 1000000;

// The clock shows 12:00 UTC, which is 13:00 local; the local day started at 23:00 UTC the evening before.
struct Fixture {
    FakeTrackerGateway gateway;
    TestClock test{"2026-11-10T12:00:00.000Z"};
    TaskCatalogStore catalog{test.clock()};
    TrackingStore tracking{&gateway, test.clock()};
    DailyTotalsStore totals{&catalog, &tracking, test.clock()};

    Fixture() {
        catalog.accept({item("a", "Alpha"), item("b", "Beta"), item("c", "Gamma")});
        tracking.accept(std::nullopt);
    }
    void run(const QString &id, const QString &taskId, const QString &start) {
        tracking.accept(*WorklogItem::fromJson(worklogJson(id, taskId, start)));
    }
};

} // namespace

class DailyTotalsStoreTests : public QObject {
    Q_OBJECT
private slots:
    void startsUnknown() {
        Fixture f;
        QVERIFY(!f.totals.known());
        QCOMPARE(f.totals.totalSeconds(), 0);
        QVERIFY(f.totals.shares().isEmpty());
        QVERIFY(f.totals.leaderTaskId().isEmpty());
        QCOMPARE(f.totals.leaderPercent(), 0);
        QCOMPARE(f.totals.secondsFor("a"), -1);
        QVERIFY(f.totals.seconds().isEmpty());
    }

    void unknownTotalsIgnoreTheRunningWorklog() {
        Fixture f;
        f.run("w1", "a", "2026-11-10T10:00:00.000Z");
        QCOMPARE(f.totals.totalSeconds(), 0);
        QCOMPARE(f.totals.secondsFor("a"), -1);
    }

    void reportSecondsPerTask() {
        Fixture f;
        QSignalSpy changed(&f.totals, &DailyTotalsStore::totalsChanged);
        f.totals.accept(report("2026-11-10T12:00:00.000Z", {{"a", 3600 * second}, {"b", 1800 * second}}));
        QVERIFY(f.totals.known());
        QCOMPARE(changed.count(), 1);
        QCOMPARE(f.totals.secondsFor("a"), 3600);
        QCOMPARE(f.totals.secondsFor("b"), 1800);
        QCOMPARE(f.totals.totalSeconds(), 5400);
        QCOMPARE(f.totals.seconds().size(), 2);
    }

    void microsecondsRoundDown() {
        Fixture f;
        f.totals.accept(report("2026-11-10T12:00:00.000Z", {{"a", 1999999}}));
        QCOMPARE(f.totals.secondsFor("a"), 1);
    }

    void aTaskWithoutTimeHasZeroSecondsWhenAReportExists() {
        Fixture f;
        f.totals.accept(report("2026-11-10T12:00:00.000Z", {{"a", 60 * second}}));
        QCOMPARE(f.totals.secondsFor("c"), 0);
        QCOMPARE(f.totals.secondsFor("not a task"), 0);
    }

    void sharesRankTasksByTimeWithTheirFractions() {
        Fixture f;
        f.totals.accept(report("2026-11-10T12:00:00.000Z", {{"a", 1800 * second}, {"b", 3600 * second}}));
        const QVariantList shares = f.totals.shares();
        QCOMPARE(shares.size(), 2);
        const QVariantMap first = shares.at(0).toMap();
        QCOMPARE(first.value("taskId").toString(), QStringLiteral("b"));
        QCOMPARE(first.value("colorName").toString(), TaskColor::nameFor("b"));
        QCOMPARE(first.value("seconds").toLongLong(), 3600);
        QVERIFY(qAbs(first.value("fraction").toDouble() - 2.0 / 3.0) < 1e-9);
        const QVariantMap second = shares.at(1).toMap();
        QCOMPARE(second.value("taskId").toString(), QStringLiteral("a"));
        QCOMPARE(second.value("seconds").toLongLong(), 1800);
        QVERIFY(qAbs(second.value("fraction").toDouble() - 1.0 / 3.0) < 1e-9);
    }

    void sharesLeaveOutTasksWithoutTimeAndTasksThatAreNotInTheCatalog() {
        Fixture f;
        f.totals.accept(report("2026-11-10T12:00:00.000Z", {{"a", 60 * second}, {"c", 0}, {"gone", 600 * second}}));
        const QVariantList shares = f.totals.shares();
        QCOMPARE(shares.size(), 1);
        QCOMPARE(shares.first().toMap().value("taskId").toString(), QStringLiteral("a"));
        QCOMPARE(shares.first().toMap().value("fraction").toDouble(), 1.0);
        // The day total still counts everything the report holds.
        QCOMPARE(f.totals.totalSeconds(), 660);
    }

    void equalTimesKeepTheCatalogOrder() {
        Fixture f;
        f.totals.accept(report("2026-11-10T12:00:00.000Z", {{"a", 60 * second}, {"b", 60 * second}}));
        const QVariantList shares = f.totals.shares();
        QCOMPARE(shares.at(0).toMap().value("taskId").toString(), QStringLiteral("a"));
        QCOMPARE(shares.at(1).toMap().value("taskId").toString(), QStringLiteral("b"));
    }

    void leaderIsTheLargestShare() {
        Fixture f;
        f.totals.accept(report("2026-11-10T12:00:00.000Z", {{"a", 3600 * second}, {"b", 1800 * second}}));
        QCOMPARE(f.totals.leaderTaskId(), QStringLiteral("a"));
        // 3600 of 5400 seconds is 66.7 percent, which rounds to 67.
        QCOMPARE(f.totals.leaderPercent(), 67);
    }

    void leaderPercentRoundsHalfUpAndReaches100() {
        Fixture f;
        f.totals.accept(report("2026-11-10T12:00:00.000Z", {{"a", 60 * second}}));
        QCOMPARE(f.totals.leaderPercent(), 100);
        // 1 of 8 seconds is 12.5 percent.
        f.totals.accept(report("2026-11-10T12:00:00.000Z", {{"a", 7 * second}, {"b", 1 * second}}));
        QCOMPARE(f.totals.leaderPercent(), 88);
        f.totals.accept(report("2026-11-10T12:00:00.000Z", {{"a", 1 * second}, {"b", 7 * second}}));
        QCOMPARE(f.totals.leaderPercent(), 88);
    }

    void aReportWithoutTimeHasNoLeader() {
        Fixture f;
        f.totals.accept(report("2026-11-10T12:00:00.000Z", {}));
        QVERIFY(f.totals.known());
        QVERIFY(f.totals.leaderTaskId().isEmpty());
        QCOMPARE(f.totals.leaderPercent(), 0);
        QVERIFY(f.totals.shares().isEmpty());
    }

    // The running worklog

    void theRunningWorklogAddsTimeSinceTheCutoff() {
        Fixture f;
        f.run("w1", "a", "2026-11-10T09:00:00.000Z");
        f.totals.accept(report("2026-11-10T11:00:00.000Z", {{"a", 7200 * second}}));
        f.test.set("2026-11-10T11:30:00.000Z");
        QCOMPARE(f.totals.secondsFor("a"), 7200 + 1800);
        QCOMPARE(f.totals.totalSeconds(), 7200 + 1800);
    }

    void theRunningWorklogCountsFromItsStartWhenItBeganAfterTheCutoff() {
        Fixture f;
        f.run("w1", "a", "2026-11-10T11:20:00.000Z");
        f.totals.accept(report("2026-11-10T11:00:00.000Z", {{"b", 60 * second}}));
        f.test.set("2026-11-10T11:30:00.000Z");
        // The task is missing from the report, so its time is only what ran since the start.
        QCOMPARE(f.totals.secondsFor("a"), 600);
        QCOMPARE(f.totals.secondsFor("b"), 60);
        QCOMPARE(f.totals.totalSeconds(), 660);
    }

    void theRunningWorklogCountsOnlyTodayWhenTheReportIsOlder() {
        Fixture f;
        // Started yesterday evening; the report was cut off before today began (23:00 UTC).
        f.run("w1", "a", "2026-11-09T19:00:00.000Z");
        f.totals.accept(report("2026-11-09T22:00:00.000Z", {}));
        f.test.set("2026-11-10T12:00:00.000Z");
        // 23:00 UTC to 12:00 UTC.
        QCOMPARE(f.totals.secondsFor("a"), 13 * 3600);
    }

    void aClockBehindTheCutoffAddsNothing() {
        Fixture f;
        f.run("w1", "a", "2026-11-10T09:00:00.000Z");
        f.totals.accept(report("2026-11-10T12:00:00.000Z", {{"a", 60 * second}}));
        f.test.set("2026-11-10T11:00:00.000Z");
        QCOMPARE(f.totals.secondsFor("a"), 60);
    }

    void secondsGrowWithTheClock() {
        Fixture f;
        f.run("w1", "a", "2026-11-10T11:00:00.000Z");
        f.totals.accept(report("2026-11-10T12:00:00.000Z", {{"a", 3600 * second}}));
        QCOMPARE(f.totals.secondsFor("a"), 3600);
        f.test.set("2026-11-10T12:00:05.000Z");
        QCOMPARE(f.totals.secondsFor("a"), 3605);
    }

    void aRunningTaskOutranksALongerOneOnceItPassesIt() {
        Fixture f;
        f.run("w1", "b", "2026-11-10T12:00:00.000Z");
        f.totals.accept(report("2026-11-10T12:00:00.000Z", {{"a", 100 * second}, {"b", 50 * second}}));
        QCOMPARE(f.totals.leaderTaskId(), QStringLiteral("a"));
        f.test.set("2026-11-10T12:02:00.000Z");
        QCOMPARE(f.totals.leaderTaskId(), QStringLiteral("b"));
    }

    // Change notifications

    void ticksAnnounceTotalsOnlyWhenARunningWorklogGrowsThem() {
        Fixture f;
        QSignalSpy changed(&f.totals, &DailyTotalsStore::totalsChanged);
        // No report, no running worklog.
        f.test.clock()->tick();
        QCOMPARE(changed.count(), 0);
        // A report but nothing running.
        f.totals.accept(report("2026-11-10T12:00:00.000Z", {{"a", 60 * second}}));
        changed.clear();
        f.test.clock()->tick();
        QCOMPARE(changed.count(), 0);
        // Running with a report.
        f.run("w1", "a", "2026-11-10T11:00:00.000Z");
        changed.clear();
        f.test.clock()->tick();
        QCOMPARE(changed.count(), 1);
    }

    void aRunningWorklogWithoutAReportStaysQuietOnTicks() {
        Fixture f;
        f.run("w1", "a", "2026-11-10T11:00:00.000Z");
        QSignalSpy changed(&f.totals, &DailyTotalsStore::totalsChanged);
        f.test.clock()->tick();
        QCOMPARE(changed.count(), 0);
    }

    void trackingAndCatalogChangesAnnounceTotals() {
        Fixture f;
        QSignalSpy changed(&f.totals, &DailyTotalsStore::totalsChanged);
        f.run("w1", "a", "2026-11-10T11:00:00.000Z");
        QCOMPARE(changed.count(), 1);
        f.catalog.accept({item("a", "Alpha")});
        QCOMPARE(changed.count(), 2);
    }

    // Reset

    void resetMakesTheTotalsUnknown() {
        Fixture f;
        f.totals.accept(report("2026-11-10T12:00:00.000Z", {{"a", 60 * second}}));
        QSignalSpy changed(&f.totals, &DailyTotalsStore::totalsChanged);
        f.totals.reset();
        QVERIFY(!f.totals.known());
        QCOMPARE(f.totals.secondsFor("a"), -1);
        QCOMPARE(changed.count(), 1);
        f.totals.reset();
        QCOMPARE(changed.count(), 1);
    }

    void acceptingNoReportMakesTheTotalsUnknown() {
        Fixture f;
        f.totals.accept(report("2026-11-10T12:00:00.000Z", {{"a", 60 * second}}));
        QSignalSpy changed(&f.totals, &DailyTotalsStore::totalsChanged);
        f.totals.accept(std::nullopt);
        QVERIFY(!f.totals.known());
        QCOMPARE(changed.count(), 1);
    }

    void aNewDayDropsYesterdaysReport() {
        Fixture f;
        f.totals.accept(report("2026-11-10T12:00:00.000Z", {{"a", 60 * second}}));
        QSignalSpy changed(&f.totals, &DailyTotalsStore::totalsChanged);
        // 23:30 UTC is already the next day in the test zone.
        f.test.advanceTo("2026-11-10T23:30:00.000Z");
        QVERIFY(!f.totals.known());
        QVERIFY(changed.count() >= 1);
        QCOMPARE(f.totals.secondsFor("a"), -1);
    }
};

REGISTER_TEST(DailyTotalsStoreTests)
#include "DailyTotalsStoreTests.moc"

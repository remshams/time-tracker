#include <QJsonArray>
#include <QSignalSpy>
#include <QTest>

#include "FakeTrackerGateway.h"
#include "Features/TaskCatalog/TaskCatalogStore.h"
#include "Features/WorklogHistory/WorklogHistoryStore.h"
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

BridgeReply page(const QJsonArray &worklogs, const QString &next = QString(), bool reset = false) {
    return BridgeReply::success(historyJson(worklogs, next, reset));
}

QJsonObject log(const QString &id, const QString &start, const QString &end = QString()) {
    return worklogJson(id, "a", start, end);
}

// The clock shows 12:00 UTC, which is 13:00 local on 10 November.
struct Fixture {
    FakeTrackerGateway gateway;
    TestClock test{"2026-11-10T12:00:00.000Z"};
    TaskCatalogStore catalog{test.clock()};
    WorklogHistoryStore history{&gateway, &catalog, test.clock()};

    // Selects task "a", which asks for its history, and answers with `worklogs`.
    void load(const QJsonArray &worklogs, const QString &next = QString()) {
        catalog.accept({item("a", "Alpha"), item("b", "Beta")});
        gateway.respond("history", page(worklogs, next));
    }
};

} // namespace

class WorklogHistoryStoreTests : public QObject {
    Q_OBJECT
private slots:
    void reloadAfterClearReadsTheSelectedTask() {
        Fixture f;
        f.load({log("w1", "2026-11-10T08:00:00.000Z", "2026-11-10T09:00:00.000Z")});
        f.history.clear();
        QVERIFY(f.history.taskId().isEmpty());
        f.history.reload();
        QCOMPARE(f.history.taskId(), QStringLiteral("a"));
        QCOMPARE(f.gateway.lastArgs("history"), (QStringList{"a", QString()}));
    }

    void theRunningPartCarriesItsOffsetForTheLiveTimer() {
        Fixture f;
        // Started 23:00 local yesterday, so today's part begins one hour into the worklog.
        f.load({log("w1", "2026-11-09T22:00:00.000Z")});
        const QVariantMap today = f.history.days().first().toMap();
        QCOMPARE(today.value("runningOffset").toLongLong(), 3600);
        QCOMPARE(today.value("runningSeconds").toLongLong(), 13 * 3600);
        const QVariantMap entry = today.value("entries").toList().first().toMap();
        QCOMPARE(entry.value("runningOffset").toLongLong(), 3600);
    }

    void theRunningRowRebuildsTheDaysOncePerMinute() {
        Fixture f;
        f.load({log("w1", "2026-11-10T11:00:00.000Z")});
        QSignalSpy days(&f.history, &WorklogHistoryStore::daysChanged);
        f.test.advanceTo("2026-11-10T12:00:01.000Z");
        f.test.advanceTo("2026-11-10T12:00:30.000Z");
        QCOMPARE(days.count(), 1);
        f.test.advanceTo("2026-11-10T12:01:00.000Z");
        QCOMPARE(days.count(), 2);
    }

    void rowsOfAWorklogPastMidnightHaveTheirOwnKeys() {
        Fixture f;
        f.load({log("w1", "2026-11-09T22:00:00.000Z")});
        const QVariantList days = f.history.days();
        const QVariantMap today = days.at(0).toMap();
        const QVariantMap todayRow = today.value("entries").toList().first().toMap();
        const QVariantMap yesterdayRow = days.at(1).toMap().value("entries").toList().first().toMap();
        QCOMPARE(todayRow.value("rowKey").toString(), QStringLiteral("w1@2026-11-10"));
        QCOMPARE(yesterdayRow.value("rowKey").toString(), QStringLiteral("w1@2026-11-09"));
        // Both rows belong to a running worklog, so neither offers Delete.
        QVERIFY(todayRow.value("worklogRunning").toBool());
        QVERIFY(yesterdayRow.value("worklogRunning").toBool());
        QCOMPARE(today.value("runningWorklogId").toString(), QStringLiteral("w1"));
        QVERIFY(days.at(1).toMap().value("runningWorklogId").toString().isEmpty());
    }

    void reloadWithoutASelectionDoesNothing() {
        Fixture f;
        f.history.reload();
        QVERIFY(f.gateway.calls.isEmpty());
    }

    void startsEmpty() {
        Fixture f;
        QVERIFY(f.history.taskId().isEmpty());
        QVERIFY(!f.history.loading());
        QVERIFY(!f.history.loaded());
        QVERIFY(!f.history.hasMore());
        QCOMPARE(f.history.count(), 0);
        QVERIFY(f.history.days().isEmpty());
        QVERIFY(f.history.error().isEmpty());
        QVERIFY(f.history.worklogs().isEmpty());
    }

    // Loading

    void selectingATaskLoadsItsFirstPage() {
        Fixture f;
        QSignalSpy loading(&f.history, &WorklogHistoryStore::loadingChanged);
        f.catalog.accept({item("a", "Alpha"), item("b", "Beta")});
        QCOMPARE(f.history.taskId(), QStringLiteral("a"));
        QCOMPARE(f.gateway.count("history"), 1);
        QCOMPARE(f.gateway.lastArgs("history"), QStringList({"a", ""}));
        QVERIFY(f.history.loading());
        QVERIFY(!f.history.loaded());
        QCOMPARE(loading.count(), 1);
        f.gateway.respond("history", page({log("w1", "2026-11-10T09:00:00.000Z", "2026-11-10T10:00:00.000Z")}));
        QVERIFY(!f.history.loading());
        QVERIFY(f.history.loaded());
        QCOMPARE(f.history.count(), 1);
        QVERIFY(!f.history.hasMore());
        QCOMPARE(loading.count(), 2);
    }

    void changingTheSelectionLoadsTheNewTaskAndDropsTheOldRows() {
        Fixture f;
        f.load({log("w1", "2026-11-10T09:00:00.000Z", "2026-11-10T10:00:00.000Z")});
        f.catalog.select("b");
        QCOMPARE(f.history.taskId(), QStringLiteral("b"));
        QCOMPARE(f.history.count(), 0);
        QVERIFY(!f.history.loaded());
        QCOMPARE(f.gateway.lastArgs("history"), QStringList({"b", ""}));
    }

    void selectingTheSameTaskAgainDoesNotReload() {
        Fixture f;
        f.load({});
        f.catalog.accept({item("a", "Alpha renamed"), item("b", "Beta")});
        QCOMPARE(f.gateway.count("history"), 1);
    }

    void anEmptyCatalogClearsTheHistory() {
        Fixture f;
        f.load({log("w1", "2026-11-10T09:00:00.000Z", "2026-11-10T10:00:00.000Z")});
        f.catalog.accept({});
        QVERIFY(f.history.taskId().isEmpty());
        QCOMPARE(f.history.count(), 0);
        QCOMPARE(f.gateway.count("history"), 1);
    }

    void anAnswerForAnEarlierTaskIsDropped() {
        Fixture f;
        f.catalog.accept({item("a", "Alpha"), item("b", "Beta")});
        f.catalog.select("b");
        // The answer for "a" arrives after the user moved on.
        f.gateway.respond("history", page({log("w1", "2026-11-10T09:00:00.000Z", "2026-11-10T10:00:00.000Z")}));
        QCOMPARE(f.history.count(), 0);
        QVERIFY(f.history.loading());
        f.gateway.respond("history",
                          page({worklogJson("w9", "b", "2026-11-10T08:00:00.000Z", "2026-11-10T08:30:00.000Z")}));
        QCOMPARE(f.history.count(), 1);
        QCOMPARE(f.history.worklogs().first().id, QStringLiteral("w9"));
        QVERIFY(!f.history.loading());
    }

    void aFailureForAnEarlierTaskIsDropped() {
        Fixture f;
        f.catalog.accept({item("a", "Alpha"), item("b", "Beta")});
        f.catalog.select("b");
        f.gateway.respond("history", BridgeReply::failure("old failure"));
        QVERIFY(f.history.error().isEmpty());
    }

    void anAnswerAfterTheStoreIsGoneIsIgnored() {
        FakeTrackerGateway gateway;
        TestClock test;
        TaskCatalogStore catalog(test.clock());
        auto *history = new WorklogHistoryStore(&gateway, &catalog, test.clock());
        catalog.accept({item("a", "Alpha")});
        delete history;
        QVERIFY(gateway.respond("history", page({})));
    }

    // Reloading

    void reloadKeepsTheShownRowsUntilTheNewPageArrives() {
        Fixture f;
        f.load({log("w1", "2026-11-10T09:00:00.000Z", "2026-11-10T10:00:00.000Z")});
        f.history.reload();
        QVERIFY(f.history.loading());
        QCOMPARE(f.history.count(), 1);
        QCOMPARE(f.gateway.lastArgs("history"), QStringList({"a", ""}));
        f.gateway.respond("history", page({log("w2", "2026-11-10T09:30:00.000Z", "2026-11-10T10:00:00.000Z"),
                                           log("w1", "2026-11-10T09:00:00.000Z", "2026-11-10T09:30:00.000Z")}));
        QCOMPARE(f.history.count(), 2);
        QCOMPARE(f.history.worklogs().first().id, QStringLiteral("w2"));
    }

    void reloadWithoutATaskDoesNothing() {
        Fixture f;
        f.history.reload();
        QVERIFY(f.gateway.calls.isEmpty());
    }

    // Paging

    void loadMoreAsksForTheNextPageAndAppends() {
        Fixture f;
        f.load({log("w3", "2026-11-10T09:00:00.000Z", "2026-11-10T10:00:00.000Z")}, "cursor-1");
        QVERIFY(f.history.hasMore());
        f.history.loadMore();
        QCOMPARE(f.gateway.lastArgs("history"), QStringList({"a", "cursor-1"}));
        QVERIFY(f.history.loading());
        f.gateway.respond("history",
                          page({log("w2", "2026-11-09T09:00:00.000Z", "2026-11-09T10:00:00.000Z")}, "cursor-2"));
        QCOMPARE(f.history.count(), 2);
        QCOMPARE(f.history.worklogs().last().id, QStringLiteral("w2"));
        QVERIFY(f.history.hasMore());
        f.history.loadMore();
        QCOMPARE(f.gateway.lastArgs("history"), QStringList({"a", "cursor-2"}));
        f.gateway.respond("history", page({log("w1", "2026-11-08T09:00:00.000Z", "2026-11-08T10:00:00.000Z")}));
        QCOMPARE(f.history.count(), 3);
        QVERIFY(!f.history.hasMore());
    }

    void loadMoreWithoutAnotherPageDoesNothing() {
        Fixture f;
        f.load({});
        f.history.loadMore();
        QCOMPARE(f.gateway.count("history"), 1);
    }

    void loadMoreWhileLoadingDoesNothing() {
        Fixture f;
        f.load({}, "cursor-1");
        f.history.loadMore();
        f.history.loadMore();
        QCOMPARE(f.gateway.count("history"), 2);
    }

    void aResetPageReplacesTheRowsEvenForALaterPage() {
        Fixture f;
        f.load({log("w3", "2026-11-10T09:00:00.000Z", "2026-11-10T10:00:00.000Z")}, "cursor-1");
        f.history.loadMore();
        f.gateway.respond("history",
                          page({log("w9", "2026-11-10T11:00:00.000Z", "2026-11-10T11:30:00.000Z")}, QString(), true));
        QCOMPARE(f.history.count(), 1);
        QCOMPARE(f.history.worklogs().first().id, QStringLiteral("w9"));
        QVERIFY(!f.history.hasMore());
    }

    void aPageWithoutResetAppendsToTheRows() {
        Fixture f;
        f.load({log("w3", "2026-11-10T09:00:00.000Z", "2026-11-10T10:00:00.000Z")}, "cursor-1");
        f.history.loadMore();
        f.gateway.respond("history",
                          page({log("w2", "2026-11-09T09:00:00.000Z", "2026-11-09T10:00:00.000Z")}, QString(), false));
        QCOMPARE(f.history.count(), 2);
    }

    void rowsAnnounceThemselvesWhenAPageArrives() {
        Fixture f;
        f.catalog.accept({item("a", "Alpha")});
        QSignalSpy rows(&f.history, &WorklogHistoryStore::rowsChanged);
        QSignalSpy days(&f.history, &WorklogHistoryStore::daysChanged);
        f.gateway.respond("history", page({log("w1", "2026-11-10T09:00:00.000Z", "2026-11-10T10:00:00.000Z")}));
        QCOMPARE(rows.count(), 1);
        QCOMPARE(days.count(), 1);
    }

    // Errors

    void aFailedLoadShowsTheErrorAndKeepsTheRows() {
        Fixture f;
        f.load({log("w1", "2026-11-10T09:00:00.000Z", "2026-11-10T10:00:00.000Z")});
        QSignalSpy error(&f.history, &WorklogHistoryStore::errorChanged);
        f.history.reload();
        f.gateway.respond("history", BridgeReply::failure("Server unreachable"));
        QCOMPARE(f.history.error(), QStringLiteral("Server unreachable"));
        QCOMPARE(error.count(), 1);
        QVERIFY(!f.history.loading());
        QCOMPARE(f.history.count(), 1);
        QVERIFY(f.history.loaded());
    }

    void anUnreadablePageShowsAGenericError() {
        Fixture f;
        f.catalog.accept({item("a", "Alpha")});
        f.gateway.respond("history", BridgeReply::success(QJsonValue::Null));
        QCOMPARE(f.history.error(), QStringLiteral("The history could not be read"));
        QVERIFY(!f.history.loaded());
    }

    void aSuccessfulLoadClearsTheError() {
        Fixture f;
        f.catalog.accept({item("a", "Alpha")});
        f.gateway.respond("history", BridgeReply::failure("Server unreachable"));
        QSignalSpy error(&f.history, &WorklogHistoryStore::errorChanged);
        f.history.reload();
        f.gateway.respond("history", page({}));
        QVERIFY(f.history.error().isEmpty());
        QCOMPARE(error.count(), 1);
        QVERIFY(f.history.loaded());
    }

    // Clearing

    void clearDropsRowsAndStaleAnswers() {
        Fixture f;
        f.catalog.accept({item("a", "Alpha")});
        QSignalSpy rows(&f.history, &WorklogHistoryStore::rowsChanged);
        QSignalSpy days(&f.history, &WorklogHistoryStore::daysChanged);
        f.history.clear();
        QVERIFY(f.history.taskId().isEmpty());
        QVERIFY(!f.history.loading());
        QVERIFY(!f.history.loaded());
        QCOMPARE(rows.count(), 1);
        QCOMPARE(days.count(), 1);
        // The answer of the request that was in flight no longer applies.
        f.gateway.respond("history", page({log("w1", "2026-11-10T09:00:00.000Z", "2026-11-10T10:00:00.000Z")}));
        QCOMPARE(f.history.count(), 0);
        QVERIFY(!f.history.loaded());
    }

    void clearRemovesTheError() {
        Fixture f;
        f.catalog.accept({item("a", "Alpha")});
        f.gateway.respond("history", BridgeReply::failure("Server unreachable"));
        f.history.clear();
        QVERIFY(f.history.error().isEmpty());
    }

    // Days

    void groupsWorklogsByLocalDay() {
        Fixture f;
        f.load({log("w1", "2026-11-10T11:00:00.000Z"),
                log("w2", "2026-11-10T08:00:00.000Z", "2026-11-10T09:30:00.000Z"),
                log("w3", "2026-11-09T09:00:00.000Z", "2026-11-09T10:00:00.000Z"),
                log("w4", "2026-11-03T09:00:00.000Z", "2026-11-03T09:10:00.000Z")});
        const QVariantList days = f.history.days();
        QCOMPARE(days.size(), 3);
        const QVariantMap today = days.at(0).toMap();
        QCOMPARE(today.value("label").toString(), QStringLiteral("Today"));
        QCOMPARE(today.value("date").toDate(), QDate(2026, 11, 10));
        // The running worklog counts up to now: one hour. Then 90 minutes.
        QCOMPARE(today.value("totalSeconds").toLongLong(), 3600 + 5400);
        QCOMPARE(today.value("entries").toList().size(), 2);
        const QVariantMap yesterday = days.at(1).toMap();
        QCOMPARE(yesterday.value("label").toString(), QStringLiteral("Yesterday"));
        QCOMPARE(yesterday.value("totalSeconds").toLongLong(), 3600);
        QCOMPARE(yesterday.value("entries").toList().size(), 1);
        const QVariantMap older = days.at(2).toMap();
        QCOMPARE(older.value("totalSeconds").toLongLong(), 600);
        QVERIFY(older.value("label").toString() != QStringLiteral("Today"));
    }

    void aDayHoldsEntriesOnlyFromItsOwnDate() {
        Fixture f;
        f.load({log("w1", "2026-11-10T08:00:00.000Z", "2026-11-10T09:00:00.000Z"),
                log("w2", "2026-11-09T08:00:00.000Z", "2026-11-09T09:00:00.000Z")});
        const QVariantList days = f.history.days();
        QCOMPARE(days.at(0).toMap().value("entries").toList().first().toMap().value("worklogId").toString(),
                 QStringLiteral("w1"));
        QCOMPARE(days.at(1).toMap().value("entries").toList().first().toMap().value("worklogId").toString(),
                 QStringLiteral("w2"));
        QCOMPARE(days.at(1).toMap().value("entries").toList().size(), 1);
    }

    void entriesCarryLabelsDurationAndAxisPosition() {
        Fixture f;
        f.load(
            {log("w1", "2026-11-10T11:00:00.000Z"), log("w2", "2026-11-10T08:00:00.000Z", "2026-11-10T09:30:00.000Z")});
        // Local start hours are 12 and 9; the axis keeps its minimum of eight hours.
        QCOMPARE(f.history.axisStartHour(), 9);
        QCOMPARE(f.history.axisEndHour(), 17);
        const QVariantList entries = f.history.days().first().toMap().value("entries").toList();
        const QVariantMap running = entries.at(0).toMap();
        QCOMPARE(running.value("worklogId").toString(), QStringLiteral("w1"));
        QVERIFY(running.value("running").toBool());
        QCOMPARE(running.value("startLabel").toString(), QStringLiteral("12:00"));
        QCOMPARE(running.value("endLabel").toString(), QStringLiteral("now"));
        QCOMPARE(running.value("seconds").toLongLong(), 3600);
        QCOMPARE(running.value("from").toDouble(), 0.375);
        QCOMPARE(running.value("to").toDouble(), 0.5);
        const QVariantMap closed = entries.at(1).toMap();
        QVERIFY(!closed.value("running").toBool());
        QCOMPARE(closed.value("startLabel").toString(), QStringLiteral("09:00"));
        QCOMPARE(closed.value("endLabel").toString(), QStringLiteral("10:30"));
        QCOMPARE(closed.value("seconds").toLongLong(), 5400);
        QCOMPARE(closed.value("from").toDouble(), 0.0);
        QCOMPARE(closed.value("to").toDouble(), 0.1875);
    }

    void aWorklogPastMidnightShowsOnBothDays() {
        Fixture f;
        // 22:00 to 01:00 local: one hour on the 10th, two hours on the 9th.
        f.load({log("w1", "2026-11-09T21:00:00.000Z", "2026-11-10T00:00:00.000Z")});
        const QVariantList days = f.history.days();
        QCOMPARE(days.size(), 2);
        const QVariantMap later = days.at(0).toMap();
        QCOMPARE(later.value("totalSeconds").toLongLong(), 3600);
        const QVariantMap morning = later.value("entries").toList().first().toMap();
        QCOMPARE(morning.value("worklogId").toString(), QStringLiteral("w1"));
        QCOMPARE(morning.value("startLabel").toString(), QStringLiteral("00:00"));
        QCOMPARE(morning.value("endLabel").toString(), QStringLiteral("01:00"));
        QCOMPARE(morning.value("seconds").toLongLong(), 3600);
        QCOMPARE(morning.value("from").toDouble(), 0.0);
        const QVariantMap earlier = days.at(1).toMap();
        QCOMPARE(earlier.value("totalSeconds").toLongLong(), 2 * 3600);
        const QVariantMap evening = earlier.value("entries").toList().first().toMap();
        QCOMPARE(evening.value("startLabel").toString(), QStringLiteral("22:00"));
        QCOMPARE(evening.value("endLabel").toString(), QStringLiteral("24:00"));
        QCOMPARE(evening.value("to").toDouble(), 1.0);
        // The axis covers both parts: midnight to the end of the day.
        QCOMPARE(f.history.axisStartHour(), 0);
        QCOMPARE(f.history.axisEndHour(), 24);
    }

    void aRunningWorklogFromYesterdayIsRunningOnlyOnToday() {
        Fixture f;
        // The clock is 13:00 local on the 10th; the worklog started at 23:00 local on the 9th.
        f.load({log("w1", "2026-11-09T22:00:00.000Z")});
        const QVariantList days = f.history.days();
        QCOMPARE(days.size(), 2);
        const QVariantMap today = days.at(0).toMap().value("entries").toList().first().toMap();
        QVERIFY(today.value("running").toBool());
        QCOMPARE(today.value("endLabel").toString(), QStringLiteral("now"));
        const QVariantMap yesterday = days.at(1).toMap().value("entries").toList().first().toMap();
        QVERIFY(!yesterday.value("running").toBool());
        QCOMPARE(yesterday.value("endLabel").toString(), QStringLiteral("24:00"));
    }

    void aWorklogThatEndsBeforeItStartsHasNoLengthAndStaysOnTheAxis() {
        Fixture f;
        f.load({log("w1", "2026-11-10T09:00:00.000Z", "2026-11-09T09:00:00.000Z")});
        const QVariantMap entry = f.history.days().first().toMap().value("entries").toList().first().toMap();
        QCOMPARE(entry.value("seconds").toLongLong(), 0);
        QCOMPARE(entry.value("to").toDouble(), 0.0);
        QCOMPARE(entry.value("from").toDouble(), 0.0);
    }

    void emptyHistoryUsesTheDefaultAxis() {
        Fixture f;
        f.load({});
        QCOMPARE(f.history.axisStartHour(), 8);
        QCOMPARE(f.history.axisEndHour(), 18);
        QVERIFY(f.history.days().isEmpty());
    }

    void theAxisSpansAtLeastEightHours() {
        Fixture f;
        f.load({log("w1", "2026-11-10T08:00:00.000Z", "2026-11-10T08:30:00.000Z")});
        // 09:00 to 09:30 local: the end hour is 10 but the axis keeps eight hours.
        QCOMPARE(f.history.axisStartHour(), 9);
        QCOMPARE(f.history.axisEndHour(), 17);
        const QVariantMap entry = f.history.days().first().toMap().value("entries").toList().first().toMap();
        QCOMPARE(entry.value("from").toDouble(), 0.0);
        QCOMPARE(entry.value("to").toDouble(), 0.0625);
    }

    void theAxisGrowsToTheLatestEnd() {
        Fixture f;
        f.load({log("w1", "2026-11-10T08:00:00.000Z", "2026-11-10T08:30:00.000Z"),
                log("w2", "2026-11-10T14:00:00.000Z", "2026-11-10T16:10:00.000Z")});
        // The latest end is 17:10 local, so the axis ends at 18.
        QCOMPARE(f.history.axisEndHour(), 18);
    }

    void theAxisWidensToEarlierHoursNearMidnight() {
        Fixture f;
        // 21:00 to 23:30 local: the axis can't pass midnight, so it starts earlier.
        f.load({log("w1", "2026-11-10T20:00:00.000Z", "2026-11-10T22:30:00.000Z")});
        QCOMPARE(f.history.axisStartHour(), 16);
        QCOMPARE(f.history.axisEndHour(), 24);
    }

    void aNewDayRelabelsTheDays() {
        Fixture f;
        f.load({log("w1", "2026-11-10T08:00:00.000Z", "2026-11-10T09:00:00.000Z")});
        QSignalSpy days(&f.history, &WorklogHistoryStore::daysChanged);
        f.test.advanceTo("2026-11-11T08:00:00.000Z");
        QCOMPARE(days.count(), 1);
        QCOMPARE(f.history.days().first().toMap().value("label").toString(), QStringLiteral("Yesterday"));
    }

    void ticksAnnounceDaysOnlyWhileTheNewestWorklogRuns() {
        Fixture f;
        f.load({log("w1", "2026-11-10T08:00:00.000Z", "2026-11-10T09:00:00.000Z")});
        QSignalSpy days(&f.history, &WorklogHistoryStore::daysChanged);
        f.test.clock()->tick();
        QCOMPARE(days.count(), 0);
        f.history.clear();
        days.clear();
        f.test.clock()->tick();
        QCOMPARE(days.count(), 0);
        f.catalog.select("b");
        f.catalog.select("a");
        f.gateway.respond("history", page({}));
        f.gateway.respond("history", page({log("w2", "2026-11-10T11:00:00.000Z")}));
        days.clear();
        f.test.clock()->tick();
        QCOMPARE(days.count(), 1);
    }

    // Single worklogs

    void worklogLooksUpById() {
        Fixture f;
        f.load({log("w1", "2026-11-10T11:00:00.000000Z"),
                log("w2", "2026-11-10T08:00:00.000000Z", "2026-11-10T09:30:00.000000Z")});
        const std::optional<WorklogItem> closed = f.history.worklog("w2");
        QVERIFY(closed);
        QCOMPARE(closed->endText, QStringLiteral("2026-11-10T09:30:00.000000Z"));
        QVERIFY(!f.history.worklog("missing"));
    }
};

REGISTER_TEST(WorklogHistoryStoreTests)
#include "WorklogHistoryStoreTests.moc"

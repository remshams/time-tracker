#include <QJsonArray>
#include <QJsonObject>
#include <QTest>
#include <QTimeZone>

#include "Contracts/Models.h"
#include "TestRegistry.h"
#include "TestSupport.h"

using namespace TestSupport;

class ModelsTests : public QObject {
    Q_OBJECT
private slots:
    // Timestamps

    void formatWritesUtcWithMilliseconds() {
        QCOMPARE(Timestamps::format(utc("2026-11-10T12:30:45.123Z")), QStringLiteral("2026-11-10T12:30:45.123Z"));
        // A local time converts to UTC; the test zone is one hour ahead.
        const QDateTime local(QDate(2026, 11, 10), QTime(13, 30, 45, 123), QTimeZone::systemTimeZone());
        QCOMPARE(Timestamps::format(local), QStringLiteral("2026-11-10T12:30:45.123Z"));
    }

    void parseRoundTrips() {
        const QString text = QStringLiteral("2026-11-10T12:30:45.123Z");
        const std::optional<QDateTime> parsed = Timestamps::parse(text);
        QVERIFY(parsed);
        QCOMPARE(parsed->timeSpec(), Qt::UTC);
        QCOMPARE(Timestamps::format(*parsed), text);
    }

    void parseAcceptsTimestampsWithoutMilliseconds() {
        const std::optional<QDateTime> parsed = Timestamps::parse(QStringLiteral("2026-11-10T12:30:45Z"));
        QVERIFY(parsed);
        QCOMPARE(Timestamps::format(*parsed), QStringLiteral("2026-11-10T12:30:45.000Z"));
    }

    void parseKeepsMicrosecondPrecisionAsMilliseconds() {
        const std::optional<QDateTime> parsed = Timestamps::parse(QStringLiteral("2026-11-10T12:30:45.123456Z"));
        QVERIFY(parsed);
        QCOMPARE(Timestamps::format(*parsed), QStringLiteral("2026-11-10T12:30:45.123Z"));
    }

    void parseConvertsOffsetsToUtc() {
        const std::optional<QDateTime> parsed = Timestamps::parse(QStringLiteral("2026-11-10T14:00:00.000+02:00"));
        QVERIFY(parsed);
        QCOMPARE(Timestamps::format(*parsed), QStringLiteral("2026-11-10T12:00:00.000Z"));
    }

    void parseRejectsText() {
        QVERIFY(!Timestamps::parse(QStringLiteral("")));
        QVERIFY(!Timestamps::parse(QStringLiteral("yesterday")));
    }

    // TaskItem

    void taskReadsEveryField() {
        const std::optional<TaskItem> task =
            TaskItem::fromJson(taskJson("t1", "Write", true, "2026-11-10T09:00:00.000Z"));
        QVERIFY(task);
        QCOMPARE(task->id, QStringLiteral("t1"));
        QCOMPARE(task->name, QStringLiteral("Write"));
        QVERIFY(task->archived);
        QVERIFY(task->latestStart);
        QCOMPARE(*task->latestStart, utc("2026-11-10T09:00:00.000Z"));
    }

    void taskDefaultsToActiveWithoutLatestStart() {
        const std::optional<TaskItem> task = TaskItem::fromJson(taskJson("t1", "Write"));
        QVERIFY(task);
        QVERIFY(!task->archived);
        QVERIFY(!task->latestStart);
    }

    void taskIgnoresANullLatestStart() {
        QJsonObject json = taskJson("t1", "Write");
        json.insert("latestStart", QJsonValue::Null);
        const std::optional<TaskItem> task = TaskItem::fromJson(json);
        QVERIFY(task);
        QVERIFY(!task->latestStart);
    }

    void taskIgnoresAnUnreadableLatestStart() {
        const std::optional<TaskItem> task = TaskItem::fromJson(taskJson("t1", "Write", false, "soon"));
        QVERIFY(task);
        QVERIFY(!task->latestStart);
    }

    void taskNeedsAnId() {
        QVERIFY(!TaskItem::fromJson(taskJson("", "Write")));
        QVERIFY(!TaskItem::fromJson(QJsonObject{{"name", "Write"}}));
        QVERIFY(!TaskItem::fromJson(QJsonValue::Null));
    }

    void taskWithoutANameKeepsAnEmptyName() {
        const std::optional<TaskItem> task = TaskItem::fromJson(QJsonObject{{"id", "t1"}});
        QVERIFY(task);
        QVERIFY(task->name.isEmpty());
    }

    // WorklogItem

    void runningWorklogHasNoEnd() {
        const std::optional<WorklogItem> worklog =
            WorklogItem::fromJson(worklogJson("w1", "t1", "2026-11-10T09:00:00.000Z"));
        QVERIFY(worklog);
        QVERIFY(worklog->isRunning());
        QVERIFY(!worklog->end);
        QVERIFY(worklog->endText.isEmpty());
        QCOMPARE(worklog->start, utc("2026-11-10T09:00:00.000Z"));
        QCOMPARE(worklog->startText, QStringLiteral("2026-11-10T09:00:00.000Z"));
    }

    void nullEndMeansRunning() {
        QJsonObject json = worklogJson("w1", "t1", "2026-11-10T09:00:00.000Z");
        json.insert("end", QJsonValue::Null);
        const std::optional<WorklogItem> worklog = WorklogItem::fromJson(json);
        QVERIFY(worklog);
        QVERIFY(worklog->isRunning());
    }

    void finishedWorklogKeepsTheTimestampText() {
        const std::optional<WorklogItem> worklog = WorklogItem::fromJson(
            worklogJson("w1", "t1", "2026-11-10T09:00:00.000000Z", "2026-11-10T10:30:00.500000Z"));
        QVERIFY(worklog);
        QVERIFY(!worklog->isRunning());
        QCOMPARE(worklog->id, QStringLiteral("w1"));
        QCOMPARE(worklog->taskId, QStringLiteral("t1"));
        // Guarded writes send back the text exactly as the bridge wrote it.
        QCOMPARE(worklog->startText, QStringLiteral("2026-11-10T09:00:00.000000Z"));
        QCOMPARE(worklog->endText, QStringLiteral("2026-11-10T10:30:00.500000Z"));
        QCOMPARE(*worklog->end, utc("2026-11-10T10:30:00.500Z"));
    }

    void worklogNeedsAnId() { QVERIFY(!WorklogItem::fromJson(worklogJson("", "t1", "2026-11-10T09:00:00.000Z"))); }

    void worklogNeedsATask() { QVERIFY(!WorklogItem::fromJson(worklogJson("w1", "", "2026-11-10T09:00:00.000Z"))); }

    void worklogNeedsAReadableStart() {
        QVERIFY(!WorklogItem::fromJson(worklogJson("w1", "t1", "")));
        QVERIFY(!WorklogItem::fromJson(worklogJson("w1", "t1", "later")));
    }

    void worklogRejectsAnUnreadableEnd() {
        QVERIFY(!WorklogItem::fromJson(worklogJson("w1", "t1", "2026-11-10T09:00:00.000Z", "later")));
    }

    void worklogRejectsANonObject() {
        QVERIFY(!WorklogItem::fromJson(QJsonValue::Null));
        QVERIFY(!WorklogItem::fromJson(QJsonValue(5)));
    }

    // DailyReport

    void reportReadsRowsAndCutoff() {
        const QJsonObject json{{"now", "2026-11-10T12:00:00.000Z"},
                               {"rows", QJsonArray{QJsonObject{{"taskId", "a"}, {"durationMicroseconds", 3600000000.0}},
                                                   QJsonObject{{"taskId", "b"}, {"durationMicroseconds", 1500000}}}}};
        const std::optional<DailyReport> report = DailyReport::fromJson(json);
        QVERIFY(report);
        QCOMPARE(report->cutoff, utc("2026-11-10T12:00:00.000Z"));
        QCOMPARE(report->microseconds.size(), 2);
        QCOMPARE(report->microseconds.value("a"), qint64(3600000000));
        QCOMPARE(report->microseconds.value("b"), qint64(1500000));
    }

    void reportWithoutRowsIsEmpty() {
        const std::optional<DailyReport> report =
            DailyReport::fromJson(QJsonObject{{"now", "2026-11-10T12:00:00.000Z"}});
        QVERIFY(report);
        QVERIFY(report->microseconds.isEmpty());
    }

    void reportNeedsAReadableCutoff() {
        QVERIFY(!DailyReport::fromJson(QJsonObject{{"rows", QJsonArray()}}));
        QVERIFY(!DailyReport::fromJson(QJsonObject{{"now", "noon"}}));
        QVERIFY(!DailyReport::fromJson(QJsonValue::Null));
    }

    // Overview

    void overviewReadsEveryPart() {
        const QJsonObject json = overviewJson(QJsonArray{taskJson("a", "Alpha"), taskJson("b", "Beta", true)},
                                              worklogJson("w1", "a", "2026-11-10T11:00:00.000Z"),
                                              "2026-11-10T12:00:00.000Z", QJsonObject{{"a", 60000000}});
        const Overview overview = Overview::fromJson(json);
        QVERIFY(overview.tasks);
        QCOMPARE(overview.tasks->size(), 2);
        QCOMPARE(overview.tasks->at(1).id, QStringLiteral("b"));
        QVERIFY(overview.trackingKnown);
        QVERIFY(overview.running);
        QCOMPARE(overview.running->id, QStringLiteral("w1"));
        QVERIFY(overview.report);
        QCOMPARE(overview.report->microseconds.value("a"), qint64(60000000));
    }

    void overviewWithIdleTrackingIsKnownWithoutARunningWorklog() {
        const Overview overview = Overview::fromJson(overviewJson(QJsonArray(), QJsonValue::Null, QString()));
        QVERIFY(overview.trackingKnown);
        QVERIFY(!overview.running);
        QVERIFY(overview.tasks);
        QVERIFY(overview.tasks->isEmpty());
        QVERIFY(!overview.report);
    }

    void overviewSkipsTasksItCannotRead() {
        const Overview overview = Overview::fromJson(
            overviewJson(QJsonArray{taskJson("", "No id"), taskJson("a", "Alpha"), 7}, QJsonValue::Null, QString()));
        QVERIFY(overview.tasks);
        QCOMPARE(overview.tasks->size(), 1);
        QCOMPARE(overview.tasks->first().id, QStringLiteral("a"));
    }

    void overviewMissingPartsStayUnknown() {
        const Overview overview = Overview::fromJson(QJsonObject());
        QVERIFY(!overview.tasks);
        QVERIFY(!overview.trackingKnown);
        QVERIFY(!overview.running);
        QVERIFY(!overview.report);
    }

    void overviewPartsThatAreNotObjectsStayUnknown() {
        const Overview overview =
            Overview::fromJson(QJsonObject{{"tasks", QJsonArray()}, {"tracking", "idle"}, {"report", 3}});
        QVERIFY(!overview.tasks);
        QVERIFY(!overview.trackingKnown);
        QVERIFY(!overview.report);
    }

    void overviewDropsAnUnreadableReport() {
        QJsonObject json = overviewJson(QJsonArray(), QJsonValue::Null, QString());
        json.insert("report", QJsonObject{{"now", "noon"}});
        const Overview overview = Overview::fromJson(json);
        QVERIFY(!overview.report);
        QVERIFY(overview.tasks);
    }

    void overviewTreatsAnUnreadableRunningWorklogAsIdle() {
        const Overview overview = Overview::fromJson(overviewJson(QJsonArray(), QJsonObject{{"id", "w1"}}, QString()));
        QVERIFY(overview.trackingKnown);
        QVERIFY(!overview.running);
    }

    void overviewOfANonObjectIsEmpty() {
        const Overview overview = Overview::fromJson(QJsonValue(4));
        QVERIFY(!overview.tasks);
        QVERIFY(!overview.trackingKnown);
        QVERIFY(!overview.report);
    }

    // HistoryPage

    void historyPageReadsWorklogsAndCursor() {
        const std::optional<HistoryPage> page =
            HistoryPage::fromJson(historyJson(QJsonArray{worklogJson("w2", "t", "2026-11-10T10:00:00.000Z"),
                                                         worklogJson("w1", "t", "2026-11-10T09:00:00.000Z")},
                                              "next-page", true));
        QVERIFY(page);
        QCOMPARE(page->worklogs.size(), 2);
        QCOMPARE(page->worklogs.first().id, QStringLiteral("w2"));
        QCOMPARE(page->nextCursor, QStringLiteral("next-page"));
        QVERIFY(page->reset);
    }

    void historyPageDefaultsToTheLastPageWithoutReset() {
        const std::optional<HistoryPage> page = HistoryPage::fromJson(QJsonObject());
        QVERIFY(page);
        QVERIFY(page->worklogs.isEmpty());
        QVERIFY(page->nextCursor.isEmpty());
        QVERIFY(!page->reset);
    }

    void historyPageSkipsWorklogsItCannotRead() {
        const std::optional<HistoryPage> page = HistoryPage::fromJson(historyJson(QJsonArray{
            worklogJson("", "t", "2026-11-10T10:00:00.000Z"), worklogJson("w1", "t", "2026-11-10T09:00:00.000Z")}));
        QVERIFY(page);
        QCOMPARE(page->worklogs.size(), 1);
        QCOMPARE(page->worklogs.first().id, QStringLiteral("w1"));
    }

    void historyPageNeedsAnObject() {
        QVERIFY(!HistoryPage::fromJson(QJsonValue::Null));
        QVERIFY(!HistoryPage::fromJson(QJsonArray()));
    }
};

REGISTER_TEST(ModelsTests)
#include "ModelsTests.moc"

#include <QDirIterator>
#include <QElapsedTimer>
#include <QJsonArray>
#include <QJsonObject>
#include <QTemporaryDir>
#include <QTest>
#include <QThread>
#include <memory>
#include <optional>

#include "Bridge/RustTrackerGateway.h"
#include "Contracts/Models.h"
#include "TestRegistry.h"

namespace {

using Done = TrackerGateway::Callback;

// Runs one gateway call and waits for its answer on the event loop.
BridgeReply await(const std::function<void(Done)> &call) {
    std::optional<BridgeReply> reply;
    call([&reply](const BridgeReply &answer) { reply = answer; });
    QElapsedTimer timer;
    timer.start();
    while (!reply && timer.elapsed() < 15000) {
        QTest::qWait(2);
    }
    return reply.value_or(BridgeReply::failure(QStringLiteral("No answer within 15 seconds")));
}

QString dataString(const BridgeReply &reply, const QString &object, const QString &key) {
    return reply.data.toObject().value(object).toObject().value(key).toString();
}

// The wall clock is fine here: the real bridge only needs times in a sensible order.
QDateTime ago(int seconds) {
    return QDateTime::currentDateTimeUtc().addSecs(-seconds);
}

} // namespace

// Runs against the real Rust bridge with a database in a temporary directory.
class RustTrackerGatewayTests : public QObject {
    Q_OBJECT
private:
    QTemporaryDir dataHome_;
    QTemporaryDir configHome_;

    BridgeReply overview(RustTrackerGateway &gateway) {
        return await([&](Done done) { gateway.refreshOverview(ago(86400), ago(-86400), ago(0), done); });
    }

    QString createTask(RustTrackerGateway &gateway, const QString &name) {
        const BridgeReply reply = await([&](Done done) { gateway.createTask(name, ago(7200), done); });
        return dataString(reply, "task", "id");
    }

    std::optional<TaskItem> findTask(RustTrackerGateway &gateway, const QString &id) {
        const Overview current = Overview::fromJson(overview(gateway).data);
        for (const TaskItem &task : current.tasks.value_or(QList<TaskItem>())) {
            if (task.id == id) {
                return task;
            }
        }
        return std::nullopt;
    }

private slots:
    void initTestCase() {
        QVERIFY(dataHome_.isValid());
        QVERIFY(configHome_.isValid());
        // The local database lives under XDG_DATA_HOME; nothing may reach the user's data.
        qputenv("XDG_DATA_HOME", dataHome_.path().toUtf8());
        qputenv("XDG_CONFIG_HOME", configHome_.path().toUtf8());
    }

    void everyCallFailsBeforeAConnectionIsOpen() {
        RustTrackerGateway gateway;
        const BridgeReply reply = overview(gateway);
        QVERIFY(!reply.ok);
        QCOMPARE(reply.error, QStringLiteral("Database is not open"));
        QVERIFY(!await([&](Done done) { gateway.history("task", QString(), done); }).ok);
        QVERIFY(!await([&](Done done) { gateway.createTask("Name", ago(0), done); }).ok);
    }

    void connectingTheLocalDatabaseCreatesItInTheDataHome() {
        RustTrackerGateway gateway;
        const BridgeReply reply = await([&](Done done) { gateway.connectTo({false, QString()}, done); });
        QVERIFY2(reply.ok, qPrintable(reply.error));
        QVERIFY(reply.data.toObject().value("compatible").toBool());
        QDirIterator files(dataHome_.path(), {"tt.db"}, QDir::Files, QDirIterator::Subdirectories);
        QVERIFY(files.hasNext());
    }

    void testingTheLocalDatabaseLeavesTheConnectionAlone() {
        RustTrackerGateway gateway;
        QVERIFY(await([&](Done done) { gateway.connectTo({false, QString()}, done); }).ok);
        const QString id = createTask(gateway, "Kept by the test");
        QVERIFY(await([&](Done done) { gateway.testConnection({false, QString()}, done); }).ok);
        QVERIFY(findTask(gateway, id));
    }

    void answersArriveOnTheGuiThread() {
        RustTrackerGateway gateway;
        QThread *answeredOn = nullptr;
        const BridgeReply reply = await([&](Done done) {
            gateway.connectTo({false, QString()}, [&answeredOn, done](const BridgeReply &answer) {
                answeredOn = QThread::currentThread();
                done(answer);
            });
        });
        QVERIFY(reply.ok);
        QCOMPARE(answeredOn, QThread::currentThread());
    }

    void callsRunInTheOrderTheyWereIssued() {
        RustTrackerGateway gateway;
        QVERIFY(await([&](Done done) { gateway.connectTo({false, QString()}, done); }).ok);
        std::optional<BridgeReply> created;
        std::optional<BridgeReply> read;
        gateway.createTask("Created in order", ago(600), [&](const BridgeReply &reply) { created = reply; });
        gateway.refreshOverview(ago(86400), ago(-86400), ago(0), [&](const BridgeReply &reply) { read = reply; });
        QTRY_VERIFY_WITH_TIMEOUT(created && read, 15000);
        QVERIFY(created->ok);
        const QString id = dataString(*created, "task", "id");
        bool found = false;
        const Overview current = Overview::fromJson(read->data);
        QVERIFY(current.tasks);
        for (const TaskItem &task : *current.tasks) {
            found = found || task.id == id;
        }
        QVERIFY(found);
    }

    void destroyingTheGatewayWithOpenCallsIsSafe() {
        auto gateway = std::make_unique<RustTrackerGateway>();
        bool answered = false;
        gateway->connectTo({false, QString()}, [&answered](const BridgeReply &) { answered = true; });
        gateway->refreshOverview(ago(86400), ago(-86400), ago(0),
                                 [&answered](const BridgeReply &) { answered = true; });
        gateway.reset();
        QTest::qWait(20);
        // The answers died with the gateway; what matters is that nothing crashed.
        Q_UNUSED(answered);
    }

    // Tasks

    void tasksCanBeCreatedRenamedArchivedAndRestored() {
        RustTrackerGateway gateway;
        QVERIFY(await([&](Done done) { gateway.connectTo({false, QString()}, done); }).ok);
        const QString id = createTask(gateway, "Write docs");
        QVERIFY(!id.isEmpty());
        std::optional<TaskItem> task = findTask(gateway, id);
        QVERIFY(task);
        QCOMPARE(task->name, QStringLiteral("Write docs"));
        QVERIFY(!task->archived);
        QVERIFY(!task->latestStart);

        const BridgeReply renamed =
            await([&](Done done) { gateway.renameTask(id, "Write docs", "Write more docs", ago(60), done); });
        QVERIFY2(renamed.ok, qPrintable(renamed.error));
        QCOMPARE(findTask(gateway, id)->name, QStringLiteral("Write more docs"));

        const BridgeReply archived =
            await([&](Done done) { gateway.archiveTask(id, "Write more docs", ago(50), done); });
        QVERIFY2(archived.ok, qPrintable(archived.error));
        QVERIFY(findTask(gateway, id)->archived);

        const BridgeReply restored = await([&](Done done) { gateway.unarchiveTask(id, ago(40), done); });
        QVERIFY2(restored.ok, qPrintable(restored.error));
        QVERIFY(!findTask(gateway, id)->archived);
    }

    void aRenameWithAStaleNameIsAConflictThatNeedsARefresh() {
        RustTrackerGateway gateway;
        QVERIFY(await([&](Done done) { gateway.connectTo({false, QString()}, done); }).ok);
        const QString id = createTask(gateway, "Original name");
        const BridgeReply reply =
            await([&](Done done) { gateway.renameTask(id, "Not the current name", "New name", ago(60), done); });
        QVERIFY(!reply.ok);
        QCOMPARE(reply.kind, QStringLiteral("conflict"));
        QVERIFY(reply.requiresRefresh);
        QVERIFY(!reply.uncertain);
        QCOMPARE(findTask(gateway, id)->name, QStringLiteral("Original name"));
    }

    void anUnknownTaskIsRejected() {
        RustTrackerGateway gateway;
        QVERIFY(await([&](Done done) { gateway.connectTo({false, QString()}, done); }).ok);
        const BridgeReply reply = await([&](Done done) { gateway.history("not-a-task", QString(), done); });
        QVERIFY(!reply.ok);
        QCOMPARE(reply.error, QStringLiteral("Invalid task ID"));
    }

    void inactiveTasksArePreviewedBeforeTheyAreArchived() {
        RustTrackerGateway gateway;
        QVERIFY(await([&](Done done) { gateway.connectTo({false, QString()}, done); }).ok);
        const QString id = createTask(gateway, "Never worked on");
        // A task nobody tracked counts as inactive from its creation, 30 days before this check.
        const QDateTime asOf = ago(-30 * 86400);
        const BridgeReply preview = await([&](Done done) { gateway.previewInactiveTasks(7, asOf, done); });
        QVERIFY2(preview.ok, qPrintable(preview.error));
        QCOMPARE(preview.data.toObject().value("inactiveDays").toInt(), 7);
        bool listed = false;
        for (const QJsonValue &task : preview.data.toObject().value("tasks").toArray()) {
            listed = listed || task.toObject().value("id").toString() == id;
        }
        QVERIFY(listed);
        const BridgeReply archived = await([&](Done done) { gateway.archiveInactiveTasks(7, asOf, done); });
        QVERIFY2(archived.ok, qPrintable(archived.error));
        QVERIFY(findTask(gateway, id)->archived);
    }

    void archivingInactiveTasksNeedsAPreviewFirst() {
        RustTrackerGateway gateway;
        QVERIFY(await([&](Done done) { gateway.connectTo({false, QString()}, done); }).ok);
        const BridgeReply reply = await([&](Done done) { gateway.archiveInactiveTasks(7, ago(0), done); });
        QVERIFY(!reply.ok);
    }

    void aNegativeInactivityPeriodIsRejectedByTheCore() {
        RustTrackerGateway gateway;
        QVERIFY(await([&](Done done) { gateway.connectTo({false, QString()}, done); }).ok);
        const BridgeReply reply = await([&](Done done) { gateway.previewInactiveTasks(-5, ago(0), done); });
        QVERIFY(!reply.ok);
        QCOMPARE(reply.error, QStringLiteral("inactivity period must be at least one day"));
    }

    // Tracking, the report and history

    void trackingStartsStopsAndShowsUpInTheOverviewAndHistory() {
        RustTrackerGateway gateway;
        QVERIFY(await([&](Done done) { gateway.connectTo({false, QString()}, done); }).ok);
        const QString first = createTask(gateway, "Tracked task");
        const QString second = createTask(gateway, "Second tracked task");

        // Idle: no worklog is expected.
        const BridgeReply started = await([&](Done done) { gateway.startTracking(first, QString(), ago(3600), done); });
        QVERIFY2(started.ok, qPrintable(started.error));
        const QString firstWorklog = dataString(started, "active", "id");
        QVERIFY(!firstWorklog.isEmpty());

        const Overview running = Overview::fromJson(overview(gateway).data);
        QVERIFY(running.trackingKnown);
        QVERIFY(running.running);
        QCOMPARE(running.running->id, firstWorklog);
        QCOMPARE(running.running->taskId, first);
        QVERIFY(running.running->isRunning());
        QVERIFY(running.report);
        QVERIFY(running.report->microseconds.value(first) >= qint64(3599) * 1000000);

        // Starting another task while one runs stops the running one at the same moment.
        const BridgeReply switched =
            await([&](Done done) { gateway.startTracking(second, firstWorklog, ago(1800), done); });
        QVERIFY2(switched.ok, qPrintable(switched.error));
        // The switch reports the worklog it stopped.
        QCOMPARE(dataString(switched, "stopped", "id"), firstWorklog);
        const QString secondWorklog = dataString(switched, "active", "id");

        // A start that expects a worklog that is not running is a conflict.
        const BridgeReply stale = await([&](Done done) { gateway.startTracking(first, firstWorklog, ago(900), done); });
        QVERIFY(!stale.ok);

        const BridgeReply stopped = await([&](Done done) { gateway.stopTracking(secondWorklog, ago(600), done); });
        QVERIFY2(stopped.ok, qPrintable(stopped.error));
        QVERIFY(stopped.data.toObject().value("didStop").toBool());
        // Stopping again finds nothing to stop, which is not an error.
        const BridgeReply again = await([&](Done done) { gateway.stopTracking(secondWorklog, ago(500), done); });
        QVERIFY(again.ok);
        QVERIFY(!again.data.toObject().value("didStop").toBool());

        const Overview idle = Overview::fromJson(overview(gateway).data);
        QVERIFY(idle.trackingKnown);
        QVERIFY(!idle.running);
        QVERIFY(idle.report);
        QVERIFY(idle.report->microseconds.value(first) >= qint64(1799) * 1000000);
        QVERIFY(idle.report->microseconds.value(second) >= qint64(1199) * 1000000);

        const BridgeReply history = await([&](Done done) { gateway.history(first, QString(), done); });
        QVERIFY2(history.ok, qPrintable(history.error));
        const std::optional<HistoryPage> page = HistoryPage::fromJson(history.data);
        QVERIFY(page);
        QCOMPARE(page->worklogs.size(), 1);
        QCOMPARE(page->worklogs.first().id, firstWorklog);
        QVERIFY(!page->worklogs.first().isRunning());
        QVERIFY(page->nextCursor.isEmpty());
        QVERIFY(!page->reset);
        const std::optional<TaskItem> tracked = findTask(gateway, first);
        QVERIFY(tracked->latestStart);
    }

    // Worklog changes

    void aWorklogCanBeCorrectedMovedAndDeleted() {
        RustTrackerGateway gateway;
        QVERIFY(await([&](Done done) { gateway.connectTo({false, QString()}, done); }).ok);
        const QString source = createTask(gateway, "Worklog source");
        const QString target = createTask(gateway, "Worklog target");
        const BridgeReply started =
            await([&](Done done) { gateway.startTracking(source, QString(), ago(7000), done); });
        QVERIFY(started.ok);
        const QString worklogId = dataString(started, "active", "id");
        QVERIFY(await([&](Done done) { gateway.stopTracking(worklogId, ago(6000), done); }).ok);

        auto readLog = [&](const QString &taskId) {
            const std::optional<HistoryPage> page =
                HistoryPage::fromJson(await([&](Done done) { gateway.history(taskId, QString(), done); }).data);
            return page && !page->worklogs.isEmpty() ? std::optional<WorklogItem>(page->worklogs.first())
                                                     : std::nullopt;
        };

        std::optional<WorklogItem> log = readLog(source);
        QVERIFY(log);
        const QString newStart = Timestamps::format(ago(5000));
        const QString newEnd = Timestamps::format(ago(4000));
        // A correction with outdated expectations is refused.
        const BridgeReply refused = await(
            [&](Done done) { gateway.correctWorklog(worklogId, newStart, newEnd, newStart, newEnd, ago(10), done); });
        QVERIFY(!refused.ok);
        const BridgeReply corrected = await([&](Done done) {
            gateway.correctWorklog(worklogId, log->startText, log->endText, newStart, newEnd, ago(10), done);
        });
        QVERIFY2(corrected.ok, qPrintable(corrected.error));
        log = readLog(source);
        QVERIFY(log);
        QCOMPARE(Timestamps::format(log->start), newStart);
        QCOMPARE(Timestamps::format(*log->end), newEnd);

        const BridgeReply candidates = await([&](Done done) { gateway.moveCandidates(source, "target", done); });
        QVERIFY2(candidates.ok, qPrintable(candidates.error));
        QCOMPARE(candidates.data.toArray().size(), 1);
        QCOMPARE(candidates.data.toArray().first().toObject().value("id").toString(), target);

        const BridgeReply moved = await(
            [&](Done done) { gateway.moveWorklog(worklogId, source, log->startText, log->endText, target, done); });
        QVERIFY2(moved.ok, qPrintable(moved.error));
        QVERIFY(!readLog(source));
        log = readLog(target);
        QVERIFY(log);
        QCOMPARE(log->id, worklogId);

        // Deleting with the wrong task is refused; with the right expectations it works.
        const BridgeReply wrongTask =
            await([&](Done done) { gateway.deleteWorklog(worklogId, source, log->startText, log->endText, done); });
        QVERIFY(!wrongTask.ok);
        const BridgeReply deleted =
            await([&](Done done) { gateway.deleteWorklog(worklogId, target, log->startText, log->endText, done); });
        QVERIFY2(deleted.ok, qPrintable(deleted.error));
        QVERIFY(!readLog(target));
    }

    void aRunningWorklogCanBeCorrectedWithoutAnEnd() {
        RustTrackerGateway gateway;
        QVERIFY(await([&](Done done) { gateway.connectTo({false, QString()}, done); }).ok);
        const QString task = createTask(gateway, "Running correction");
        const BridgeReply started = await([&](Done done) { gateway.startTracking(task, QString(), ago(3000), done); });
        const QString worklogId = dataString(started, "active", "id");
        const QString startText = dataString(started, "active", "start");
        const QString newStart = Timestamps::format(ago(2500));
        // The empty end texts describe a running worklog.
        const BridgeReply corrected = await([&](Done done) {
            gateway.correctWorklog(worklogId, startText, QString(), newStart, QString(), ago(10), done);
        });
        QVERIFY2(corrected.ok, qPrintable(corrected.error));
        const Overview current = Overview::fromJson(overview(gateway).data);
        QVERIFY(current.running);
        QCOMPARE(Timestamps::format(current.running->start), newStart);
        QVERIFY(await([&](Done done) { gateway.stopTracking(worklogId, ago(5), done); }).ok);
    }

    // Connections

    void testingAnUnreachableServerFailsAsUnavailable() {
        RustTrackerGateway gateway;
        const BridgeReply reply = await([&](Done done) { gateway.testConnection({true, "http://127.0.0.1:1"}, done); });
        QVERIFY(!reply.ok);
        QCOMPARE(reply.kind, QStringLiteral("unavailable"));
        QVERIFY(reply.error.contains("unavailable"));
    }

    void theServerUrlIsTrimmedBeforeItIsUsed() {
        RustTrackerGateway gateway;
        const BridgeReply reply =
            await([&](Done done) { gateway.testConnection({true, "  http://127.0.0.1:1\n"}, done); });
        QVERIFY(!reply.ok);
        QCOMPARE(reply.kind, QStringLiteral("unavailable"));
    }

    void testingAnInvalidServerUrlExplainsTheProblem() {
        RustTrackerGateway gateway;
        for (const QString &url : {QStringLiteral("not a url"), QString(), QStringLiteral("   ")}) {
            const BridgeReply reply = await([&](Done done) { gateway.testConnection({true, url}, done); });
            QVERIFY(!reply.ok);
            QVERIFY2(reply.error.contains("invalid tracker endpoint"), qPrintable(reply.error));
        }
    }

    void aFailingServerKeepsThePreviousConnection() {
        RustTrackerGateway gateway;
        QVERIFY(await([&](Done done) { gateway.connectTo({false, QString()}, done); }).ok);
        const QString id = createTask(gateway, "Survives a failed switch");
        const BridgeReply failed = await([&](Done done) { gateway.connectTo({true, "http://127.0.0.1:1"}, done); });
        QVERIFY(!failed.ok);
        QCOMPARE(failed.kind, QStringLiteral("unavailable"));
        // The local database is still the open connection.
        const std::optional<TaskItem> task = findTask(gateway, id);
        QVERIFY(task);
        QCOMPARE(task->name, QStringLiteral("Survives a failed switch"));
        QVERIFY(await([&](Done done) { gateway.createTask("Still writable", ago(30), done); }).ok);
    }

    void aFailedFirstConnectionLeavesTheGatewayDisconnected() {
        RustTrackerGateway gateway;
        QVERIFY(!await([&](Done done) { gateway.connectTo({true, "http://127.0.0.1:1"}, done); }).ok);
        QCOMPARE(overview(gateway).error, QStringLiteral("Database is not open"));
    }
};

REGISTER_TEST(RustTrackerGatewayTests)
#include "RustTrackerGatewayTests.moc"

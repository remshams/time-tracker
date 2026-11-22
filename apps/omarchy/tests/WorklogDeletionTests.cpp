#include <QSignalSpy>
#include <QTest>
#include <cstdlib>
#include <ctime>

#include "Features/WorklogDeletion/WorklogDeletionStore.h"
#include "TestRegistry.h"
#include "WorklogFixture.h"

namespace {

const QString startText = QStringLiteral("2026-11-20T08:00:30.250Z"); // 09:00 in Berlin
const QString endText = QStringLiteral("2026-11-20T09:30:45.500Z");   // 10:30 in Berlin

struct DeletionFixture : WorklogFixture {
    WorklogDeletionStore store{&gateway, &history, &clock};

    DeletionFixture() {
        showHistory({WorklogFixture::worklog("w1", startText, endText),
                     WorklogFixture::worklog("running", "2026-11-20T10:00:00.000Z"),
                     WorklogFixture::worklog("yesterday", "2026-11-19T08:00:00.000Z", "2026-11-19T09:00:00.000Z"),
                     WorklogFixture::worklog("older", "2026-11-02T08:00:00.000Z", "2026-11-02T09:00:00.000Z")});
    }
};

} // namespace

class WorklogDeletionTests : public QObject {
    Q_OBJECT

    QByteArray previousZone_;
    bool hadZone_ = false;

private slots:
    // The labels are local times, so they need one zone.
    void initTestCase() {
        hadZone_ = qEnvironmentVariableIsSet("TZ");
        previousZone_ = qgetenv("TZ");
        qputenv("TZ", "Europe/Berlin");
        tzset();
    }

    void cleanupTestCase() {
        if (hadZone_) {
            qputenv("TZ", previousZone_);
        } else {
            qunsetenv("TZ");
        }
        tzset();
    }

    void beginShowsWhatWillBeRemoved() {
        DeletionFixture f;
        QSignalSpy begun(&f.store, &WorklogDeletionStore::begun);
        QSignalSpy canDeleteChanged(&f.store, &WorklogDeletionStore::canDeleteChanged);

        QVERIFY(f.store.begin("w1"));

        QCOMPARE(begun.count(), 1);
        QCOMPARE(f.store.taskId(), QStringLiteral("task-a"));
        QCOMPARE(f.store.dayLabel(), QStringLiteral("Today"));
        QCOMPARE(f.store.startLabel(), QStringLiteral("09:00"));
        QCOMPARE(f.store.endLabel(), QStringLiteral("10:30"));
        QCOMPARE(f.store.durationSeconds(), qint64(5415));
        QVERIFY(f.store.canDelete());
        QCOMPARE(canDeleteChanged.count(), 1);
        QVERIFY(!f.store.pending());
        QVERIFY(f.store.error().isEmpty());
    }

    void theDayLabelFollowsTheWorklogsStartDay() {
        DeletionFixture f;

        QVERIFY(f.store.begin("yesterday"));
        QCOMPARE(f.store.dayLabel(), QStringLiteral("Yesterday"));

        QVERIFY(f.store.begin("older"));
        QVERIFY(f.store.dayLabel() != QStringLiteral("Today"));
        QVERIFY(f.store.dayLabel() != QStringLiteral("Yesterday"));
        QVERIFY(!f.store.dayLabel().isEmpty());
    }

    void beginRefusesAWorklogThatIsNotShown() {
        DeletionFixture f;

        QVERIFY(!f.store.begin("missing"));

        QVERIFY(f.store.taskId().isEmpty());
        QVERIFY(!f.store.canDelete());
        f.store.confirm();
        QCOMPARE(f.gateway.count("deleteWorklog"), 0);
    }

    void aRunningWorklogCannotBeDeleted() {
        DeletionFixture f;

        QVERIFY(f.store.begin("running"));

        QVERIFY(!f.store.canDelete());
        QVERIFY(f.store.error().contains(QStringLiteral("running")));
        QVERIFY(f.store.error().contains(QStringLiteral("Stop the timer")));
        QCOMPARE(f.store.endLabel(), QStringLiteral("now"));
        QCOMPARE(f.store.durationSeconds(), qint64(0));
        f.store.confirm();
        QCOMPARE(f.gateway.count("deleteWorklog"), 0);
    }

    void confirmSendsTheOriginalTaskAndTimestampsAndSucceeds() {
        DeletionFixture f;
        f.store.begin("w1");
        QSignalSpy deleted(&f.store, &WorklogDeletionStore::deleted);
        QSignalSpy refresh(&f.store, &WorklogDeletionStore::refreshRequested);
        QSignalSpy pendingChanged(&f.store, &WorklogDeletionStore::pendingChanged);
        QSignalSpy canDeleteChanged(&f.store, &WorklogDeletionStore::canDeleteChanged);

        f.store.confirm();

        QCOMPARE(f.gateway.lastArgs("deleteWorklog"), (QStringList{"w1", "task-a", startText, endText}));
        QVERIFY(f.store.pending());
        QVERIFY(!f.store.canDelete());
        QCOMPARE(pendingChanged.count(), 1);
        QCOMPARE(canDeleteChanged.count(), 1);
        f.store.confirm(); // A second confirmation while one is open sends nothing.
        QCOMPARE(f.gateway.count("deleteWorklog"), 1);
        QCOMPARE(deleted.count(), 0);
        QCOMPARE(refresh.count(), 0);

        f.gateway.respond("deleteWorklog", writeSucceeded());

        QVERIFY(!f.store.pending());
        QCOMPARE(deleted.count(), 1);
        QCOMPARE(refresh.count(), 1);
        QVERIFY(f.store.error().isEmpty());
    }

    void aRefusedDeletionShowsTheServersMessageAndMayBeRepeated() {
        DeletionFixture f;
        f.store.begin("w1");
        QSignalSpy deleted(&f.store, &WorklogDeletionStore::deleted);
        QSignalSpy refresh(&f.store, &WorklogDeletionStore::refreshRequested);

        f.store.confirm();
        f.gateway.respond("deleteWorklog", BridgeReply::failure("The worklog changed"));

        QCOMPARE(f.store.error(), QStringLiteral("The worklog changed"));
        QVERIFY(!f.store.pending());
        QVERIFY(f.store.canDelete());
        QCOMPARE(deleted.count(), 0);
        QCOMPARE(refresh.count(), 1);

        f.store.confirm();
        QVERIFY(f.store.error().isEmpty());
        QCOMPARE(f.gateway.count("deleteWorklog"), 2);
    }

    void anUncertainDeletionSaysItCouldNotBeConfirmed() {
        DeletionFixture f;
        f.store.begin("w1");
        QSignalSpy refresh(&f.store, &WorklogDeletionStore::refreshRequested);

        f.store.confirm();
        f.gateway.respond("deleteWorklog", writeUncertain());

        QVERIFY(f.store.error().contains(QStringLiteral("could not be confirmed")));
        QVERIFY(f.store.error().contains(QStringLiteral("Check the reloaded history")));
        QVERIFY(!f.store.error().contains(QStringLiteral("timed out")));
        QCOMPARE(refresh.count(), 1);
    }

    void cancellingBlocksTheDeletion() {
        DeletionFixture f;
        f.store.begin("w1");

        f.store.cancel();

        QVERIFY(!f.store.canDelete());
        f.store.confirm();
        QCOMPARE(f.gateway.count("deleteWorklog"), 0);
    }

    void cancellingAnOpenDeletionOnlyRefreshes() {
        DeletionFixture f;
        f.store.begin("w1");
        QSignalSpy deleted(&f.store, &WorklogDeletionStore::deleted);
        QSignalSpy refresh(&f.store, &WorklogDeletionStore::refreshRequested);
        f.store.confirm();

        f.store.cancel();
        f.gateway.respond("deleteWorklog", writeSucceeded());

        QCOMPARE(deleted.count(), 0);
        QCOMPARE(refresh.count(), 1);
    }

    void aNewDialogIgnoresTheAnswerOfAnOlderDeletion() {
        DeletionFixture f;
        f.store.begin("w1");
        f.store.confirm();
        QVERIFY(f.store.pending());

        f.store.begin("yesterday");

        QVERIFY(!f.store.pending());
        QVERIFY(f.store.canDelete());
        QSignalSpy deleted(&f.store, &WorklogDeletionStore::deleted);
        QSignalSpy refresh(&f.store, &WorklogDeletionStore::refreshRequested);
        f.gateway.respond("deleteWorklog", BridgeReply::failure("Conflict"));
        QVERIFY(f.store.error().isEmpty());
        QCOMPARE(deleted.count(), 0);
        QCOMPARE(refresh.count(), 1);
    }

    void aNewDialogClearsTheOldError() {
        DeletionFixture f;
        f.store.begin("w1");
        f.store.confirm();
        f.gateway.respond("deleteWorklog", BridgeReply::failure("Conflict"));
        QVERIFY(!f.store.error().isEmpty());

        f.store.begin("w1");

        QVERIFY(f.store.error().isEmpty());
    }

    void anAnswerAfterTheStoreIsGoneIsDropped() {
        DeletionFixture f;
        auto *store = new WorklogDeletionStore(&f.gateway, &f.history, &f.clock);
        store->begin("w1");
        store->confirm();

        delete store;

        QVERIFY(f.gateway.respond("deleteWorklog", writeSucceeded()));
    }
};

REGISTER_TEST(WorklogDeletionTests)
#include "WorklogDeletionTests.moc"

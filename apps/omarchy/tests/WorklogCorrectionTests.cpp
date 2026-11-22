#include <QSignalSpy>
#include <QTest>
#include <cstdlib>
#include <ctime>

#include "Features/WorklogCorrection/WorklogCorrectionStore.h"
#include "TestRegistry.h"
#include "WorklogFixture.h"

namespace {

const QString startText = QStringLiteral("2026-11-20T08:00:30.250Z"); // 09:00:30 in Berlin
const QString endText = QStringLiteral("2026-11-20T09:30:45.500Z");   // 10:30:45 in Berlin

} // namespace

class WorklogCorrectionTests : public QObject {
    Q_OBJECT

    QByteArray previousZone_;
    bool hadZone_ = false;

    static BridgeReply conflict() { return BridgeReply::failure(QStringLiteral("The worklog changed")); }

private slots:
    // The expected results depend on one zone, which Qt picks up from TZ at each conversion.
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

    void beginShowsLocalFieldsAndTheZone() {
        WorklogFixture fixture;
        fixture.showHistory({WorklogFixture::worklog("w1", startText, endText)});
        WorklogCorrectionStore store(&fixture.gateway, &fixture.history, &fixture.clock);
        QSignalSpy begun(&store, &WorklogCorrectionStore::begun);

        QVERIFY(store.begin("w1"));

        QCOMPARE(begun.count(), 1);
        QCOMPARE(store.taskId(), QStringLiteral("task-a"));
        QVERIFY(!store.running());
        QCOMPARE(store.startDate(), QStringLiteral("2026-11-20"));
        QCOMPARE(store.startTime(), QStringLiteral("09:00"));
        QCOMPARE(store.endDate(), QStringLiteral("2026-11-20"));
        QCOMPARE(store.endTime(), QStringLiteral("10:30"));
        QCOMPARE(store.timeZone(), QStringLiteral("CET (UTC+0100)"));
        QCOMPARE(store.durationSeconds(), qint64(5415));
        QVERIFY(store.validationMessage().isEmpty());
        QVERIFY(!store.changed());
        QVERIFY(!store.canSave());
        QVERIFY(!store.pending());
        QVERIFY(store.error().isEmpty());
    }

    void beginRefusesAWorklogThatIsNotShown() {
        WorklogFixture fixture;
        fixture.showHistory({});
        WorklogCorrectionStore store(&fixture.gateway, &fixture.history, &fixture.clock);

        QVERIFY(!store.begin("missing"));

        QVERIFY(store.taskId().isEmpty());
        QCOMPARE(store.durationSeconds(), qint64(0));
        QVERIFY(!store.canSave());
        QVERIFY(store.validationMessage().isEmpty());
        store.save();
        QCOMPARE(fixture.gateway.count("correctWorklog"), 0);
    }

    void aChangedEndKeepsTheOriginalStartText() {
        WorklogFixture fixture;
        fixture.showHistory({WorklogFixture::worklog("w1", startText, endText)});
        WorklogCorrectionStore store(&fixture.gateway, &fixture.history, &fixture.clock);
        store.begin("w1");
        QSignalSpy canSaveChanged(&store, &WorklogCorrectionStore::canSaveChanged);

        store.setEndTime("11:00");

        QVERIFY(store.changed());
        QVERIFY(store.canSave());
        QCOMPARE(canSaveChanged.count(), 1);
        QCOMPARE(store.durationSeconds(), qint64(7169)); // 08:00:30.250 to 10:00:00 UTC, truncated to whole seconds
        store.save();
        QCOMPARE(
            fixture.gateway.lastArgs("correctWorklog"),
            (QStringList{"w1", startText, endText, startText, "2026-11-20T10:00:00.000Z", "2026-11-20T12:00:00.000Z"}));
    }

    void aChangedStartKeepsTheOriginalEndTextAndDropsSeconds() {
        WorklogFixture fixture;
        fixture.showHistory({WorklogFixture::worklog("w1", startText, endText)});
        WorklogCorrectionStore store(&fixture.gateway, &fixture.history, &fixture.clock);
        store.begin("w1");

        store.setStartTime("09:15");
        store.save();

        QCOMPARE(fixture.gateway.lastArgs("correctWorklog").mid(0, 5),
                 (QStringList{"w1", startText, endText, "2026-11-20T08:15:00.000Z", endText}));
    }

    void bothFieldsChangeWithADateAndATime() {
        WorklogFixture fixture;
        fixture.showHistory({WorklogFixture::worklog("w1", startText, endText)});
        WorklogCorrectionStore store(&fixture.gateway, &fixture.history, &fixture.clock);
        store.begin("w1");

        store.setStartDate("2026-11-19");
        store.setEndDate("2026-11-20");
        store.setEndTime("9:45");
        store.save();

        QCOMPARE(fixture.gateway.lastArgs("correctWorklog").mid(3, 2),
                 (QStringList{"2026-11-19T08:00:00.000Z", "2026-11-20T08:45:00.000Z"}));
    }

    void typingTheOriginalTextBackIsNoChange() {
        WorklogFixture fixture;
        fixture.showHistory({WorklogFixture::worklog("w1", startText, endText)});
        WorklogCorrectionStore store(&fixture.gateway, &fixture.history, &fixture.clock);
        store.begin("w1");

        store.setStartTime("09:15");
        QVERIFY(store.canSave());
        store.setStartTime("09:00");

        QVERIFY(!store.changed());
        QVERIFY(!store.canSave());
        store.save();
        QCOMPARE(fixture.gateway.count("correctWorklog"), 0);
    }

    void aChangeOfTheEndAloneCountsEvenWithTheStartUntouched() {
        WorklogFixture fixture;
        fixture.showHistory({WorklogFixture::worklog("w1", startText, endText)});
        WorklogCorrectionStore store(&fixture.gateway, &fixture.history, &fixture.clock);
        store.begin("w1");

        store.setEndDate("2026-11-21");

        QVERIFY(store.changed());
        store.setEndDate("2026-11-20");
        QVERIFY(!store.changed());
    }

    void whitespaceAroundTheFieldsIsIgnored() {
        WorklogFixture fixture;
        fixture.showHistory({WorklogFixture::worklog("w1", startText, endText)});
        WorklogCorrectionStore store(&fixture.gateway, &fixture.history, &fixture.clock);
        store.begin("w1");

        store.setStartDate(" 2026-11-20 ");
        store.setStartTime(" 09:00 ");
        QVERIFY(!store.changed());
        store.setStartTime(" 09:20 ");
        store.save();

        QCOMPARE(fixture.gateway.lastArgs("correctWorklog").at(3), QStringLiteral("2026-11-20T08:20:00.000Z"));
    }

    void unreadableFieldsBlockSavingAndExplain() {
        WorklogFixture fixture;
        fixture.showHistory({WorklogFixture::worklog("w1", startText, endText)});
        WorklogCorrectionStore store(&fixture.gateway, &fixture.history, &fixture.clock);
        store.begin("w1");

        const QList<QPair<QString, QString>> badInputs = {
            {"2026-11-20", "9:61"},  {"2026-11-20", "24:00"}, {"2026-11-20", "0930"}, {"2026-11-20", ""},
            {"2026-13-01", "09:00"}, {"2026-02-30", "09:00"}, {"2026-1-5", "09:00"},  {"20.11.2026", "09:00"},
        };
        for (const auto &[date, time] : badInputs) {
            store.setStartDate(date);
            store.setStartTime(time);
            QVERIFY2(!store.validationMessage().isEmpty(), qPrintable(date + " " + time));
            QVERIFY2(!store.canSave(), qPrintable(date + " " + time));
            QCOMPARE(store.durationSeconds(), qint64(0));
            store.save();
        }
        QCOMPARE(fixture.gateway.count("correctWorklog"), 0);

        store.setStartDate("2026-11-20");
        store.setStartTime("09:00");
        store.setEndTime("x");
        QVERIFY(!store.validationMessage().isEmpty());
        QVERIFY(!store.canSave());
    }

    void anEndBeforeTheStartShowsANegativeDurationAndTheCoreDecides() {
        WorklogFixture fixture;
        fixture.showHistory({WorklogFixture::worklog("w1", startText, endText)});
        WorklogCorrectionStore store(&fixture.gateway, &fixture.history, &fixture.clock);
        store.begin("w1");

        store.setEndTime("08:00");

        QCOMPARE(store.durationSeconds(), qint64(-3630)); // 08:00:30.250 to 07:00:00 UTC, truncated to whole seconds
        QVERIFY(store.validationMessage().isEmpty());
        QVERIFY(store.canSave());
    }

    void aRunningWorklogEditsOnlyItsStartAndKeepsRunning() {
        WorklogFixture fixture;
        fixture.showHistory({WorklogFixture::worklog("w1", startText)});
        WorklogCorrectionStore store(&fixture.gateway, &fixture.history, &fixture.clock);
        store.begin("w1");

        QVERIFY(store.running());
        QVERIFY(store.endDate().isEmpty());
        QVERIFY(store.endTime().isEmpty());
        QCOMPARE(store.durationSeconds(), qint64(14369)); // 08:00:30.250 to 12:00:00 UTC
        // The end fields do not exist for a running worklog, so typing there changes nothing.
        store.setEndTime("11:00");
        QVERIFY(!store.changed());

        store.setStartTime("09:30");
        QCOMPARE(store.durationSeconds(), qint64(12600)); // 08:30:00 to 12:00:00 UTC
        store.save();

        QCOMPARE(fixture.gateway.lastArgs("correctWorklog"),
                 (QStringList{"w1", startText, QString(), "2026-11-20T08:30:00.000Z", QString(),
                              "2026-11-20T12:00:00.000Z"}));
    }

    void aRunningPreviewGrowsWithTheClockAndACompletedOneDoesNot() {
        WorklogFixture fixture;
        fixture.showHistory(
            {WorklogFixture::worklog("w1", startText), WorklogFixture::worklog("w2", startText, endText)});
        WorklogCorrectionStore store(&fixture.gateway, &fixture.history, &fixture.clock);
        QSignalSpy preview(&store, &WorklogCorrectionStore::previewChanged);

        store.begin("w1");
        preview.clear();
        fixture.now = fixture.now.addSecs(60);
        fixture.clock.tick();
        QCOMPARE(preview.count(), 1);
        QCOMPARE(store.durationSeconds(), qint64(14429));

        store.begin("w2");
        preview.clear();
        fixture.clock.tick();
        QCOMPARE(preview.count(), 0);

        store.begin("w1");
        store.cancel();
        preview.clear();
        fixture.clock.tick();
        QCOMPARE(preview.count(), 0);
    }

    void aSavePendsUntilTheAnswerAndSucceeds() {
        WorklogFixture fixture;
        fixture.showHistory({WorklogFixture::worklog("w1", startText, endText)});
        WorklogCorrectionStore store(&fixture.gateway, &fixture.history, &fixture.clock);
        store.begin("w1");
        store.setEndTime("11:00");
        QSignalSpy saved(&store, &WorklogCorrectionStore::saved);
        QSignalSpy refresh(&store, &WorklogCorrectionStore::refreshRequested);
        QSignalSpy pendingChanged(&store, &WorklogCorrectionStore::pendingChanged);
        QSignalSpy canSaveChanged(&store, &WorklogCorrectionStore::canSaveChanged);

        store.save();

        QVERIFY(store.pending());
        QVERIFY(!store.canSave());
        QCOMPARE(pendingChanged.count(), 1);
        QCOMPARE(canSaveChanged.count(), 1);
        store.save(); // A second save while one is open sends nothing.
        QCOMPARE(fixture.gateway.count("correctWorklog"), 1);
        QCOMPARE(saved.count(), 0);
        QCOMPARE(refresh.count(), 0);

        QVERIFY(fixture.gateway.respond("correctWorklog", writeSucceeded()));

        QVERIFY(!store.pending());
        QCOMPARE(saved.count(), 1);
        QCOMPARE(refresh.count(), 1);
        QVERIFY(store.error().isEmpty());
    }

    void aRefusedSaveKeepsTheDraftAndShowsTheServersMessage() {
        WorklogFixture fixture;
        fixture.showHistory({WorklogFixture::worklog("w1", startText, endText)});
        WorklogCorrectionStore store(&fixture.gateway, &fixture.history, &fixture.clock);
        store.begin("w1");
        store.setEndTime("11:00");
        QSignalSpy saved(&store, &WorklogCorrectionStore::saved);
        QSignalSpy refresh(&store, &WorklogCorrectionStore::refreshRequested);
        QSignalSpy errorChanged(&store, &WorklogCorrectionStore::errorChanged);

        store.save();
        fixture.gateway.respond("correctWorklog", conflict());

        QCOMPARE(store.error(), QStringLiteral("The worklog changed"));
        QCOMPARE(errorChanged.count(), 1);
        QVERIFY(!store.pending());
        QCOMPARE(store.endTime(), QStringLiteral("11:00"));
        QVERIFY(store.canSave());
        QCOMPARE(saved.count(), 0);
        QCOMPARE(refresh.count(), 1);

        // A new attempt clears the old message and sends the same expected timestamps again.
        store.save();
        QVERIFY(store.error().isEmpty());
        QCOMPARE(fixture.gateway.count("correctWorklog"), 2);
        QCOMPARE(fixture.gateway.lastArgs("correctWorklog").mid(0, 3), (QStringList{"w1", startText, endText}));
    }

    void anUncertainSaveSaysTheChangeCouldNotBeConfirmed() {
        WorklogFixture fixture;
        fixture.showHistory({WorklogFixture::worklog("w1", startText, endText)});
        WorklogCorrectionStore store(&fixture.gateway, &fixture.history, &fixture.clock);
        store.begin("w1");
        store.setEndTime("11:00");
        QSignalSpy refresh(&store, &WorklogCorrectionStore::refreshRequested);

        store.save();
        fixture.gateway.respond("correctWorklog", writeUncertain());

        QVERIFY(store.error().contains(QStringLiteral("could not be confirmed")));
        QVERIFY(store.error().contains(QStringLiteral("Check the reloaded history")));
        QVERIFY(!store.error().contains(QStringLiteral("timed out")));
        QCOMPARE(refresh.count(), 1);
    }

    void cancellingAnOpenSaveOnlyRefreshes() {
        WorklogFixture fixture;
        fixture.showHistory({WorklogFixture::worklog("w1", startText, endText)});
        WorklogCorrectionStore store(&fixture.gateway, &fixture.history, &fixture.clock);
        store.begin("w1");
        store.setEndTime("11:00");
        QSignalSpy saved(&store, &WorklogCorrectionStore::saved);
        QSignalSpy refresh(&store, &WorklogCorrectionStore::refreshRequested);

        store.save();
        store.cancel();
        QVERIFY(!store.canSave());
        fixture.gateway.respond("correctWorklog", writeSucceeded());

        QCOMPARE(saved.count(), 0);
        QCOMPARE(refresh.count(), 1);
    }

    void aNewDraftIgnoresTheAnswerOfAnOlderSave() {
        WorklogFixture fixture;
        fixture.showHistory(
            {WorklogFixture::worklog("w1", startText, endText), WorklogFixture::worklog("w2", startText, endText)});
        WorklogCorrectionStore store(&fixture.gateway, &fixture.history, &fixture.clock);
        store.begin("w1");
        store.setEndTime("11:00");
        store.save();
        QVERIFY(store.pending());

        store.begin("w2");

        QVERIFY(!store.pending());
        QSignalSpy saved(&store, &WorklogCorrectionStore::saved);
        QSignalSpy refresh(&store, &WorklogCorrectionStore::refreshRequested);
        fixture.gateway.respond("correctWorklog", conflict());
        QVERIFY(store.error().isEmpty());
        QCOMPARE(saved.count(), 0);
        QCOMPARE(refresh.count(), 1);
    }

    void aNewDraftClearsTheOldError() {
        WorklogFixture fixture;
        fixture.showHistory({WorklogFixture::worklog("w1", startText, endText)});
        WorklogCorrectionStore store(&fixture.gateway, &fixture.history, &fixture.clock);
        store.begin("w1");
        store.setEndTime("11:00");
        store.save();
        fixture.gateway.respond("correctWorklog", conflict());
        QVERIFY(!store.error().isEmpty());

        store.begin("w1");

        QVERIFY(store.error().isEmpty());
        QCOMPARE(store.endTime(), QStringLiteral("10:30"));
    }

    void anAnswerAfterTheStoreIsGoneIsDropped() {
        WorklogFixture fixture;
        fixture.showHistory({WorklogFixture::worklog("w1", startText, endText)});
        auto *store = new WorklogCorrectionStore(&fixture.gateway, &fixture.history, &fixture.clock);
        store->begin("w1");
        store->setEndTime("11:00");
        store->save();

        delete store;

        QVERIFY(fixture.gateway.respond("correctWorklog", writeSucceeded()));
    }

    void settingAFieldToItsCurrentTextEmitsNothing() {
        WorklogFixture fixture;
        fixture.showHistory({WorklogFixture::worklog("w1", startText, endText)});
        WorklogCorrectionStore store(&fixture.gateway, &fixture.history, &fixture.clock);
        store.begin("w1");
        QSignalSpy fields(&store, &WorklogCorrectionStore::fieldsChanged);

        store.setStartDate("2026-11-20");
        store.setStartTime("09:00");
        store.setEndDate("2026-11-20");
        store.setEndTime("10:30");
        QCOMPARE(fields.count(), 0);

        store.setEndTime("10:31");
        QCOMPARE(fields.count(), 1);
    }
};

REGISTER_TEST(WorklogCorrectionTests)
#include "WorklogCorrectionTests.moc"

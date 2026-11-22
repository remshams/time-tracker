#include <QJsonArray>
#include <QJsonObject>
#include <QSignalSpy>
#include <QTest>

#include "FakeTrackerGateway.h"
#include "Features/BulkTaskArchiving/BulkTaskArchivingStore.h"
#include "Shared/DisplayClock.h"
#include "Shared/TaskColor.h"
#include "TestRegistry.h"

namespace {

const QString kFirstStamp = QStringLiteral("2026-05-04T10:00:00.000Z");
const QString kLaterStamp = QStringLiteral("2026-05-04T10:05:00.000Z");

QJsonObject candidate(const QString &id, const QString &name) {
    return QJsonObject{{QStringLiteral("id"), id}, {QStringLiteral("name"), name}, {QStringLiteral("archived"), false}};
}

BridgeReply preview(const QJsonArray &tasks) {
    return BridgeReply::success(QJsonObject{{QStringLiteral("tasks"), tasks}});
}

BridgeReply twoCandidates() {
    return preview({candidate(QStringLiteral("a"), QStringLiteral("Alpha")),
                    candidate(QStringLiteral("b"), QStringLiteral("Beta"))});
}

BridgeReply archived(int count) {
    return BridgeReply::success(QJsonObject{{QStringLiteral("archivedCount"), count}});
}

QStringList taskIds(const QVariantList &tasks) {
    QStringList ids;
    for (const QVariant &task : tasks) {
        ids.append(task.toMap().value(QStringLiteral("id")).toString());
    }
    return ids;
}

} // namespace

class BulkTaskArchivingTests : public QObject {
    Q_OBJECT
private slots:
    void init() {
        now = QDateTime::fromString(kFirstStamp, Qt::ISODateWithMs);
        gateway = std::make_unique<FakeTrackerGateway>();
        clock = std::make_unique<DisplayClock>([this] { return now; }, false);
        store = std::make_unique<BulkTaskArchivingStore>(gateway.get(), clock.get());
    }

    void parsesPositiveWholeNumbersOnly_data() {
        QTest::addColumn<QString>("text");
        QTest::addColumn<int>("days");
        QTest::newRow("plain") << "14" << 14;
        QTest::newRow("padded") << " 7 " << 7;
        QTest::newRow("leading zeros") << "007" << 7;
        QTest::newRow("zero") << "0" << 0;
        QTest::newRow("empty") << "" << 0;
        QTest::newRow("blank") << "  " << 0;
        QTest::newRow("negative") << "-3" << 0;
        QTest::newRow("plus sign") << "+3" << 0;
        QTest::newRow("fraction") << "1.5" << 0;
        QTest::newRow("letters") << "abc" << 0;
        QTest::newRow("mixed") << "3d" << 0;
        QTest::newRow("overflow") << "99999999999" << 0;
        QTest::newRow("other digits") << QString::fromUtf8("\xD9\xA3") << 0;
        QTest::newRow("below the digits") << "/" << 0;
        QTest::newRow("above the digits") << ":" << 0;
    }
    void parsesPositiveWholeNumbersOnly() {
        QFETCH(QString, text);
        QFETCH(int, days);
        QCOMPARE(BulkTaskArchivingStore::parseDays(text), days);
    }

    void beginLoadsThePreviewForTheDefaultPeriod() {
        QSignalSpy daysText(store.get(), &BulkTaskArchivingStore::daysTextChanged);
        store->begin();
        QCOMPARE(store->daysText(), QStringLiteral("14"));
        QCOMPARE(daysText.count(), 1);
        QCOMPARE(gateway->lastArgs("previewInactiveTasks"), (QStringList{QStringLiteral("14"), kFirstStamp}));
        QVERIFY(store->loading());
        QVERIFY(!store->previewLoaded());
        QVERIFY(!store->canSubmit());
        QCOMPARE(store->archivedCount(), -1);
    }

    void aLoadedPreviewListsTheTasksWithTheirColors() {
        store->begin();
        gateway->respond("previewInactiveTasks", twoCandidates());
        QVERIFY(!store->loading());
        QVERIFY(store->previewLoaded());
        QVERIFY(store->canSubmit());
        QCOMPARE(store->tasks().size(), 2);
        const QVariantMap first = store->tasks().at(0).toMap();
        QCOMPARE(first.value(QStringLiteral("id")).toString(), QStringLiteral("a"));
        QCOMPARE(first.value(QStringLiteral("name")).toString(), QStringLiteral("Alpha"));
        QCOMPARE(first.value(QStringLiteral("colorName")).toString(), TaskColor::nameFor(QStringLiteral("a")));
        QVERIFY(store->error().isEmpty());
    }

    void entriesWithoutAnIdAreSkipped() {
        store->begin();
        gateway->respond("previewInactiveTasks",
                         preview({QJsonObject{{QStringLiteral("name"), QStringLiteral("Nameless")}},
                                  candidate(QStringLiteral("b"), QStringLiteral("Beta"))}));
        QCOMPARE(taskIds(store->tasks()), (QStringList{QStringLiteral("b")}));
    }

    void anEmptyPreviewCannotBeConfirmed() {
        store->begin();
        gateway->respond("previewInactiveTasks", preview({}));
        QVERIFY(store->previewLoaded());
        QVERIFY(store->tasks().isEmpty());
        QVERIFY(!store->canSubmit());
        store->submit();
        QCOMPARE(gateway->count("archiveInactiveTasks"), 0);
    }

    void aFailedPreviewShowsTheErrorAndCannotBeConfirmed() {
        store->begin();
        gateway->respond("previewInactiveTasks", BridgeReply::failure(QStringLiteral("Server unavailable")));
        QVERIFY(!store->loading());
        QVERIFY(!store->previewLoaded());
        QCOMPARE(store->error(), QStringLiteral("Server unavailable"));
        QVERIFY(!store->canSubmit());
    }

    void changingTheDaysLoadsOneNewPreview() {
        store->begin();
        gateway->respond("previewInactiveTasks", twoCandidates());
        store->setDaysText(QStringLiteral("30"));
        QCOMPARE(gateway->count("previewInactiveTasks"), 2);
        QCOMPARE(gateway->lastArgs("previewInactiveTasks").first(), QStringLiteral("30"));
        QVERIFY(store->loading());
        QVERIFY(store->tasks().isEmpty());
        QVERIFY(!store->canSubmit());
    }

    void settingTheSameTextLoadsNothing() {
        store->begin();
        store->setDaysText(QStringLiteral("14"));
        QCOMPARE(gateway->count("previewInactiveTasks"), 1);
    }

    void aStaleReplyIsDroppedWhenTheNewerOneArrivesLater() {
        store->begin();
        store->setDaysText(QStringLiteral("30"));
        gateway->respond("previewInactiveTasks", preview({candidate(QStringLiteral("old"), QStringLiteral("Old"))}));
        // The reply to the 14-day request must not satisfy the 30-day one.
        QVERIFY(store->loading());
        QVERIFY(!store->previewLoaded());
        QVERIFY(store->tasks().isEmpty());
        gateway->respond("previewInactiveTasks", preview({candidate(QStringLiteral("new"), QStringLiteral("New"))}));
        QCOMPARE(taskIds(store->tasks()), (QStringList{QStringLiteral("new")}));
    }

    void aStaleReplyIsDroppedWhenTheNewerOneArrivedFirst() {
        store->begin();
        store->setDaysText(QStringLiteral("30"));
        // Answer the newest request first by taking the calls apart.
        auto stale = std::move(gateway->calls[0].done);
        gateway->calls[0].done = nullptr;
        gateway->respond("previewInactiveTasks", preview({candidate(QStringLiteral("new"), QStringLiteral("New"))}));
        stale(preview({candidate(QStringLiteral("old"), QStringLiteral("Old"))}));
        QCOMPARE(taskIds(store->tasks()), (QStringList{QStringLiteral("new")}));
        QVERIFY(store->previewLoaded());
    }

    void aStaleFailureDoesNotShowItsError() {
        store->begin();
        store->setDaysText(QStringLiteral("30"));
        gateway->respond("previewInactiveTasks", BridgeReply::failure(QStringLiteral("Old failure")));
        QVERIFY(store->error().isEmpty());
        QVERIFY(store->loading());
    }

    void confirmingSendsThePeriodAndTimeOfThePreviewThatWasShown() {
        store->begin();
        store->setDaysText(QStringLiteral("30"));
        gateway->respond("previewInactiveTasks", twoCandidates());
        gateway->respond("previewInactiveTasks", twoCandidates());
        now = QDateTime::fromString(kLaterStamp, Qt::ISODateWithMs);
        store->submit();
        QCOMPARE(gateway->lastArgs("archiveInactiveTasks"), (QStringList{QStringLiteral("30"), kFirstStamp}));
    }

    void aNonNumericPeriodDropsThePreviewAndSendsNothing() {
        store->begin();
        gateway->respond("previewInactiveTasks", twoCandidates());
        store->setDaysText(QStringLiteral("abc"));
        QCOMPARE(gateway->count("previewInactiveTasks"), 1);
        QVERIFY(store->tasks().isEmpty());
        QVERIFY(!store->previewLoaded());
        QVERIFY(!store->loading());
        QVERIFY(!store->canSubmit());
        QVERIFY(store->error().contains(QStringLiteral("whole number")));
    }

    void anInvalidPeriodIgnoresTheReplyOfAnEarlierRequest() {
        store->begin();
        store->setDaysText(QStringLiteral("0"));
        gateway->respond("previewInactiveTasks", twoCandidates());
        QVERIFY(!store->previewLoaded());
        QVERIFY(!store->canSubmit());
    }

    void fixingAnInvalidPeriodLoadsAPreviewAndClearsTheMessage() {
        store->begin();
        store->setDaysText(QStringLiteral(""));
        store->setDaysText(QStringLiteral("7"));
        QVERIFY(store->error().isEmpty());
        QCOMPARE(gateway->lastArgs("previewInactiveTasks").first(), QStringLiteral("7"));
        QVERIFY(store->loading());
    }

    void refreshLoadsANewPreviewAndClearsTheError() {
        store->begin();
        gateway->respond("previewInactiveTasks", BridgeReply::failure(QStringLiteral("Server unavailable")));
        store->refresh();
        QCOMPARE(gateway->count("previewInactiveTasks"), 2);
        QVERIFY(store->error().isEmpty());
        QVERIFY(store->loading());
    }

    void refreshNeedsAValidPeriod() {
        store->begin();
        store->setDaysText(QStringLiteral("x"));
        store->refresh();
        QCOMPARE(gateway->count("previewInactiveTasks"), 1);
    }

    void cancelDropsThePreviewAndTheRepliesOnTheirWay() {
        QSignalSpy state(store.get(), &BulkTaskArchivingStore::stateChanged);
        store->begin();
        state.clear();
        store->cancel();
        QCOMPARE(state.count(), 1);
        QVERIFY(!store->loading());
        gateway->respond("previewInactiveTasks", twoCandidates());
        QVERIFY(!store->previewLoaded());
        QVERIFY(store->tasks().isEmpty());
    }

    void confirmingNeedsALoadedPreview() {
        store->submit();
        store->begin();
        store->submit();
        QCOMPARE(gateway->count("archiveInactiveTasks"), 0);
    }

    void isPendingUntilTheReplyAndRejectsASecondConfirmation() {
        store->begin();
        gateway->respond("previewInactiveTasks", twoCandidates());
        store->submit();
        QVERIFY(store->pending());
        QVERIFY(!store->canSubmit());
        store->submit();
        QCOMPARE(gateway->count("archiveInactiveTasks"), 1);
    }

    void thePeriodAndPreviewAreFixedWhileConfirming() {
        store->begin();
        gateway->respond("previewInactiveTasks", twoCandidates());
        store->submit();
        store->setDaysText(QStringLiteral("30"));
        store->refresh();
        store->begin();
        QCOMPARE(store->daysText(), QStringLiteral("14"));
        QCOMPARE(gateway->count("previewInactiveTasks"), 1);
    }

    void successReportsTheActualCountAndRequestsARefresh() {
        QSignalSpy refresh(store.get(), &BulkTaskArchivingStore::refreshRequested);
        store->begin();
        gateway->respond("previewInactiveTasks", twoCandidates());
        store->submit();
        gateway->respond("archiveInactiveTasks", archived(1));
        QCOMPARE(store->archivedCount(), 1);
        QVERIFY(!store->pending());
        QVERIFY(store->error().isEmpty());
        QVERIFY(!store->previewLoaded());
        QVERIFY(store->tasks().isEmpty());
        QVERIFY(!store->canSubmit());
        QCOMPARE(refresh.count(), 1);
    }

    void aReplyWithoutACountMeansNothingWasArchived() {
        store->begin();
        gateway->respond("previewInactiveTasks", twoCandidates());
        store->submit();
        gateway->respond("archiveInactiveTasks", BridgeReply::success(QJsonObject{}));
        QCOMPARE(store->archivedCount(), 0);
    }

    void theResultFixesThePeriodAndBeginStartsOver() {
        store->begin();
        gateway->respond("previewInactiveTasks", twoCandidates());
        store->submit();
        gateway->respond("archiveInactiveTasks", archived(2));
        store->setDaysText(QStringLiteral("30"));
        store->refresh();
        QCOMPARE(store->daysText(), QStringLiteral("14"));
        QCOMPARE(gateway->count("previewInactiveTasks"), 1);
        store->begin();
        QCOMPARE(store->archivedCount(), -1);
        QCOMPARE(gateway->count("previewInactiveTasks"), 2);
    }

    void aFailedConfirmationNeedsAFreshPreview() {
        QSignalSpy refresh(store.get(), &BulkTaskArchivingStore::refreshRequested);
        store->begin();
        gateway->respond("previewInactiveTasks", twoCandidates());
        store->submit();
        gateway->respond("archiveInactiveTasks", BridgeReply::failure(QStringLiteral("Archive preview changed")));
        QCOMPARE(store->error(), QStringLiteral("Archive preview changed"));
        QCOMPARE(store->archivedCount(), -1);
        QVERIFY(!store->previewLoaded());
        QVERIFY(store->tasks().isEmpty());
        QVERIFY(!store->canSubmit());
        store->submit();
        QCOMPARE(gateway->count("archiveInactiveTasks"), 1);
        QCOMPARE(refresh.count(), 1);
        store->refresh();
        gateway->respond("previewInactiveTasks", twoCandidates());
        QVERIFY(store->canSubmit());
        QVERIFY(store->error().isEmpty());
    }

    void anUncertainFailureSaysTheChangeIsUnconfirmed() {
        BridgeReply reply = BridgeReply::failure(QStringLiteral("timed out"));
        reply.uncertain = true;
        store->begin();
        gateway->respond("previewInactiveTasks", twoCandidates());
        store->submit();
        gateway->respond("archiveInactiveTasks", reply);
        QVERIFY(store->error().contains(QStringLiteral("could not be confirmed")));
        QVERIFY(!store->error().contains(QStringLiteral("timed out")));
    }

    void repliesAfterTheStoreIsGoneAreIgnored() {
        store->begin();
        gateway->respond("previewInactiveTasks", twoCandidates());
        store->submit();
        store.reset();
        QVERIFY(gateway->respond("archiveInactiveTasks", archived(2)));
        init();
        store->begin();
        store.reset();
        QVERIFY(gateway->respond("previewInactiveTasks", twoCandidates()));
    }

private:
    QDateTime now;
    std::unique_ptr<FakeTrackerGateway> gateway;
    std::unique_ptr<DisplayClock> clock;
    std::unique_ptr<BulkTaskArchivingStore> store;
};

REGISTER_TEST(BulkTaskArchivingTests)
#include "BulkTaskArchivingTests.moc"

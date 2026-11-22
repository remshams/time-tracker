#include <QJsonObject>
#include <QSignalSpy>
#include <QTest>

#include "FakeTrackerGateway.h"
#include "Features/TaskArchiving/TaskArchivingStore.h"
#include "Features/TaskCatalog/TaskCatalogStore.h"
#include "Shared/DisplayClock.h"
#include "TestRegistry.h"

namespace {

const QDateTime kNow = QDateTime::fromString(QStringLiteral("2026-05-04T10:00:00.000Z"), Qt::ISODateWithMs);
const QString kStamp = QStringLiteral("2026-05-04T10:00:00.000Z");

TaskItem task(const QString &id, const QString &name, bool archived = false) {
    TaskItem item;
    item.id = id;
    item.name = name;
    item.archived = archived;
    return item;
}

BridgeReply changed(const QString &id) {
    return BridgeReply::success(QJsonObject{{QStringLiteral("task"), QJsonObject{{QStringLiteral("id"), id}}}});
}

} // namespace

class TaskArchivingTests : public QObject {
    Q_OBJECT
private slots:
    void init() {
        gateway = std::make_unique<FakeTrackerGateway>();
        clock = std::make_unique<DisplayClock>([] { return kNow; }, false);
        catalog = std::make_unique<TaskCatalogStore>(clock.get());
        catalog->accept({task(QStringLiteral("a"), QStringLiteral("Alpha")),
                         task(QStringLiteral("z"), QStringLiteral("Zulu"), true)});
        store = std::make_unique<TaskArchivingStore>(gateway.get(), catalog.get(), clock.get());
    }

    void anActiveTaskAsksForConfirmationWithoutWriting() {
        QSignalSpy dialog(store.get(), &TaskArchivingStore::dialogRequested);
        store->begin(QStringLiteral("a"));
        QCOMPARE(dialog.count(), 1);
        QVERIFY(!store->unarchive());
        QCOMPARE(store->taskName(), QStringLiteral("Alpha"));
        QVERIFY(store->canSubmit());
        QVERIFY(store->error().isEmpty());
        QCOMPARE(gateway->calls.size(), 0);
    }

    void confirmingSendsTheTaskTheNameAndTheTime() {
        store->begin(QStringLiteral("a"));
        store->submit();
        QCOMPARE(gateway->lastArgs("archiveTask"), (QStringList{QStringLiteral("a"), QStringLiteral("Alpha"), kStamp}));
        QCOMPARE(gateway->count("unarchiveTask"), 0);
    }

    void theConfirmedNameSurvivesARenameOnAnotherClient() {
        store->begin(QStringLiteral("a"));
        catalog->accept({task(QStringLiteral("a"), QStringLiteral("Renamed elsewhere"))});
        store->submit();
        QCOMPARE(gateway->lastArgs("archiveTask").at(1), QStringLiteral("Alpha"));
        QCOMPARE(store->taskName(), QStringLiteral("Alpha"));
    }

    void theClickedTaskWinsOverTheSelection() {
        catalog->select(QStringLiteral("a"));
        catalog->accept(
            {task(QStringLiteral("a"), QStringLiteral("Alpha")), task(QStringLiteral("b"), QStringLiteral("Beta"))});
        store->begin(QStringLiteral("b"));
        catalog->select(QStringLiteral("a"));
        store->submit();
        QCOMPARE(gateway->lastArgs("archiveTask").first(), QStringLiteral("b"));
    }

    void anArchivedTaskIsUnarchivedAtOnce() {
        QSignalSpy dialog(store.get(), &TaskArchivingStore::dialogRequested);
        store->begin(QStringLiteral("z"));
        QCOMPARE(gateway->lastArgs("unarchiveTask"), (QStringList{QStringLiteral("z"), kStamp}));
        QCOMPARE(gateway->count("archiveTask"), 0);
        QVERIFY(store->unarchive());
        QVERIFY(store->pending());
        QCOMPARE(dialog.count(), 0);
    }

    void aSuccessfulUnarchiveStaysSilentAndRefreshes() {
        QSignalSpy dialog(store.get(), &TaskArchivingStore::dialogRequested);
        QSignalSpy refresh(store.get(), &TaskArchivingStore::refreshRequested);
        QSignalSpy succeeded(store.get(), &TaskArchivingStore::succeeded);
        store->begin(QStringLiteral("z"));
        gateway->respond("unarchiveTask", changed(QStringLiteral("z")));
        QCOMPARE(dialog.count(), 0);
        QCOMPARE(refresh.count(), 1);
        QCOMPARE(succeeded.count(), 1);
        QVERIFY(!store->pending());
        QVERIFY(store->error().isEmpty());
    }

    void aSuccessfulUnarchiveSelectsTheTaskOnTheNextSwitchToActive() {
        catalog->setArchivedTab(true);
        store->begin(QStringLiteral("z"));
        QVERIFY(gateway->respond(QStringLiteral("unarchiveTask"), changed(QStringLiteral("z"))));
        // The refresh brings the task back as active.
        catalog->accept(
            {task(QStringLiteral("a"), QStringLiteral("Alpha")), task(QStringLiteral("z"), QStringLiteral("Zulu"))});
        catalog->setArchivedTab(false);
        QCOMPARE(catalog->selectedTaskId(), QStringLiteral("z"));
    }

    void anArchiveDoesNotChangeTheActiveSelectionMemory() {
        catalog->accept(
            {task(QStringLiteral("a"), QStringLiteral("Alpha")), task(QStringLiteral("b"), QStringLiteral("Beta"))});
        store->begin(QStringLiteral("b"));
        store->submit();
        QVERIFY(gateway->respond(QStringLiteral("archiveTask"), changed(QStringLiteral("b"))));
        QCOMPARE(catalog->selectedTaskId(), QStringLiteral("a"));
    }

    void aFailedUnarchiveOpensTheDialogWithTheError() {
        QSignalSpy dialog(store.get(), &TaskArchivingStore::dialogRequested);
        QSignalSpy refresh(store.get(), &TaskArchivingStore::refreshRequested);
        QSignalSpy succeeded(store.get(), &TaskArchivingStore::succeeded);
        store->begin(QStringLiteral("z"));
        gateway->respond("unarchiveTask", BridgeReply::failure(QStringLiteral("Task not found")));
        QCOMPARE(dialog.count(), 1);
        QCOMPARE(store->error(), QStringLiteral("Task not found"));
        QCOMPARE(refresh.count(), 1);
        QCOMPARE(succeeded.count(), 0);
        // The dialog retries with the same action.
        QVERIFY(store->canSubmit());
        store->submit();
        QCOMPARE(gateway->count("unarchiveTask"), 2);
        QVERIFY(store->error().isEmpty());
    }

    void anUncertainFailureSaysTheChangeIsUnconfirmed() {
        BridgeReply reply = BridgeReply::failure(QStringLiteral("timed out"));
        reply.uncertain = true;
        store->begin(QStringLiteral("z"));
        gateway->respond("unarchiveTask", reply);
        QVERIFY(store->error().contains(QStringLiteral("could not be confirmed")));
        QVERIFY(!store->error().contains(QStringLiteral("timed out")));
    }

    void aSuccessfulArchiveClosesTheDialogAndRefreshes() {
        QSignalSpy refresh(store.get(), &TaskArchivingStore::refreshRequested);
        QSignalSpy succeeded(store.get(), &TaskArchivingStore::succeeded);
        QSignalSpy dialog(store.get(), &TaskArchivingStore::dialogRequested);
        store->begin(QStringLiteral("a"));
        store->submit();
        QVERIFY(store->pending());
        QVERIFY(!store->canSubmit());
        gateway->respond("archiveTask", changed(QStringLiteral("a")));
        QCOMPARE(succeeded.count(), 1);
        QCOMPARE(refresh.count(), 1);
        QCOMPARE(dialog.count(), 1);
        QVERIFY(!store->pending());
    }

    void aRejectedArchiveShowsTheMessageAndKeepsTheDialog() {
        QSignalSpy refresh(store.get(), &TaskArchivingStore::refreshRequested);
        QSignalSpy succeeded(store.get(), &TaskArchivingStore::succeeded);
        store->begin(QStringLiteral("a"));
        store->submit();
        gateway->respond("archiveTask", BridgeReply::failure(QStringLiteral("Stop tracking before archiving")));
        QCOMPARE(store->error(), QStringLiteral("Stop tracking before archiving"));
        QCOMPARE(succeeded.count(), 0);
        QCOMPARE(refresh.count(), 1);
        QVERIFY(store->canSubmit());
    }

    void aSecondSubmitWhilePendingSendsNothing() {
        store->begin(QStringLiteral("a"));
        store->submit();
        store->submit();
        QCOMPARE(gateway->count("archiveTask"), 1);
    }

    void submittingWithoutACommandSendsNothing() {
        QVERIFY(!store->canSubmit());
        store->submit();
        QCOMPARE(gateway->calls.size(), 0);
    }

    void anUnknownTaskIsIgnored() {
        QSignalSpy dialog(store.get(), &TaskArchivingStore::dialogRequested);
        store->begin(QStringLiteral("missing"));
        QCOMPARE(dialog.count(), 0);
        QCOMPARE(gateway->calls.size(), 0);
        QVERIFY(!store->canSubmit());
    }

    void aNewCommandIsIgnoredWhileOneRuns() {
        store->begin(QStringLiteral("a"));
        store->submit();
        store->begin(QStringLiteral("z"));
        QCOMPARE(gateway->count("unarchiveTask"), 0);
        QCOMPARE(store->taskName(), QStringLiteral("Alpha"));
    }

    void aNewCommandClearsTheOldError() {
        store->begin(QStringLiteral("a"));
        store->submit();
        gateway->respond("archiveTask", BridgeReply::failure(QStringLiteral("No")));
        store->begin(QStringLiteral("a"));
        QVERIFY(store->error().isEmpty());
    }

    void aReplyAfterTheStoreIsGoneIsIgnored() {
        store->begin(QStringLiteral("a"));
        store->submit();
        store.reset();
        QVERIFY(gateway->respond("archiveTask", changed(QStringLiteral("a"))));
    }

private:
    std::unique_ptr<FakeTrackerGateway> gateway;
    std::unique_ptr<DisplayClock> clock;
    std::unique_ptr<TaskCatalogStore> catalog;
    std::unique_ptr<TaskArchivingStore> store;
};

REGISTER_TEST(TaskArchivingTests)
#include "TaskArchivingTests.moc"

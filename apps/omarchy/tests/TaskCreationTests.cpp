#include <QJsonObject>
#include <QSignalSpy>
#include <QTest>

#include "FakeTrackerGateway.h"
#include "Features/TaskCatalog/TaskCatalogStore.h"
#include "Features/TaskCreation/TaskCreationStore.h"
#include "Shared/DisplayClock.h"
#include "TestRegistry.h"

namespace {

const QDateTime kNow = QDateTime::fromString(QStringLiteral("2026-05-04T10:00:00.000Z"), Qt::ISODateWithMs);
const QString kStamp = QStringLiteral("2026-05-04T10:00:00.000Z");

BridgeReply created(const QString &id, const QString &name) {
    return BridgeReply::success(
        QJsonObject{{QStringLiteral("task"), QJsonObject{{QStringLiteral("id"), id}, {QStringLiteral("name"), name}}}});
}

TaskItem task(const QString &id, const QString &name, bool archived = false) {
    TaskItem item;
    item.id = id;
    item.name = name;
    item.archived = archived;
    return item;
}

} // namespace

class TaskCreationTests : public QObject {
    Q_OBJECT
private slots:
    void init() {
        gateway = std::make_unique<FakeTrackerGateway>();
        now = kNow;
        clock = std::make_unique<DisplayClock>([this] { return now; }, false);
        catalog = std::make_unique<TaskCatalogStore>(clock.get());
        store = std::make_unique<TaskCreationStore>(gateway.get(), catalog.get(), clock.get());
    }

    void sendsTheTrimmedNameWithTheClickTime() {
        store->setDraft(QStringLiteral("  Write docs \n"));
        store->submit();
        QCOMPARE(gateway->count("createTask"), 1);
        QCOMPARE(gateway->lastArgs("createTask"), (QStringList{QStringLiteral("Write docs"), kStamp}));
    }

    void anEmptyOrBlankDraftSendsNothing() {
        QVERIFY(!store->canSubmit());
        store->submit();
        store->setDraft(QStringLiteral("   "));
        QVERIFY(!store->canSubmit());
        store->submit();
        QCOMPARE(gateway->count("createTask"), 0);
        QVERIFY(!store->pending());
    }

    void aDraftEnablesSubmitting() {
        QSignalSpy state(store.get(), &TaskCreationStore::stateChanged);
        store->setDraft(QStringLiteral("Plan"));
        QVERIFY(store->canSubmit());
        QCOMPARE(state.count(), 1);
    }

    void settingTheSameDraftChangesNothing() {
        store->setDraft(QStringLiteral("Plan"));
        QSignalSpy draft(store.get(), &TaskCreationStore::draftChanged);
        QSignalSpy state(store.get(), &TaskCreationStore::stateChanged);
        store->setDraft(QStringLiteral("Plan"));
        QCOMPARE(draft.count(), 0);
        QCOMPARE(state.count(), 0);
        QCOMPARE(store->draft(), QStringLiteral("Plan"));
    }

    void isPendingUntilTheReplyAndRejectsASecondSubmit() {
        store->setDraft(QStringLiteral("Plan"));
        store->submit();
        QVERIFY(store->pending());
        QVERIFY(!store->canSubmit());
        store->submit();
        QCOMPARE(gateway->count("createTask"), 1);
        QVERIFY(gateway->respond("createTask", created(QStringLiteral("t1"), QStringLiteral("Plan"))));
        QVERIFY(!store->pending());
    }

    void successAnnouncesAndRequestsARefresh() {
        QSignalSpy succeeded(store.get(), &TaskCreationStore::succeeded);
        QSignalSpy refresh(store.get(), &TaskCreationStore::refreshRequested);
        store->setDraft(QStringLiteral("Plan"));
        store->submit();
        gateway->respond("createTask", created(QStringLiteral("t1"), QStringLiteral("Plan")));
        QCOMPARE(succeeded.count(), 1);
        QCOMPARE(refresh.count(), 1);
        QVERIFY(store->error().isEmpty());
    }

    void selectsTheNewTaskInActiveOnceTheRefreshDeliversIt() {
        catalog->accept(
            {task(QStringLiteral("old"), QStringLiteral("Old"), true), task(QStringLiteral("a"), QStringLiteral("A"))});
        catalog->select(QStringLiteral("old"));
        QVERIFY(catalog->archivedTab());
        store->setDraft(QStringLiteral("Plan"));
        store->submit();
        gateway->respond("createTask", created(QStringLiteral("t1"), QStringLiteral("Plan")));
        // The reply alone does not select: the task list has not seen the task yet.
        QVERIFY(catalog->archivedTab());
        catalog->accept({task(QStringLiteral("old"), QStringLiteral("Old"), true),
                         task(QStringLiteral("a"), QStringLiteral("A")),
                         task(QStringLiteral("t1"), QStringLiteral("Plan"))});
        QVERIFY(!catalog->archivedTab());
        QCOMPARE(catalog->selectedTaskId(), QStringLiteral("t1"));
    }

    void aMuchLaterListDoesNotTakeTheSelection() {
        catalog->accept({task(QStringLiteral("a"), QStringLiteral("A"))});
        store->setDraft(QStringLiteral("Plan"));
        store->submit();
        gateway->respond("createTask", created(QStringLiteral("t1"), QStringLiteral("Plan")));
        // The reload after the create failed; the next list arrives with a later poll.
        now = kNow.addSecs(30);
        catalog->accept(
            {task(QStringLiteral("a"), QStringLiteral("A")), task(QStringLiteral("t1"), QStringLiteral("Plan"))});
        QCOMPARE(catalog->selectedTaskId(), QStringLiteral("a"));
    }

    void aListWithinTheWindowStillTakesTheSelection() {
        catalog->accept({task(QStringLiteral("a"), QStringLiteral("A"))});
        store->setDraft(QStringLiteral("Plan"));
        store->submit();
        gateway->respond("createTask", created(QStringLiteral("t1"), QStringLiteral("Plan")));
        now = kNow.addSecs(10);
        catalog->accept(
            {task(QStringLiteral("a"), QStringLiteral("A")), task(QStringLiteral("t1"), QStringLiteral("Plan"))});
        QCOMPARE(catalog->selectedTaskId(), QStringLiteral("t1"));
    }

    void anEarlierRefreshWithoutTheTaskKeepsWaiting() {
        store->setDraft(QStringLiteral("Plan"));
        store->submit();
        gateway->respond("createTask", created(QStringLiteral("t1"), QStringLiteral("Plan")));
        catalog->accept({task(QStringLiteral("a"), QStringLiteral("A"))});
        QCOMPARE(catalog->selectedTaskId(), QStringLiteral("a"));
        catalog->accept(
            {task(QStringLiteral("a"), QStringLiteral("A")), task(QStringLiteral("t1"), QStringLiteral("Plan"))});
        QCOMPARE(catalog->selectedTaskId(), QStringLiteral("t1"));
    }

    void selectsAtOnceWhenTheListAlreadyHasTheTask() {
        catalog->accept(
            {task(QStringLiteral("a"), QStringLiteral("A")), task(QStringLiteral("t1"), QStringLiteral("Plan"))});
        catalog->select(QStringLiteral("a"));
        store->setDraft(QStringLiteral("Plan"));
        store->submit();
        gateway->respond("createTask", created(QStringLiteral("t1"), QStringLiteral("Plan")));
        QCOMPARE(catalog->selectedTaskId(), QStringLiteral("t1"));
    }

    void selectsTheCreatedTaskOnlyOnce() {
        const QList<TaskItem> tasks{task(QStringLiteral("a"), QStringLiteral("A")),
                                    task(QStringLiteral("t1"), QStringLiteral("Plan"))};
        store->setDraft(QStringLiteral("Plan"));
        store->submit();
        gateway->respond("createTask", created(QStringLiteral("t1"), QStringLiteral("Plan")));
        catalog->accept(tasks);
        catalog->select(QStringLiteral("a"));
        catalog->accept(tasks);
        QCOMPARE(catalog->selectedTaskId(), QStringLiteral("a"));
    }

    void aReplyWithoutATaskSelectsNothing() {
        catalog->accept({task(QStringLiteral("a"), QStringLiteral("A"))});
        store->setDraft(QStringLiteral("Plan"));
        store->submit();
        gateway->respond("createTask", BridgeReply::success(QJsonObject{}));
        catalog->accept(
            {task(QStringLiteral("a"), QStringLiteral("A")), task(QStringLiteral("b"), QStringLiteral("B"))});
        QCOMPARE(catalog->selectedTaskId(), QStringLiteral("a"));
    }

    void anErrorKeepsTheDraftAndShowsTheMessage() {
        QSignalSpy succeeded(store.get(), &TaskCreationStore::succeeded);
        QSignalSpy refresh(store.get(), &TaskCreationStore::refreshRequested);
        store->setDraft(QStringLiteral("Plan"));
        store->submit();
        gateway->respond("createTask", BridgeReply::failure(QStringLiteral("Task name must contain text")));
        QCOMPARE(store->error(), QStringLiteral("Task name must contain text"));
        QCOMPARE(store->draft(), QStringLiteral("Plan"));
        QVERIFY(!store->pending());
        QVERIFY(store->canSubmit());
        QCOMPARE(succeeded.count(), 0);
        QCOMPARE(refresh.count(), 1);
    }

    void anErrorSelectsNothing() {
        catalog->accept({task(QStringLiteral("a"), QStringLiteral("A"))});
        store->setDraft(QStringLiteral("Plan"));
        store->submit();
        gateway->respond("createTask", BridgeReply::failure(QStringLiteral("No")));
        catalog->accept(
            {task(QStringLiteral("a"), QStringLiteral("A")), task(QStringLiteral("b"), QStringLiteral("B"))});
        QCOMPARE(catalog->selectedTaskId(), QStringLiteral("a"));
    }

    void anUncertainErrorSaysTheChangeIsUnconfirmed() {
        BridgeReply reply = BridgeReply::failure(QStringLiteral("timed out"));
        reply.uncertain = true;
        store->setDraft(QStringLiteral("Plan"));
        store->submit();
        gateway->respond("createTask", reply);
        QVERIFY(store->error().contains(QStringLiteral("could not be confirmed")));
        QVERIFY(!store->error().contains(QStringLiteral("timed out")));
    }

    void submittingAgainClearsTheOldError() {
        store->setDraft(QStringLiteral("Plan"));
        store->submit();
        gateway->respond("createTask", BridgeReply::failure(QStringLiteral("No")));
        store->submit();
        QVERIFY(store->error().isEmpty());
        QVERIFY(store->pending());
        QCOMPARE(gateway->count("createTask"), 2);
    }

    void beginStartsAFreshDraft() {
        store->setDraft(QStringLiteral("Plan"));
        store->submit();
        gateway->respond("createTask", BridgeReply::failure(QStringLiteral("No")));
        QSignalSpy draft(store.get(), &TaskCreationStore::draftChanged);
        store->begin();
        QVERIFY(store->draft().isEmpty());
        QVERIFY(store->error().isEmpty());
        QCOMPARE(draft.count(), 1);
    }

    void beginForgetsAnUnfinishedSelection() {
        store->setDraft(QStringLiteral("Plan"));
        store->submit();
        gateway->respond("createTask", created(QStringLiteral("t1"), QStringLiteral("Plan")));
        store->begin();
        catalog->accept(
            {task(QStringLiteral("a"), QStringLiteral("A")), task(QStringLiteral("t1"), QStringLiteral("Plan"))});
        QCOMPARE(catalog->selectedTaskId(), QStringLiteral("a"));
    }

    void aReplyAfterTheStoreIsGoneIsIgnored() {
        store->setDraft(QStringLiteral("Plan"));
        store->submit();
        store.reset();
        QVERIFY(gateway->respond("createTask", created(QStringLiteral("t1"), QStringLiteral("Plan"))));
    }

private:
    std::unique_ptr<FakeTrackerGateway> gateway;
    QDateTime now;
    std::unique_ptr<DisplayClock> clock;
    std::unique_ptr<TaskCatalogStore> catalog;
    std::unique_ptr<TaskCreationStore> store;
};

REGISTER_TEST(TaskCreationTests)
#include "TaskCreationTests.moc"

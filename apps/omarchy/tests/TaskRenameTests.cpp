#include <QJsonObject>
#include <QSignalSpy>
#include <QTest>

#include "FakeTrackerGateway.h"
#include "Features/TaskCatalog/TaskCatalogStore.h"
#include "Features/TaskRename/TaskRenameStore.h"
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

BridgeReply renamed(const QString &id, const QString &name) {
    return BridgeReply::success(
        QJsonObject{{QStringLiteral("task"), QJsonObject{{QStringLiteral("id"), id}, {QStringLiteral("name"), name}}}});
}

} // namespace

class TaskRenameTests : public QObject {
    Q_OBJECT
private slots:
    void init() {
        gateway = std::make_unique<FakeTrackerGateway>();
        clock = std::make_unique<DisplayClock>([] { return kNow; }, false);
        catalog = std::make_unique<TaskCatalogStore>(clock.get());
        catalog->accept({task(QStringLiteral("a"), QStringLiteral("Alpha")),
                         task(QStringLiteral("z"), QStringLiteral("Zulu"), true)});
        store = std::make_unique<TaskRenameStore>(gateway.get(), catalog.get(), clock.get());
    }

    void beginPrefillsTheCurrentName() {
        QSignalSpy draft(store.get(), &TaskRenameStore::draftChanged);
        QVERIFY(store->begin(QStringLiteral("a")));
        QCOMPARE(store->draft(), QStringLiteral("Alpha"));
        QCOMPARE(store->originalName(), QStringLiteral("Alpha"));
        QCOMPARE(draft.count(), 1);
        QVERIFY(store->error().isEmpty());
        QVERIFY(!store->pending());
    }

    void beginReadsArchivedTasksToo() {
        QVERIFY(store->begin(QStringLiteral("z")));
        QCOMPARE(store->draft(), QStringLiteral("Zulu"));
    }

    void beginRejectsAnUnknownTask() {
        QVERIFY(!store->begin(QStringLiteral("missing")));
        QVERIFY(!store->canSubmit());
        store->submit();
        QCOMPARE(gateway->count("renameTask"), 0);
    }

    void beginClearsAnOldErrorAndDraft() {
        store->begin(QStringLiteral("a"));
        store->setDraft(QStringLiteral("Beta"));
        store->submit();
        gateway->respond("renameTask", BridgeReply::failure(QStringLiteral("No")));
        QVERIFY(store->begin(QStringLiteral("z")));
        QVERIFY(store->error().isEmpty());
        QCOMPARE(store->draft(), QStringLiteral("Zulu"));
    }

    void beginIsRefusedWhileAWriteRuns() {
        store->begin(QStringLiteral("a"));
        store->setDraft(QStringLiteral("Beta"));
        store->submit();
        QVERIFY(!store->begin(QStringLiteral("z")));
        QCOMPARE(store->originalName(), QStringLiteral("Alpha"));
    }

    void sendsTheOpenedNameAsTheExpectedName() {
        store->begin(QStringLiteral("a"));
        store->setDraft(QStringLiteral("  Beta  "));
        store->submit();
        QCOMPARE(gateway->lastArgs("renameTask"),
                 (QStringList{QStringLiteral("a"), QStringLiteral("Alpha"), QStringLiteral("Beta"), kStamp}));
    }

    void keepsTheCapturedTaskWhenTheSelectionMoves() {
        store->begin(QStringLiteral("a"));
        catalog->select(QStringLiteral("z"));
        store->setDraft(QStringLiteral("Beta"));
        store->submit();
        QCOMPARE(gateway->lastArgs("renameTask").first(), QStringLiteral("a"));
    }

    void keepsTheDraftAndExpectedNameThroughARefresh() {
        store->begin(QStringLiteral("a"));
        store->setDraft(QStringLiteral("Beta"));
        catalog->accept({task(QStringLiteral("a"), QStringLiteral("Renamed elsewhere"))});
        QCOMPARE(store->draft(), QStringLiteral("Beta"));
        store->submit();
        QCOMPARE(gateway->lastArgs("renameTask").at(1), QStringLiteral("Alpha"));
    }

    void anUnchangedOrEmptyNameCannotBeSaved() {
        store->begin(QStringLiteral("a"));
        QVERIFY(!store->canSubmit());
        store->setDraft(QStringLiteral(" Alpha "));
        QVERIFY(!store->canSubmit());
        store->setDraft(QStringLiteral("  "));
        QVERIFY(!store->canSubmit());
        store->submit();
        QCOMPARE(gateway->count("renameTask"), 0);
        store->setDraft(QStringLiteral("Beta"));
        QVERIFY(store->canSubmit());
    }

    void settingTheSameDraftChangesNothing() {
        store->begin(QStringLiteral("a"));
        QSignalSpy draft(store.get(), &TaskRenameStore::draftChanged);
        QSignalSpy state(store.get(), &TaskRenameStore::stateChanged);
        store->setDraft(QStringLiteral("Alpha"));
        QCOMPARE(draft.count(), 0);
        QCOMPARE(state.count(), 0);
    }

    void isPendingUntilTheReplyAndRejectsASecondSubmit() {
        store->begin(QStringLiteral("a"));
        store->setDraft(QStringLiteral("Beta"));
        store->submit();
        QVERIFY(store->pending());
        QVERIFY(!store->canSubmit());
        store->submit();
        QCOMPARE(gateway->count("renameTask"), 1);
        gateway->respond("renameTask", renamed(QStringLiteral("a"), QStringLiteral("Beta")));
        QVERIFY(!store->pending());
    }

    void successAnnouncesAndRequestsARefresh() {
        QSignalSpy succeeded(store.get(), &TaskRenameStore::succeeded);
        QSignalSpy refresh(store.get(), &TaskRenameStore::refreshRequested);
        store->begin(QStringLiteral("a"));
        store->setDraft(QStringLiteral("Beta"));
        store->submit();
        gateway->respond("renameTask", renamed(QStringLiteral("a"), QStringLiteral("Beta")));
        QCOMPARE(succeeded.count(), 1);
        QCOMPARE(refresh.count(), 1);
        QVERIFY(store->error().isEmpty());
    }

    void anErrorKeepsTheDraftAndRequestsARefresh() {
        QSignalSpy succeeded(store.get(), &TaskRenameStore::succeeded);
        QSignalSpy refresh(store.get(), &TaskRenameStore::refreshRequested);
        store->begin(QStringLiteral("a"));
        store->setDraft(QStringLiteral("Beta"));
        store->submit();
        gateway->respond("renameTask", BridgeReply::failure(QStringLiteral("The task was renamed by another client")));
        QCOMPARE(store->error(), QStringLiteral("The task was renamed by another client"));
        QCOMPARE(store->draft(), QStringLiteral("Beta"));
        QVERIFY(store->canSubmit());
        QCOMPARE(succeeded.count(), 0);
        QCOMPARE(refresh.count(), 1);
    }

    void anUncertainErrorSaysTheChangeIsUnconfirmed() {
        BridgeReply reply = BridgeReply::failure(QStringLiteral("timed out"));
        reply.uncertain = true;
        store->begin(QStringLiteral("a"));
        store->setDraft(QStringLiteral("Beta"));
        store->submit();
        gateway->respond("renameTask", reply);
        QVERIFY(store->error().contains(QStringLiteral("could not be confirmed")));
        QVERIFY(!store->error().contains(QStringLiteral("timed out")));
    }

    void submittingAgainClearsTheOldError() {
        store->begin(QStringLiteral("a"));
        store->setDraft(QStringLiteral("Beta"));
        store->submit();
        gateway->respond("renameTask", BridgeReply::failure(QStringLiteral("No")));
        store->submit();
        QVERIFY(store->error().isEmpty());
        QCOMPARE(gateway->count("renameTask"), 2);
    }

    void aReplyAfterTheStoreIsGoneIsIgnored() {
        store->begin(QStringLiteral("a"));
        store->setDraft(QStringLiteral("Beta"));
        store->submit();
        store.reset();
        QVERIFY(gateway->respond("renameTask", renamed(QStringLiteral("a"), QStringLiteral("Beta"))));
    }

private:
    std::unique_ptr<FakeTrackerGateway> gateway;
    std::unique_ptr<DisplayClock> clock;
    std::unique_ptr<TaskCatalogStore> catalog;
    std::unique_ptr<TaskRenameStore> store;
};

REGISTER_TEST(TaskRenameTests)
#include "TaskRenameTests.moc"

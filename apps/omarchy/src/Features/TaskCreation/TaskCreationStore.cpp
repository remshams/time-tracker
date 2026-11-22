#include "Features/TaskCreation/TaskCreationStore.h"

#include <QJsonObject>
#include <QPointer>

#include "Contracts/TrackerGateway.h"
#include "Features/TaskCatalog/TaskCatalogStore.h"
#include "Shared/DisplayClock.h"

TaskCreationStore::TaskCreationStore(TrackerGateway *gateway, TaskCatalogStore *catalog, DisplayClock *clock,
                                     QObject *parent)
    : QObject(parent), gateway_(gateway), catalog_(catalog), clock_(clock) {
    // The created task reaches the list with the refresh that follows the write, not with the reply.
    connect(catalog_, &TaskCatalogStore::tasksChanged, this, &TaskCreationStore::selectCreated);
}

void TaskCreationStore::setDraft(const QString &draft) {
    if (draft_ == draft) {
        return;
    }
    draft_ = draft;
    emit draftChanged();
    emit stateChanged();
}

void TaskCreationStore::begin() {
    draft_.clear();
    error_.clear();
    createdId_.clear();
    emit draftChanged();
    emit stateChanged();
}

void TaskCreationStore::submit() {
    if (!canSubmit()) {
        return;
    }
    error_.clear();
    pending_ = true;
    emit stateChanged();
    QPointer<TaskCreationStore> self(this);
    gateway_->createTask(draft_.trimmed(), clock_->now(), [self](const BridgeReply &reply) {
        if (self) {
            self->finish(reply);
        }
    });
}

void TaskCreationStore::finish(const BridgeReply &reply) {
    pending_ = false;
    if (reply.ok) {
        createdId_ =
            reply.data.toObject().value(QStringLiteral("task")).toObject().value(QStringLiteral("id")).toString();
        error_.clear();
    } else {
        error_ =
            reply.userMessage(tr("The change could not be confirmed. Check the reloaded list before trying again."));
    }
    emit stateChanged();
    if (reply.ok) {
        emit succeeded();
    }
    emit refreshRequested();
    selectCreated();
}

void TaskCreationStore::selectCreated() {
    if (createdId_.isEmpty() || !catalog_->task(createdId_)) {
        return;
    }
    const QString taskId = createdId_;
    createdId_.clear();
    catalog_->select(taskId);
}

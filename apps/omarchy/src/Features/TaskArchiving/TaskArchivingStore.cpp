#include "Features/TaskArchiving/TaskArchivingStore.h"

#include <QPointer>

#include "Contracts/TrackerGateway.h"
#include "Features/TaskCatalog/TaskCatalogStore.h"
#include "Shared/DisplayClock.h"

TaskArchivingStore::TaskArchivingStore(TrackerGateway *gateway, TaskCatalogStore *catalog, DisplayClock *clock,
                                       QObject *parent)
    : QObject(parent), gateway_(gateway), catalog_(catalog), clock_(clock) {}

void TaskArchivingStore::begin(const QString &taskId) {
    const std::optional<TaskItem> item = catalog_->task(taskId);
    if (!item || pending_) {
        return;
    }
    taskId_ = item->id;
    taskName_ = item->name;
    unarchive_ = item->archived;
    error_.clear();
    emit stateChanged();
    if (unarchive_) {
        submit();
    } else {
        emit dialogRequested();
    }
}

void TaskArchivingStore::submit() {
    if (!canSubmit()) {
        return;
    }
    error_.clear();
    pending_ = true;
    emit stateChanged();
    QPointer<TaskArchivingStore> self(this);
    auto done = [self](const BridgeReply &reply) {
        if (self) {
            self->finish(reply);
        }
    };
    if (unarchive_) {
        gateway_->unarchiveTask(taskId_, clock_->now(), done);
    } else {
        gateway_->archiveTask(taskId_, taskName_, clock_->now(), done);
    }
}

void TaskArchivingStore::finish(const BridgeReply &reply) {
    pending_ = false;
    if (!reply.ok) {
        error_ =
            reply.userMessage(tr("The change could not be confirmed. Check the reloaded list before trying again."));
    }
    emit stateChanged();
    if (reply.ok && unarchive_) {
        catalog_->rememberForActive(taskId_);
    }
    if (reply.ok) {
        emit succeeded();
    } else {
        // An unarchive has no dialog yet, so a failure opens one to show the message.
        emit dialogRequested();
    }
    emit refreshRequested();
}

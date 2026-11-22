#include "Features/TaskRename/TaskRenameStore.h"

#include <QPointer>

#include "Contracts/TrackerGateway.h"
#include "Features/TaskCatalog/TaskCatalogStore.h"
#include "Shared/DisplayClock.h"

TaskRenameStore::TaskRenameStore(TrackerGateway *gateway, TaskCatalogStore *catalog, DisplayClock *clock,
                                 QObject *parent)
    : QObject(parent), gateway_(gateway), catalog_(catalog), clock_(clock) {}

void TaskRenameStore::setDraft(const QString &draft) {
    if (draft_ == draft) {
        return;
    }
    draft_ = draft;
    emit draftChanged();
    emit stateChanged();
}

bool TaskRenameStore::canSubmit() const {
    const QString name = draft_.trimmed();
    return !taskId_.isEmpty() && !pending_ && !name.isEmpty() && name != originalName_;
}

bool TaskRenameStore::begin(const QString &taskId) {
    const std::optional<TaskItem> item = catalog_->task(taskId);
    if (!item || pending_) {
        return false;
    }
    taskId_ = item->id;
    originalName_ = item->name;
    draft_ = item->name;
    error_.clear();
    emit draftChanged();
    emit stateChanged();
    return true;
}

void TaskRenameStore::submit() {
    if (!canSubmit()) {
        return;
    }
    error_.clear();
    pending_ = true;
    emit stateChanged();
    QPointer<TaskRenameStore> self(this);
    gateway_->renameTask(taskId_, originalName_, draft_.trimmed(), clock_->now(), [self](const BridgeReply &reply) {
        if (self) {
            self->finish(reply);
        }
    });
}

void TaskRenameStore::finish(const BridgeReply &reply) {
    pending_ = false;
    if (!reply.ok) {
        error_ =
            reply.userMessage(tr("The change could not be confirmed. Check the reloaded list before trying again."));
    }
    emit stateChanged();
    if (reply.ok) {
        emit succeeded();
    }
    emit refreshRequested();
}

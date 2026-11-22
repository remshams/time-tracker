#include "Features/WorklogDeletion/WorklogDeletionStore.h"

#include <QPointer>

#include "Contracts/TrackerGateway.h"
#include "Features/WorklogHistory/WorklogHistoryStore.h"
#include "Shared/DisplayClock.h"
#include "Shared/Format.h"

WorklogDeletionStore::WorklogDeletionStore(TrackerGateway *gateway, WorklogHistoryStore *history, DisplayClock *clock,
                                           QObject *parent)
    : QObject(parent), gateway_(gateway), history_(history), clock_(clock) {}

bool WorklogDeletionStore::begin(const QString &worklogId) {
    const std::optional<WorklogItem> item = history_->worklog(worklogId);
    if (!item) {
        return false;
    }
    ++session_;
    original_ = item;
    active_ = true;
    dayLabel_ = Format::dayLabel(item->start.toLocalTime().date(), clock_->now().toLocalTime().date());
    startLabel_ = Format::clockTime(item->start);
    endLabel_ = item->end ? Format::clockTime(*item->end) : tr("now");
    durationSeconds_ = item->end ? qMax<qint64>(0, item->start.secsTo(*item->end)) : 0;
    setPending(false);
    // The core refuses to delete a running worklog, so the dialog says so without asking it.
    setError(item->isRunning() ? tr("A running worklog cannot be deleted. Stop the timer first.") : QString());
    emit begun();
    refreshCanDelete();
    return true;
}

void WorklogDeletionStore::cancel() {
    ++session_;
    active_ = false;
    refreshCanDelete();
}

void WorklogDeletionStore::confirm() {
    if (!canDelete() || !original_) {
        return;
    }
    const WorklogItem original = *original_;
    setError(QString());
    setPending(true);
    const int session = session_;
    QPointer<WorklogDeletionStore> self(this);
    gateway_->deleteWorklog(original.id, original.taskId, original.startText, original.endText,
                            [self, session](const BridgeReply &reply) {
        if (!self) {
            return;
        }
        if (session == self->session_) {
            self->finish(reply);
            if (reply.ok) {
                emit self->deleted();
            }
        }
        emit self->refreshRequested();
    });
}

void WorklogDeletionStore::refreshCanDelete() {
    const bool next = canDelete();
    if (next != canDelete_) {
        canDelete_ = next;
        emit canDeleteChanged();
    }
}

void WorklogDeletionStore::setPending(bool pending) {
    if (pending_ != pending) {
        pending_ = pending;
        emit pendingChanged();
        refreshCanDelete();
    }
}

void WorklogDeletionStore::setError(const QString &error) {
    if (error_ != error) {
        error_ = error;
        emit errorChanged();
    }
}

void WorklogDeletionStore::finish(const BridgeReply &reply) {
    setPending(false);
    if (!reply.ok) {
        setError(reply.userMessage(
            tr("The deletion could not be confirmed. Check the reloaded history before deleting again.")));
    }
}

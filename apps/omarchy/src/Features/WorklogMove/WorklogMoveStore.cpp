#include "Features/WorklogMove/WorklogMoveStore.h"

#include <QJsonArray>
#include <QJsonObject>
#include <QPointer>
#include <QVariantMap>

#include "Contracts/TrackerGateway.h"
#include "Features/WorklogHistory/WorklogHistoryStore.h"

WorklogMoveStore::WorklogMoveStore(TrackerGateway *gateway, WorklogHistoryStore *history, QObject *parent)
    : QObject(parent), gateway_(gateway), history_(history) {}

void WorklogMoveStore::setQuery(const QString &text) {
    if (!active_ || query_ == text) {
        return;
    }
    query_ = text;
    emit queryChanged();
    // The user is working on the search now, so an old move error is out of date.
    setError(QString());
    search();
}

int WorklogMoveStore::currentIndex() const {
    for (int index = 0; index < candidates_.size(); ++index) {
        if (candidates_.at(index).toMap().value(QStringLiteral("id")).toString() == selectedId_) {
            return index;
        }
    }
    return -1;
}

bool WorklogMoveStore::canMove() const {
    return active_ && fresh_ && !pending_ && currentIndex() >= 0;
}

bool WorklogMoveStore::begin(const QString &worklogId) {
    const std::optional<WorklogItem> item = history_->worklog(worklogId);
    if (!item) {
        return false;
    }
    ++session_;
    original_ = item;
    active_ = true;
    query_.clear();
    candidates_.clear();
    selectedId_.clear();
    setPending(false);
    setError(QString());
    emit begun();
    emit queryChanged();
    emit candidatesChanged();
    emit selectionChanged();
    search();
    return true;
}

void WorklogMoveStore::cancel() {
    ++session_;
    ++searchGeneration_;
    active_ = false;
    setSearchState(false, false);
    refreshCanMove();
}

void WorklogMoveStore::moveSelection(int step) {
    select(currentIndex() + step);
}

void WorklogMoveStore::select(int index) {
    if (candidates_.isEmpty()) {
        return;
    }
    const int bounded = qBound(0, index, static_cast<int>(candidates_.size()) - 1);
    setSelectedTaskId(candidates_.at(bounded).toMap().value(QStringLiteral("id")).toString());
}

void WorklogMoveStore::move() {
    if (!canMove() || !original_) {
        return;
    }
    const WorklogItem original = *original_;
    const QString destination = selectedId_;
    setError(QString());
    setPending(true);
    const int session = session_;
    QPointer<WorklogMoveStore> self(this);
    gateway_->moveWorklog(original.id, original.taskId, original.startText, original.endText, destination,
                          [self, session, destination](const BridgeReply &reply) {
        if (!self) {
            return;
        }
        if (session == self->session_) {
            self->finish(reply);
            if (reply.ok) {
                emit self->moved(destination);
            }
        }
        emit self->refreshRequested();
    });
}

void WorklogMoveStore::search() {
    if (!original_) {
        return;
    }
    const QString sourceTaskId = original_->taskId;
    const int generation = ++searchGeneration_;
    setSearchState(true, false);
    refreshCanMove();
    QPointer<WorklogMoveStore> self(this);
    gateway_->moveCandidates(sourceTaskId, query_, [self, generation](const BridgeReply &reply) {
        if (self && generation == self->searchGeneration_) {
            self->acceptCandidates(reply);
        }
    });
}

void WorklogMoveStore::acceptCandidates(const BridgeReply &reply) {
    if (!reply.ok || !reply.data.isArray()) {
        // The old results stay visible, but they no longer answer the query.
        setError(reply.ok ? tr("The task search could not be read") : reply.error);
        setSearchState(false, false);
        refreshCanMove();
        return;
    }
    QVariantList next;
    for (const auto &entry : reply.data.toArray()) {
        const QJsonObject object = entry.toObject();
        const QString id = object.value(QStringLiteral("id")).toString();
        if (!id.isEmpty()) {
            next.append(QVariantMap{{QStringLiteral("id"), id},
                                    {QStringLiteral("name"), object.value(QStringLiteral("name")).toString()}});
        }
    }
    candidates_ = next;
    emit candidatesChanged();
    // Keep the chosen task while it is still listed; otherwise the best match is chosen.
    const bool stillListed = currentIndex() >= 0;
    if (!stillListed) {
        setSelectedTaskId(next.isEmpty() ? QString() : next.first().toMap().value(QStringLiteral("id")).toString());
    }
    setSearchState(false, true);
    refreshCanMove();
}

void WorklogMoveStore::setSelectedTaskId(const QString &taskId) {
    if (selectedId_ != taskId) {
        selectedId_ = taskId;
        emit selectionChanged();
        refreshCanMove();
    }
}

void WorklogMoveStore::setSearchState(bool searching, bool fresh) {
    if (searching_ != searching || fresh_ != fresh) {
        searching_ = searching;
        fresh_ = fresh;
        emit searchStateChanged();
    }
}

void WorklogMoveStore::refreshCanMove() {
    const bool next = canMove();
    if (next != canMove_) {
        canMove_ = next;
        emit canMoveChanged();
    }
}

void WorklogMoveStore::setPending(bool pending) {
    if (pending_ != pending) {
        pending_ = pending;
        emit pendingChanged();
        refreshCanMove();
    }
}

void WorklogMoveStore::setError(const QString &error) {
    if (error_ != error) {
        error_ = error;
        emit errorChanged();
    }
}

void WorklogMoveStore::finish(const BridgeReply &reply) {
    setPending(false);
    if (!reply.ok) {
        setError(
            reply.userMessage(tr("The move could not be confirmed. Check the reloaded history before moving again.")));
    }
}

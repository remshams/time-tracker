#include "Features/Tracking/TrackingStore.h"

#include <QPointer>

#include "Contracts/TrackerGateway.h"
#include "Shared/DisplayClock.h"

TrackingStore::TrackingStore(TrackerGateway *gateway, DisplayClock *clock, QObject *parent)
    : QObject(parent), gateway_(gateway), clock_(clock) {
    connect(clock_, &DisplayClock::ticked, this, [this] {
        if (running_) {
            emit elapsedChanged();
        }
    });
}

void TrackingStore::accept(const std::optional<WorklogItem> &running) {
    const bool wasAvailable = available();
    const bool changed =
        !known_ || running_.has_value() != running.has_value() ||
        (running_ && running &&
         (running_->id != running->id || running_->start != running->start || running_->taskId != running->taskId));
    known_ = true;
    running_ = running;
    // The error shows with the reload that follows the failed command and goes with the next one.
    if (!error_.isEmpty() && ++readsSinceError_ > 1) {
        setError(QString());
    }
    if (changed) {
        emit stateChanged();
        emit elapsedChanged();
    }
    if (wasAvailable != available()) {
        emit availabilityChanged();
    }
}

void TrackingStore::setStale(bool stale) {
    if (stale_ == stale) {
        return;
    }
    const bool wasAvailable = available();
    stale_ = stale;
    emit staleChanged();
    if (wasAvailable != available()) {
        emit availabilityChanged();
    }
}

void TrackingStore::reset() {
    const bool wasAvailable = available();
    known_ = false;
    running_.reset();
    emit stateChanged();
    emit elapsedChanged();
    setError(QString());
    if (wasAvailable != available()) {
        emit availabilityChanged();
    }
}

qint64 TrackingStore::elapsedSeconds() const {
    return running_ ? qMax<qint64>(0, running_->start.secsTo(clock_->now())) : 0;
}

void TrackingStore::start(const QString &taskId) {
    if (!available() || taskId.isEmpty() || runningTaskId() == taskId) {
        return;
    }
    setError(QString());
    setPending(true);
    const QString expected = running_ ? running_->id : QString();
    QPointer<TrackingStore> self(this);
    gateway_->startTracking(taskId, expected, clock_->now(), [self](const BridgeReply &reply) {
        if (self) {
            self->finish(reply, tr("The tracking change could not be confirmed. Check the timer before trying again."));
        }
    });
}

void TrackingStore::stop() {
    if (!available() || !running_) {
        return;
    }
    // Read before the setters below, because their signals could change running_.
    const auto worklogId = running_->id;
    setError(QString());
    setPending(true);
    QPointer<TrackingStore> self(this);
    gateway_->stopTracking(worklogId, clock_->now(), [self](const BridgeReply &reply) {
        if (self) {
            self->finish(reply, tr("The stop could not be confirmed. Check the timer before trying again."));
        }
    });
}

void TrackingStore::toggle(const QString &taskId) {
    if (running_) {
        stop();
    } else {
        start(taskId);
    }
}

void TrackingStore::setPending(bool pending) {
    if (pending_ == pending) {
        return;
    }
    const bool wasAvailable = available();
    pending_ = pending;
    emit pendingChanged();
    if (wasAvailable != available()) {
        emit availabilityChanged();
    }
}

void TrackingStore::setError(const QString &error) {
    if (error_ == error) {
        return;
    }
    error_ = error;
    emit errorChanged();
}

void TrackingStore::finish(const BridgeReply &reply, const QString &unconfirmed) {
    setPending(false);
    if (!reply.ok) {
        setError(reply.userMessage(unconfirmed));
        readsSinceError_ = 0;
    }
    emit refreshRequested();
}

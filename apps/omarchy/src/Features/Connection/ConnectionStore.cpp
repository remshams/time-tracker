#include "Features/Connection/ConnectionStore.h"

#include <QPointer>

namespace {
const QString remoteKey = QStringLiteral("connection/remote");
const QString serverUrlKey = QStringLiteral("connection/serverUrl");
} // namespace

ConnectionStore::ConnectionStore(TrackerGateway *gateway, QSettings *settings, QObject *parent)
    : QObject(parent), gateway_(gateway), settings_(settings) {
    target_.remote = settings_->value(remoteKey, false).toBool();
    target_.serverUrl = settings_->value(serverUrlKey).toString();
    draft_ = target_;
}

void ConnectionStore::setDraftRemote(bool remote) {
    if (draft_.remote == remote) {
        return;
    }
    draft_.remote = remote;
    draftResult_.clear();
    emit draftChanged();
}

void ConnectionStore::setDraftServerUrl(const QString &url) {
    if (draft_.serverUrl == url) {
        return;
    }
    draft_.serverUrl = url;
    draftResult_.clear();
    emit draftChanged();
}

void ConnectionStore::connectSaved() {
    setStatus(QStringLiteral("connecting"), QString());
    QPointer<ConnectionStore> self(this);
    gateway_->connectTo(target_, [self](const BridgeReply &reply) {
        if (!self) {
            return;
        }
        if (!reply.ok) {
            self->setStatus(QStringLiteral("unavailable"), reply.error);
            return;
        }
        self->setStatus(QStringLiteral("connected"), QString());
        emit self->connected();
    });
}

void ConnectionStore::markReachable() {
    setStatus(QStringLiteral("connected"), QString());
}

void ConnectionStore::markUnavailable(const QString &message) {
    setStatus(QStringLiteral("unavailable"), message);
}

void ConnectionStore::resetDraft() {
    draft_ = target_;
    draftPending_ = false;
    draftResult_.clear();
    draftResultOk_ = false;
    emit draftChanged();
}

void ConnectionStore::testDraft() {
    if (draftPending_) {
        return;
    }
    draftPending_ = true;
    draftResult_.clear();
    emit draftChanged();
    QPointer<ConnectionStore> self(this);
    gateway_->testConnection(draft_, [self](const BridgeReply &reply) {
        if (self) {
            self->finishDraft(reply.ok, reply.ok ? tr("Connection works") : reply.error);
        }
    });
}

void ConnectionStore::applyDraft() {
    if (draftPending_) {
        return;
    }
    draftPending_ = true;
    draftResult_.clear();
    emit draftChanged();
    const ConnectionTarget candidate = draft_;
    QPointer<ConnectionStore> self(this);
    gateway_->connectTo(candidate, [self, candidate](const BridgeReply &reply) {
        if (!self) {
            return;
        }
        if (!reply.ok) {
            self->finishDraft(false, reply.error);
            return;
        }
        self->target_ = candidate;
        self->save();
        emit self->targetChanged();
        self->setStatus(QStringLiteral("connected"), QString());
        self->finishDraft(true, tr("Connected"));
        emit self->connected();
    });
}

void ConnectionStore::setStatus(const QString &status, const QString &message) {
    if (status_ == status && statusMessage_ == message) {
        return;
    }
    status_ = status;
    statusMessage_ = message;
    emit statusChanged();
}

void ConnectionStore::finishDraft(bool ok, const QString &message) {
    draftPending_ = false;
    draftResultOk_ = ok;
    draftResult_ = message;
    emit draftChanged();
}

void ConnectionStore::save() {
    settings_->setValue(remoteKey, target_.remote);
    settings_->setValue(serverUrlKey, target_.serverUrl);
    settings_->sync();
}

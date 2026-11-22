#include "Features/BulkTaskArchiving/BulkTaskArchivingStore.h"

#include <QJsonArray>
#include <QJsonObject>
#include <QPointer>
#include <QVariantMap>
#include <algorithm>

#include "Contracts/Models.h"
#include "Contracts/TrackerGateway.h"
#include "Shared/DisplayClock.h"
#include "Shared/TaskColor.h"

BulkTaskArchivingStore::BulkTaskArchivingStore(TrackerGateway *gateway, DisplayClock *clock, QObject *parent)
    : QObject(parent), gateway_(gateway), clock_(clock) {}

int BulkTaskArchivingStore::parseDays(const QString &text) {
    const QString trimmed = text.trimmed();
    // toInt would also accept signs and group separators.
    const bool digits = !trimmed.isEmpty() &&
                        std::all_of(trimmed.cbegin(), trimmed.cend(), [](QChar c) { return c >= '0' && c <= '9'; });
    bool ok = false;
    const int days = digits ? trimmed.toInt(&ok) : 0;
    return ok && days > 0 ? days : 0;
}

void BulkTaskArchivingStore::setDaysText(const QString &text) {
    // The period is fixed while a confirmation runs and after its result is shown.
    if (daysText_ == text || pending_ || archivedCount_ >= 0) {
        return;
    }
    daysText_ = text;
    emit daysTextChanged();
    const int days = parseDays(text);
    if (days == 0) {
        clearPreview();
        error_ = tr("Enter a whole number of days greater than zero.");
        emit stateChanged();
        return;
    }
    error_.clear();
    loadPreview(days);
}

void BulkTaskArchivingStore::begin() {
    if (pending_) {
        return;
    }
    archivedCount_ = -1;
    error_.clear();
    daysText_ = QString::number(kDefaultDays);
    emit daysTextChanged();
    loadPreview(kDefaultDays);
}

void BulkTaskArchivingStore::refresh() {
    const int days = parseDays(daysText_);
    if (days == 0 || pending_ || archivedCount_ >= 0) {
        return;
    }
    error_.clear();
    loadPreview(days);
}

void BulkTaskArchivingStore::cancel() {
    clearPreview();
    emit stateChanged();
}

void BulkTaskArchivingStore::clearPreview() {
    ++generation_;
    loading_ = false;
    previewLoaded_ = false;
    tasks_.clear();
}

void BulkTaskArchivingStore::loadPreview(int days) {
    clearPreview();
    loading_ = true;
    emit stateChanged();
    const quint64 generation = generation_;
    const QDateTime asOf = clock_->now();
    QPointer<BulkTaskArchivingStore> self(this);
    gateway_->previewInactiveTasks(days, asOf, [self, generation, days, asOf](const BridgeReply &reply) {
        if (self) {
            self->acceptPreview(generation, days, asOf, reply);
        }
    });
}

void BulkTaskArchivingStore::acceptPreview(quint64 generation, int days, const QDateTime &asOf,
                                           const BridgeReply &reply) {
    if (generation != generation_) {
        return;
    }
    loading_ = false;
    if (reply.ok) {
        for (const QJsonValue &value : reply.data.toObject().value(QStringLiteral("tasks")).toArray()) {
            if (const std::optional<TaskItem> item = TaskItem::fromJson(value)) {
                tasks_.append(QVariantMap{{QStringLiteral("id"), item->id},
                                          {QStringLiteral("name"), item->name},
                                          {QStringLiteral("colorName"), TaskColor::nameFor(item->id)}});
            }
        }
        previewDays_ = days;
        previewAsOf_ = asOf;
        previewLoaded_ = true;
    } else {
        // Loading a preview changes nothing, so its error needs no "could not be confirmed" wording.
        error_ = reply.error;
    }
    emit stateChanged();
}

void BulkTaskArchivingStore::submit() {
    if (!canSubmit()) {
        return;
    }
    error_.clear();
    pending_ = true;
    emit stateChanged();
    QPointer<BulkTaskArchivingStore> self(this);
    gateway_->archiveInactiveTasks(previewDays_, previewAsOf_, [self](const BridgeReply &reply) {
        if (self) {
            self->finish(reply);
        }
    });
}

void BulkTaskArchivingStore::finish(const BridgeReply &reply) {
    pending_ = false;
    // The bridge consumed the preview, whether the archive worked or not.
    previewLoaded_ = false;
    tasks_.clear();
    if (reply.ok) {
        archivedCount_ = reply.data.toObject().value(QStringLiteral("archivedCount")).toInt();
    } else {
        error_ =
            reply.userMessage(tr("The change could not be confirmed. Check the reloaded list before trying again."));
    }
    emit stateChanged();
    emit refreshRequested();
}

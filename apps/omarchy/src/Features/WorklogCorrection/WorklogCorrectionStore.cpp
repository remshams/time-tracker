#include "Features/WorklogCorrection/WorklogCorrectionStore.h"

#include <QPointer>
#include <QRegularExpression>
#include <QTimeZone>

#include "Contracts/TrackerGateway.h"
#include "Features/WorklogHistory/WorklogHistoryStore.h"
#include "Shared/DisplayClock.h"

namespace {

const QString dateFormat = QStringLiteral("yyyy-MM-dd");
const QString timeFormat = QStringLiteral("HH:mm");

// Local date and time fields as typed. Dates need four-digit years and two-digit months
// and days; hours may have one digit. Seconds are not part of the editor.
std::optional<QDateTime> localDateTime(const QString &dateText, const QString &timeText) {
    static const QRegularExpression datePattern(QStringLiteral("^(\\d{4})-(\\d{2})-(\\d{2})$"));
    static const QRegularExpression timePattern(QStringLiteral("^(\\d{1,2}):(\\d{2})$"));
    const QRegularExpressionMatch date = datePattern.match(dateText);
    const QRegularExpressionMatch time = timePattern.match(timeText);
    if (!date.hasMatch() || !time.hasMatch()) {
        return std::nullopt;
    }
    const QDate day(date.captured(1).toInt(), date.captured(2).toInt(), date.captured(3).toInt());
    const QTime clock(time.captured(1).toInt(), time.captured(2).toInt());
    if (!day.isValid() || !clock.isValid()) {
        return std::nullopt;
    }
    return QDateTime(day, clock, QTimeZone::LocalTime);
}

} // namespace

WorklogCorrectionStore::WorklogCorrectionStore(TrackerGateway *gateway, WorklogHistoryStore *history,
                                               DisplayClock *clock, QObject *parent)
    : QObject(parent), gateway_(gateway), history_(history), clock_(clock) {
    connect(clock_, &DisplayClock::ticked, this, [this] {
        // A running worklog's end is the current time, so its duration preview grows.
        if (active_ && running()) {
            emit previewChanged();
        }
    });
}

void WorklogCorrectionStore::setStartDate(const QString &text) {
    fieldEdited(startDate_, text);
}

void WorklogCorrectionStore::setStartTime(const QString &text) {
    fieldEdited(startTime_, text);
}

void WorklogCorrectionStore::setEndDate(const QString &text) {
    fieldEdited(endDate_, text);
}

void WorklogCorrectionStore::setEndTime(const QString &text) {
    fieldEdited(endTime_, text);
}

bool WorklogCorrectionStore::startChanged() const {
    return startDate_.trimmed() != initialStartDate_ || startTime_.trimmed() != initialStartTime_;
}

bool WorklogCorrectionStore::endChanged() const {
    return endDate_.trimmed() != initialEndDate_ || endTime_.trimmed() != initialEndTime_;
}

bool WorklogCorrectionStore::changed() const {
    return startChanged() || (!running() && endChanged());
}

std::optional<WorklogCorrectionStore::Replacement> WorklogCorrectionStore::replacement() const {
    if (!original_) {
        return std::nullopt;
    }
    Replacement result;
    // A field the user did not touch keeps the stored text, with its seconds and fraction.
    if (startChanged()) {
        const std::optional<QDateTime> start = localDateTime(startDate_.trimmed(), startTime_.trimmed());
        if (!start) {
            return std::nullopt;
        }
        result.start = start->toUTC();
        result.startText = Timestamps::format(result.start);
    } else {
        result.start = original_->start;
        result.startText = original_->startText;
    }
    if (original_->isRunning()) {
        return result;
    }
    if (endChanged()) {
        const std::optional<QDateTime> end = localDateTime(endDate_.trimmed(), endTime_.trimmed());
        if (!end) {
            return std::nullopt;
        }
        result.end = end->toUTC();
        result.endText = Timestamps::format(*result.end);
    } else {
        result.end = original_->end;
        result.endText = original_->endText;
    }
    return result;
}

qint64 WorklogCorrectionStore::durationSeconds() const {
    const std::optional<Replacement> draft = replacement();
    if (!draft) {
        return 0;
    }
    return draft->start.secsTo(draft->end.value_or(clock_->now()));
}

QString WorklogCorrectionStore::validationMessage() const {
    if (!original_ || replacement()) {
        return QString();
    }
    return tr("Enter the date as YYYY-MM-DD and the time as HH:MM, for example 2026-11-20 and 09:30.");
}

bool WorklogCorrectionStore::canSave() const {
    return active_ && !pending_ && changed() && replacement().has_value();
}

bool WorklogCorrectionStore::begin(const QString &worklogId) {
    const std::optional<WorklogItem> item = history_->worklog(worklogId);
    if (!item) {
        return false;
    }
    ++session_;
    original_ = item;
    active_ = true;
    const QDateTime start = item->start.toLocalTime();
    initialStartDate_ = startDate_ = start.toString(dateFormat);
    initialStartTime_ = startTime_ = start.toString(timeFormat);
    // A running worklog has no end fields.
    const QDateTime end = item->end.value_or(item->start).toLocalTime();
    initialEndDate_ = endDate_ = item->isRunning() ? QString() : end.toString(dateFormat);
    initialEndTime_ = endTime_ = item->isRunning() ? QString() : end.toString(timeFormat);
    timeZone_ = tr("%1 (UTC%2)").arg(start.toString(QStringLiteral("t")), start.toString(QStringLiteral("tt")));
    setPending(false);
    setError(QString());
    emit begun();
    refreshState();
    return true;
}

void WorklogCorrectionStore::cancel() {
    ++session_;
    active_ = false;
    refreshCanSave();
}

void WorklogCorrectionStore::save() {
    if (!canSave()) {
        return;
    }
    const std::optional<Replacement> draft = replacement();
    if (!original_ || !draft) {
        return;
    }
    // A copy of the original, because the setters below may change the member the analyzer would check.
    const WorklogItem original = *original_;
    const Replacement &values = *draft;
    setError(QString());
    setPending(true);
    refreshCanSave();
    const int session = session_;
    QPointer<WorklogCorrectionStore> self(this);
    gateway_->correctWorklog(original.id, original.startText, original.endText, values.startText, values.endText,
                             clock_->now(), [self, session](const BridgeReply &reply) {
        if (!self) {
            return;
        }
        if (session == self->session_) {
            self->finish(reply);
        }
        emit self->refreshRequested();
    });
}

void WorklogCorrectionStore::fieldEdited(QString &field, const QString &text) {
    if (field == text) {
        return;
    }
    field = text;
    refreshState();
}

void WorklogCorrectionStore::refreshState() {
    emit fieldsChanged();
    emit previewChanged();
    refreshCanSave();
}

void WorklogCorrectionStore::refreshCanSave() {
    const bool next = canSave();
    if (next != canSave_) {
        canSave_ = next;
        emit canSaveChanged();
    }
}

void WorklogCorrectionStore::setPending(bool pending) {
    if (pending_ != pending) {
        pending_ = pending;
        emit pendingChanged();
    }
}

void WorklogCorrectionStore::setError(const QString &error) {
    if (error_ != error) {
        error_ = error;
        emit errorChanged();
    }
}

void WorklogCorrectionStore::finish(const BridgeReply &reply) {
    setPending(false);
    if (reply.ok) {
        emit saved();
    } else {
        setError(reply.userMessage(
            tr("The change could not be confirmed. Check the reloaded history before trying again.")));
    }
    refreshCanSave();
}

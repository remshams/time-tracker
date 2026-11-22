#include "Features/WorklogHistory/WorklogHistoryStore.h"

#include <QPointer>
#include <QVariantMap>
#include <cmath>

#include "Contracts/TrackerGateway.h"
#include "Features/TaskCatalog/TaskCatalogStore.h"
#include "Shared/DisplayClock.h"
#include "Shared/Format.h"

namespace {

// Minutes since local midnight of `day`, clamped to that day.
double minuteOfDay(const QDateTime &value, const QDate &day) {
    const QDateTime local = value.toLocalTime();
    if (local.date() < day) {
        return 0;
    }
    if (local.date() > day) {
        return 24 * 60;
    }
    return local.time().msecsSinceStartOfDay() / 60000.0;
}

} // namespace

WorklogHistoryStore::WorklogHistoryStore(TrackerGateway *gateway, TaskCatalogStore *catalog, DisplayClock *clock,
                                         QObject *parent)
    : QObject(parent), gateway_(gateway), catalog_(catalog), clock_(clock) {
    connect(catalog_, &TaskCatalogStore::selectionChanged, this, [this] { selectTask(catalog_->selectedTaskId()); });
    connect(clock_, &DisplayClock::ticked, this, [this] {
        // The running row grows every second; "Today" becomes "Yesterday" at midnight.
        if (!worklogs_.isEmpty() && worklogs_.first().isRunning()) {
            emit daysChanged();
        }
    });
    connect(clock_, &DisplayClock::dayChanged, this, &WorklogHistoryStore::daysChanged);
}

QVariantList WorklogHistoryStore::days() const {
    const QDateTime now = clock_->now();
    const QDate today = now.toLocalTime().date();
    const double axisStart = axisStartHour() * 60.0;
    const double axisSpan = qMax(60.0, (axisEndHour() - axisStartHour()) * 60.0);
    QVariantList result;
    QVariantMap day;
    QVariantList entries;
    qint64 dayTotal = 0;
    QDate current;
    auto flush = [&] {
        if (!current.isValid()) {
            return;
        }
        day.insert(QStringLiteral("label"), Format::dayLabel(current, today));
        day.insert(QStringLiteral("totalSeconds"), dayTotal);
        day.insert(QStringLiteral("entries"), entries);
        result.append(day);
    };
    for (const WorklogItem &worklog : worklogs_) {
        const QDate date = worklog.start.toLocalTime().date();
        if (date != current) {
            flush();
            current = date;
            day = QVariantMap{{QStringLiteral("date"), date}};
            entries.clear();
            dayTotal = 0;
        }
        const QDateTime end = worklog.end.value_or(now);
        const qint64 seconds = qMax<qint64>(0, worklog.start.secsTo(end));
        dayTotal += seconds;
        const double from = (minuteOfDay(worklog.start, date) - axisStart) / axisSpan;
        const double to = (minuteOfDay(end, date) - axisStart) / axisSpan;
        entries.append(QVariantMap{
            {QStringLiteral("worklogId"), worklog.id},
            {QStringLiteral("running"), worklog.isRunning()},
            {QStringLiteral("startLabel"), Format::clockTime(worklog.start)},
            {QStringLiteral("endLabel"), worklog.isRunning() ? tr("now") : Format::clockTime(end)},
            {QStringLiteral("seconds"), seconds},
            {QStringLiteral("from"), qBound(0.0, from, 1.0)},
            {QStringLiteral("to"), qBound(0.0, to, 1.0)},
        });
    }
    flush();
    return result;
}

int WorklogHistoryStore::axisStartHour() const {
    if (worklogs_.isEmpty()) {
        return 8;
    }
    int earliest = 23;
    for (const WorklogItem &worklog : worklogs_) {
        earliest = qMin(earliest, worklog.start.toLocalTime().time().hour());
    }
    return earliest;
}

int WorklogHistoryStore::axisEndHour() const {
    if (worklogs_.isEmpty()) {
        return 18;
    }
    const QDateTime now = clock_->now();
    int latest = 1;
    for (const WorklogItem &worklog : worklogs_) {
        const QDate day = worklog.start.toLocalTime().date();
        const double minute = minuteOfDay(worklog.end.value_or(now), day);
        latest = qMax(latest, static_cast<int>(std::ceil(minute / 60.0)));
    }
    // Keep at least four hours so short days don't stretch one worklog across the row.
    return qMin(24, qMax(latest, axisStartHour() + 4));
}

std::optional<WorklogItem> WorklogHistoryStore::worklog(const QString &worklogId) const {
    for (const WorklogItem &item : worklogs_) {
        if (item.id == worklogId) {
            return item;
        }
    }
    return std::nullopt;
}

void WorklogHistoryStore::reload() {
    if (!taskId_.isEmpty()) {
        request(QString());
    }
}

void WorklogHistoryStore::loadMore() {
    if (!loading_ && hasMore()) {
        request(nextCursor_);
    }
}

void WorklogHistoryStore::clear() {
    ++generation_;
    taskId_.clear();
    worklogs_.clear();
    nextCursor_.clear();
    loaded_ = false;
    setLoading(false);
    setError(QString());
    emit rowsChanged();
    emit daysChanged();
}

void WorklogHistoryStore::selectTask(const QString &taskId) {
    if (taskId == taskId_) {
        return;
    }
    clear();
    taskId_ = taskId;
    emit rowsChanged();
    reload();
}

void WorklogHistoryStore::request(const QString &cursor) {
    const int generation = ++generation_;
    const QString taskId = taskId_;
    setLoading(true);
    QPointer<WorklogHistoryStore> self(this);
    gateway_->history(taskId, cursor, [self, generation, cursor](const BridgeReply &reply) {
        if (!self || generation != self->generation_) {
            return;
        }
        self->setLoading(false);
        const std::optional<HistoryPage> page = reply.ok ? HistoryPage::fromJson(reply.data) : std::nullopt;
        if (!page) {
            self->setError(reply.ok ? tr("The history could not be read") : reply.error);
            return;
        }
        self->setError(QString());
        if (cursor.isEmpty() || page->reset) {
            self->worklogs_ = page->worklogs;
        } else {
            self->worklogs_.append(page->worklogs);
        }
        self->nextCursor_ = page->nextCursor;
        self->loaded_ = true;
        emit self->rowsChanged();
        emit self->daysChanged();
    });
}

void WorklogHistoryStore::setLoading(bool loading) {
    if (loading_ != loading) {
        loading_ = loading;
        emit loadingChanged();
    }
}

void WorklogHistoryStore::setError(const QString &error) {
    if (error_ != error) {
        error_ = error;
        emit errorChanged();
    }
}

#include "Features/WorklogHistory/WorklogHistoryStore.h"

#include <QPointer>
#include <QVariantMap>
#include <algorithm>
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

// The smallest hour span the axis shows, so one short worklog doesn't stretch across the row.
constexpr int minimumAxisHours = 8;

} // namespace

WorklogHistoryStore::WorklogHistoryStore(TrackerGateway *gateway, TaskCatalogStore *catalog, DisplayClock *clock,
                                         QObject *parent)
    : QObject(parent), gateway_(gateway), catalog_(catalog), clock_(clock) {
    connect(catalog_, &TaskCatalogStore::selectionChanged, this, [this] { selectTask(catalog_->selectedTaskId()); });
    connect(clock_, &DisplayClock::ticked, this, [this] {
        // The running bar grows on the axis once a minute. Rebuilding the days every second
        // would recreate the rows and close their menus; the running row's time ticks in QML.
        const qint64 minute = clock_->now().toSecsSinceEpoch() / 60;
        if (minute != shownMinute_ && !worklogs_.isEmpty() && worklogs_.first().isRunning()) {
            shownMinute_ = minute;
            emit daysChanged();
        }
    });
    connect(clock_, &DisplayClock::dayChanged, this, &WorklogHistoryStore::daysChanged);
}

QList<WorklogHistoryStore::DaySegment> WorklogHistoryStore::segments() const {
    const QDateTime now = clock_->now();
    QList<DaySegment> result;
    for (const WorklogItem &worklog : worklogs_) {
        // Invalid data, an end before the start, shows as an empty worklog on its start day.
        const QDateTime end = std::max(worklog.start, worklog.end.value_or(now));
        // A worklog that runs past midnight shows on every local day it touches. The loop
        // walks back from the end, so the first part it keeps is the last one.
        bool last = true;
        for (QDate day = end.toLocalTime().date(); day >= worklog.start.toLocalTime().date(); day = day.addDays(-1)) {
            const QDateTime from = std::max(worklog.start, day.startOfDay());
            const QDateTime to = std::min(end, day.addDays(1).startOfDay());
            if (from < to || worklog.start == end) {
                result.append({&worklog, day, from, to, last});
                last = false;
            }
        }
    }
    // Newest day first; within a day, worklogs keep their newest-first order.
    std::stable_sort(result.begin(), result.end(),
                     [](const DaySegment &left, const DaySegment &right) { return left.day > right.day; });
    return result;
}

QVariantList WorklogHistoryStore::days() const {
    const QDate today = clock_->now().toLocalTime().date();
    const QList<DaySegment> parts = segments();
    const auto [startHour, endHour] = axisHours(parts);
    const double axisStart = startHour * 60.0;
    const double axisSpan = (endHour - startHour) * 60.0;
    QVariantList result;
    QVariantMap day;
    QVariantList entries;
    qint64 dayTotal = 0;
    // The running part of a day, which QML keeps current between rebuilds.
    qint64 runningSeconds = 0;
    qint64 runningOffset = 0;
    QDate current;
    auto flush = [&] {
        if (!current.isValid()) {
            return;
        }
        day.insert(QStringLiteral("label"), Format::dayLabel(current, today));
        day.insert(QStringLiteral("totalSeconds"), dayTotal);
        day.insert(QStringLiteral("runningSeconds"), runningSeconds);
        day.insert(QStringLiteral("runningOffset"), runningOffset);
        day.insert(QStringLiteral("entries"), entries);
        result.append(day);
    };
    for (const DaySegment &part : parts) {
        if (part.day != current) {
            flush();
            current = part.day;
            day = QVariantMap{{QStringLiteral("date"), part.day}};
            entries.clear();
            dayTotal = 0;
            runningSeconds = 0;
            runningOffset = 0;
        }
        const qint64 seconds = qMax<qint64>(0, part.from.secsTo(part.to));
        dayTotal += seconds;
        const bool running = part.worklog->isRunning() && part.last;
        // Seconds between the worklog's start and this part's start, so the running part's
        // length is the tracking timer minus this offset.
        const qint64 offset = part.worklog->start.secsTo(part.from);
        if (running) {
            runningSeconds = seconds;
            runningOffset = offset;
        }
        const bool endsAtMidnight = part.to == part.day.addDays(1).startOfDay();
        const QString endLabel = running          ? tr("now")
                                 : endsAtMidnight ? QStringLiteral("24:00")
                                                  : Format::clockTime(part.to);
        entries.append(QVariantMap{
            {QStringLiteral("worklogId"), part.worklog->id},
            {QStringLiteral("running"), running},
            {QStringLiteral("startLabel"), Format::clockTime(part.from)},
            {QStringLiteral("endLabel"), endLabel},
            {QStringLiteral("seconds"), seconds},
            {QStringLiteral("runningOffset"), offset},
            {QStringLiteral("from"), qBound(0.0, (minuteOfDay(part.from, part.day) - axisStart) / axisSpan, 1.0)},
            {QStringLiteral("to"), qBound(0.0, (minuteOfDay(part.to, part.day) - axisStart) / axisSpan, 1.0)},
        });
    }
    flush();
    return result;
}

std::pair<int, int> WorklogHistoryStore::axisHours(const QList<DaySegment> &parts) {
    if (parts.isEmpty()) {
        return {8, 18};
    }
    int earliest = 23;
    int latest = 1;
    for (const DaySegment &part : parts) {
        earliest = qMin(earliest, static_cast<int>(minuteOfDay(part.from, part.day)) / 60);
        latest = qMax(latest, static_cast<int>(std::ceil(minuteOfDay(part.to, part.day) / 60.0)));
    }
    // Widen short spans to the minimum, first to later hours, then to earlier ones.
    const int end = qMin(24, qMax(latest, earliest + minimumAxisHours));
    const int start = qMax(0, qMin(earliest, end - minimumAxisHours));
    return {start, end};
}

int WorklogHistoryStore::axisStartHour() const {
    return axisHours(segments()).first;
}

int WorklogHistoryStore::axisEndHour() const {
    return axisHours(segments()).second;
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
    // After clear() the store has no task, but the catalog may still select one, for
    // example after reconnecting to a source with the same tasks.
    if (taskId_.isEmpty()) {
        taskId_ = catalog_->selectedTaskId();
        emit rowsChanged();
    }
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

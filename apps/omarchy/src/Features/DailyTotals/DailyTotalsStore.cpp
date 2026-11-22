#include "Features/DailyTotals/DailyTotalsStore.h"

#include <QVariantMap>
#include <algorithm>

#include "Features/TaskCatalog/TaskCatalogStore.h"
#include "Features/Tracking/TrackingStore.h"
#include "Shared/DisplayClock.h"
#include "Shared/TaskColor.h"

DailyTotalsStore::DailyTotalsStore(TaskCatalogStore *catalog, TrackingStore *tracking, DisplayClock *clock,
                                   QObject *parent)
    : QObject(parent), catalog_(catalog), tracking_(tracking), clock_(clock) {
    connect(clock_, &DisplayClock::ticked, this, [this] {
        if (report_ && tracking_->running()) {
            emit totalsChanged();
        }
    });
    // Yesterday's report no longer covers today; the next refresh brings a new one.
    connect(clock_, &DisplayClock::dayChanged, this, &DailyTotalsStore::reset);
    connect(tracking_, &TrackingStore::stateChanged, this, &DailyTotalsStore::totalsChanged);
    connect(catalog_, &TaskCatalogStore::tasksChanged, this, &DailyTotalsStore::totalsChanged);
}

void DailyTotalsStore::accept(const std::optional<DailyReport> &report) {
    report_ = report;
    emit totalsChanged();
}

void DailyTotalsStore::reset() {
    if (!report_) {
        return;
    }
    report_.reset();
    emit totalsChanged();
}

QHash<QString, qint64> DailyTotalsStore::seconds() const {
    QHash<QString, qint64> result;
    if (!report_) {
        return result;
    }
    for (auto it = report_->microseconds.cbegin(); it != report_->microseconds.cend(); ++it) {
        result.insert(it.key(), it.value() / 1000000);
    }
    // The report counted the running worklog up to its cutoff; add the time since then.
    if (const std::optional<WorklogItem> &running = tracking_->runningWorklog()) {
        const QDateTime from = std::max(report_->cutoff, running->start);
        const QDateTime dayStart = clock_->dayStart();
        const QDateTime counted = std::max(from, dayStart);
        const qint64 extra = qMax<qint64>(0, counted.secsTo(clock_->now()));
        result[running->taskId] += extra;
    }
    return result;
}

qint64 DailyTotalsStore::totalSeconds() const {
    qint64 total = 0;
    for (const qint64 value : seconds()) {
        total += value;
    }
    return total;
}

QList<QPair<QString, qint64>> DailyTotalsStore::ranked() const {
    QList<QPair<QString, qint64>> rows;
    const QHash<QString, qint64> values = seconds();
    for (const TaskItem &task : catalog_->tasks()) {
        const qint64 value = values.value(task.id);
        if (value > 0) {
            rows.append({task.id, value});
        }
    }
    std::stable_sort(rows.begin(), rows.end(),
                     [](const auto &left, const auto &right) { return left.second > right.second; });
    return rows;
}

QVariantList DailyTotalsStore::shares() const {
    const QList<QPair<QString, qint64>> rows = ranked();
    qint64 total = 0;
    for (const auto &row : rows) {
        total += row.second;
    }
    QVariantList result;
    for (const auto &row : rows) {
        result.append(QVariantMap{{QStringLiteral("taskId"), row.first},
                                  {QStringLiteral("colorName"), TaskColor::nameFor(row.first)},
                                  {QStringLiteral("seconds"), row.second},
                                  {QStringLiteral("fraction"), static_cast<double>(row.second) / total}});
    }
    return result;
}

QString DailyTotalsStore::leaderTaskId() const {
    const QList<QPair<QString, qint64>> rows = ranked();
    return rows.isEmpty() ? QString() : rows.first().first;
}

int DailyTotalsStore::leaderPercent() const {
    const QList<QPair<QString, qint64>> rows = ranked();
    qint64 total = 0;
    for (const auto &row : rows) {
        total += row.second;
    }
    return total == 0 ? 0 : static_cast<int>((rows.first().second * 100 + total / 2) / total);
}

qint64 DailyTotalsStore::secondsFor(const QString &taskId) const {
    return report_ ? seconds().value(taskId) : -1;
}

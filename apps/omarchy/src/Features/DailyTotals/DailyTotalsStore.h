#pragma once

#include <QHash>
#include <QObject>
#include <QVariantList>
#include <optional>

#include "Contracts/Models.h"

class DisplayClock;
class TaskCatalogStore;
class TrackingStore;

// Today's totals per task and for the day. The report counts up to its cutoff; the
// running worklog keeps adding time on every clock tick until the next report.
class DailyTotalsStore : public QObject {
    Q_OBJECT
    Q_PROPERTY(bool known READ known NOTIFY totalsChanged)
    Q_PROPERTY(qint64 totalSeconds READ totalSeconds NOTIFY totalsChanged)
    Q_PROPERTY(QVariantList shares READ shares NOTIFY totalsChanged)
    Q_PROPERTY(QString leaderTaskId READ leaderTaskId NOTIFY totalsChanged)
    Q_PROPERTY(int leaderPercent READ leaderPercent NOTIFY totalsChanged)
public:
    DailyTotalsStore(TaskCatalogStore *catalog, TrackingStore *tracking, DisplayClock *clock,
                     QObject *parent = nullptr);

    // Takes in the report from a refresh. No report leaves the totals unknown.
    void accept(const std::optional<DailyReport> &report);
    void reset();

    bool known() const { return report_.has_value(); }
    qint64 totalSeconds() const;
    // [{taskId, colorName, seconds, fraction}] for tasks with time today, largest first.
    QVariantList shares() const;
    QString leaderTaskId() const;
    int leaderPercent() const;

    // The task's seconds today, or -1 while no report covers today.
    Q_INVOKABLE qint64 secondsFor(const QString &taskId) const;
    QHash<QString, qint64> seconds() const;

signals:
    void totalsChanged();

private:
    QList<QPair<QString, qint64>> ranked() const;

    TaskCatalogStore *catalog_;
    TrackingStore *tracking_;
    DisplayClock *clock_;
    std::optional<DailyReport> report_;
};

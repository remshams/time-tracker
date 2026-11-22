#pragma once

#include <QObject>
#include <QVariantList>
#include <optional>

#include "Contracts/Models.h"

class DisplayClock;
class TaskCatalogStore;
class TrackerGateway;

// The selected task's worklogs, newest first, loaded one page of up to 50 at a time and
// grouped by local day for the timeline. A task change drops the previous task's rows;
// a reload of the same task keeps the shown rows until the new page arrives.
class WorklogHistoryStore : public QObject {
    Q_OBJECT
    Q_PROPERTY(bool loading READ loading NOTIFY loadingChanged)
    Q_PROPERTY(bool loaded READ loaded NOTIFY rowsChanged)
    Q_PROPERTY(QString error READ error NOTIFY errorChanged)
    Q_PROPERTY(bool hasMore READ hasMore NOTIFY rowsChanged)
    Q_PROPERTY(int count READ count NOTIFY rowsChanged)
    Q_PROPERTY(QVariantList days READ days NOTIFY daysChanged)
    Q_PROPERTY(int axisStartHour READ axisStartHour NOTIFY daysChanged)
    Q_PROPERTY(int axisEndHour READ axisEndHour NOTIFY daysChanged)
public:
    WorklogHistoryStore(TrackerGateway *gateway, TaskCatalogStore *catalog, DisplayClock *clock,
                        QObject *parent = nullptr);

    QString taskId() const { return taskId_; }
    bool loading() const { return loading_; }
    bool loaded() const { return loaded_; }
    QString error() const { return error_; }
    bool hasMore() const { return !nextCursor_.isEmpty(); }
    int count() const { return static_cast<int>(worklogs_.size()); }
    QVariantList days() const;
    int axisStartHour() const;
    int axisEndHour() const;

    const QList<WorklogItem> &worklogs() const { return worklogs_; }
    std::optional<WorklogItem> worklog(const QString &worklogId) const;

    // Loads the first page for the selected task again.
    void reload();
    Q_INVOKABLE void loadMore();
    void clear();

signals:
    void rowsChanged();
    void loadingChanged();
    void errorChanged();
    void daysChanged();

private:
    // The part of one worklog that falls on one local day.
    struct DaySegment {
        const WorklogItem *worklog;
        QDate day;
        QDateTime from;
        QDateTime to;
        bool last; // The part on the day the worklog ends, or runs now.
    };
    QList<DaySegment> segments() const;
    static std::pair<int, int> axisHours(const QList<DaySegment> &parts);

    void selectTask(const QString &taskId);
    void request(const QString &cursor);
    void setLoading(bool loading);
    void setError(const QString &error);

    TrackerGateway *gateway_;
    TaskCatalogStore *catalog_;
    DisplayClock *clock_;
    QString taskId_;
    QList<WorklogItem> worklogs_;
    QString nextCursor_;
    bool loading_ = false;
    bool loaded_ = false;
    QString error_;
    // Each request takes a new generation; answers for an older one are dropped.
    int generation_ = 0;
    qint64 shownMinute_ = -1;
};

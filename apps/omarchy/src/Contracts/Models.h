#pragma once

#include <QDateTime>
#include <QHash>
#include <QJsonValue>
#include <QList>
#include <QString>
#include <optional>

// Wire models from the bridge. Worklogs keep the timestamp text the bridge sent, because
// guarded writes must send back exactly the values the user reviewed.

struct TaskItem {
    QString id;
    QString name;
    bool archived = false;
    std::optional<QDateTime> latestStart;

    static std::optional<TaskItem> fromJson(const QJsonValue &value);
};

struct WorklogItem {
    QString id;
    QString taskId;
    QString startText;
    QString endText; // Empty while the worklog runs.
    QDateTime start;
    std::optional<QDateTime> end;

    bool isRunning() const { return !end.has_value(); }
    static std::optional<WorklogItem> fromJson(const QJsonValue &value);
};

// Today's report: microseconds per task up to `cutoff`.
struct DailyReport {
    QHash<QString, qint64> microseconds;
    QDateTime cutoff;

    static std::optional<DailyReport> fromJson(const QJsonValue &value);
};

// One refresh: the task list, the tracking state and today's report. A missing part
// means the bridge has no confirmed observation for it.
struct Overview {
    std::optional<QList<TaskItem>> tasks;
    bool trackingKnown = false;
    std::optional<WorklogItem> running;
    std::optional<DailyReport> report;

    static Overview fromJson(const QJsonValue &value);
};

struct HistoryPage {
    QList<WorklogItem> worklogs;
    QString nextCursor; // Empty on the last page.
    bool reset = false; // The history changed; this page starts over.

    static std::optional<HistoryPage> fromJson(const QJsonValue &value);
};

namespace Timestamps {
// RFC 3339 in UTC with milliseconds, which the bridge accepts for every timestamp argument.
QString format(const QDateTime &value);
std::optional<QDateTime> parse(const QString &text);
} // namespace Timestamps

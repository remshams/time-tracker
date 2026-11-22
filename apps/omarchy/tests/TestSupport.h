#pragma once

#include <QDateTime>
#include <QJsonArray>
#include <QJsonObject>
#include <QString>

#include "Shared/DisplayClock.h"

// Helpers the test files share.
namespace TestSupport {

// A UTC time from "2026-11-10T12:00:00Z" style text.
inline QDateTime utc(const QString &iso) {
    return QDateTime::fromString(iso, Qt::ISODateWithMs).toUTC();
}

inline QJsonObject taskJson(const QString &id, const QString &name, bool archived = false,
                            const QString &latestStart = QString()) {
    QJsonObject task{
        {QStringLiteral("id"), id}, {QStringLiteral("name"), name}, {QStringLiteral("archived"), archived}};
    if (!latestStart.isEmpty()) {
        task.insert(QStringLiteral("latestStart"), latestStart);
    }
    return task;
}

inline QJsonObject worklogJson(const QString &id, const QString &taskId, const QString &start,
                               const QString &end = QString()) {
    QJsonObject worklog{
        {QStringLiteral("id"), id}, {QStringLiteral("taskId"), taskId}, {QStringLiteral("start"), start}};
    if (!end.isEmpty()) {
        worklog.insert(QStringLiteral("end"), end);
    }
    return worklog;
}

inline QJsonObject historyJson(const QJsonArray &worklogs, const QString &nextCursor = QString(), bool reset = false) {
    return {{QStringLiteral("worklogs"), worklogs},
            {QStringLiteral("nextCursor"), nextCursor},
            {QStringLiteral("reset"), reset}};
}

// The {tasks, tracking, report} object a refresh answers with. A null `running` means
// tracking is known to be idle; leave a part out of the result by passing `false` for it.
inline QJsonObject overviewJson(const QJsonArray &tasks, const QJsonValue &running, const QString &reportNow,
                                const QJsonObject &microsecondsByTask = {}) {
    QJsonObject overview{{QStringLiteral("tasks"), QJsonObject{{QStringLiteral("value"), tasks}}},
                         {QStringLiteral("tracking"), QJsonObject{{QStringLiteral("value"), running}}}};
    if (!reportNow.isEmpty()) {
        QJsonArray rows;
        for (auto it = microsecondsByTask.begin(); it != microsecondsByTask.end(); ++it) {
            rows.append(QJsonObject{{QStringLiteral("taskId"), it.key()},
                                    {QStringLiteral("durationMicroseconds"), it.value()}});
        }
        overview.insert(QStringLiteral("report"),
                        QJsonObject{{QStringLiteral("now"), reportNow}, {QStringLiteral("rows"), rows}});
    }
    return overview;
}

// A display clock that shows the time a test sets. It never ticks by itself.
class TestClock {
public:
    explicit TestClock(const QString &iso = QStringLiteral("2026-11-10T12:00:00.000Z"))
        : time_(utc(iso)), clock_([this] { return time_; }, false) {}

    DisplayClock *clock() { return &clock_; }
    // Moves the time and lets the clock notice, like one tick of the real clock.
    void advanceTo(const QString &iso) {
        time_ = utc(iso);
        clock_.tick();
    }
    // Moves the time without a tick, for stores that read the clock only on demand.
    void set(const QString &iso) { time_ = utc(iso); }

private:
    QDateTime time_;
    DisplayClock clock_;
};

} // namespace TestSupport

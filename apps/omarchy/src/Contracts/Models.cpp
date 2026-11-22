#include "Contracts/Models.h"

#include <QJsonArray>
#include <QJsonObject>

namespace Timestamps {

QString format(const QDateTime &value) {
    return value.toUTC().toString(Qt::ISODateWithMs);
}

std::optional<QDateTime> parse(const QString &text) {
    QDateTime value = QDateTime::fromString(text, Qt::ISODateWithMs);
    if (!value.isValid()) {
        return std::nullopt;
    }
    return value.toUTC();
}

} // namespace Timestamps

std::optional<TaskItem> TaskItem::fromJson(const QJsonValue &value) {
    const QJsonObject object = value.toObject();
    const QString id = object.value(QStringLiteral("id")).toString();
    if (id.isEmpty()) {
        return std::nullopt;
    }
    TaskItem task;
    task.id = id;
    task.name = object.value(QStringLiteral("name")).toString();
    task.archived = object.value(QStringLiteral("archived")).toBool();
    const QJsonValue latest = object.value(QStringLiteral("latestStart"));
    if (latest.isString()) {
        task.latestStart = Timestamps::parse(latest.toString());
    }
    return task;
}

std::optional<WorklogItem> WorklogItem::fromJson(const QJsonValue &value) {
    const QJsonObject object = value.toObject();
    WorklogItem worklog;
    worklog.id = object.value(QStringLiteral("id")).toString();
    worklog.taskId = object.value(QStringLiteral("taskId")).toString();
    worklog.startText = object.value(QStringLiteral("start")).toString();
    const std::optional<QDateTime> start = Timestamps::parse(worklog.startText);
    if (worklog.id.isEmpty() || worklog.taskId.isEmpty() || !start) {
        return std::nullopt;
    }
    worklog.start = *start;
    const QJsonValue end = object.value(QStringLiteral("end"));
    if (end.isString()) {
        worklog.endText = end.toString();
        worklog.end = Timestamps::parse(worklog.endText);
        if (!worklog.end) {
            return std::nullopt;
        }
    }
    return worklog;
}

std::optional<DailyReport> DailyReport::fromJson(const QJsonValue &value) {
    const QJsonObject object = value.toObject();
    const std::optional<QDateTime> cutoff = Timestamps::parse(object.value(QStringLiteral("now")).toString());
    if (!cutoff) {
        return std::nullopt;
    }
    DailyReport report;
    report.cutoff = *cutoff;
    const QJsonArray rows = object.value(QStringLiteral("rows")).toArray();
    for (const QJsonValue &row : rows) {
        const QJsonObject item = row.toObject();
        report.microseconds.insert(item.value(QStringLiteral("taskId")).toString(),
                                   static_cast<qint64>(item.value(QStringLiteral("durationMicroseconds")).toDouble()));
    }
    return report;
}

Overview Overview::fromJson(const QJsonValue &value) {
    const QJsonObject object = value.toObject();
    Overview overview;
    const QJsonValue tasks = object.value(QStringLiteral("tasks"));
    if (tasks.isObject()) {
        QList<TaskItem> items;
        for (const QJsonValue &task : tasks.toObject().value(QStringLiteral("value")).toArray()) {
            if (std::optional<TaskItem> item = TaskItem::fromJson(task)) {
                items.append(*item);
            }
        }
        overview.tasks = items;
    }
    const QJsonValue tracking = object.value(QStringLiteral("tracking"));
    if (tracking.isObject()) {
        overview.trackingKnown = true;
        overview.running = WorklogItem::fromJson(tracking.toObject().value(QStringLiteral("value")));
    }
    const QJsonValue report = object.value(QStringLiteral("report"));
    if (report.isObject()) {
        overview.report = DailyReport::fromJson(report);
    }
    return overview;
}

std::optional<HistoryPage> HistoryPage::fromJson(const QJsonValue &value) {
    if (!value.isObject()) {
        return std::nullopt;
    }
    const QJsonObject object = value.toObject();
    HistoryPage page;
    for (const QJsonValue &item : object.value(QStringLiteral("worklogs")).toArray()) {
        if (std::optional<WorklogItem> worklog = WorklogItem::fromJson(item)) {
            page.worklogs.append(*worklog);
        }
    }
    page.nextCursor = object.value(QStringLiteral("nextCursor")).toString();
    page.reset = object.value(QStringLiteral("reset")).toBool();
    return page;
}

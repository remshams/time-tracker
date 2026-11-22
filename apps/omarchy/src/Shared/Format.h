#pragma once

#include <QDateTime>
#include <QObject>
#include <QString>
#include <optional>

// Text formats QML and the stores share.
class Format : public QObject {
    Q_OBJECT
public:
    using QObject::QObject;

    // "1:12:00" for 4320 seconds. Negative input counts as zero.
    Q_INVOKABLE static QString duration(qint64 seconds);
    // "1:12" without the seconds, which the UI shows dimmed after it.
    Q_INVOKABLE static QString hoursMinutes(qint64 seconds);
    // ":00", the seconds part that follows hoursMinutes().
    Q_INVOKABLE static QString secondsSuffix(qint64 seconds);
    // "15:30" in local time.
    Q_INVOKABLE static QString clockTime(const QDateTime &value);
    // "Today", "Yesterday", or "Wed 11 Nov" relative to `today`.
    static QString dayLabel(const QDate &day, const QDate &today);
    // When a task last ran: "Today 13:12", "Yesterday", "3 days ago" or "Never".
    static QString lastWorked(const std::optional<QDateTime> &latestStart, const QDate &today);
};

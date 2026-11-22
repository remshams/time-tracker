#include "Shared/Format.h"

#include <QLocale>

QString Format::duration(qint64 seconds) {
    return hoursMinutes(seconds) + secondsSuffix(seconds);
}

QString Format::hoursMinutes(qint64 seconds) {
    const qint64 value = qMax<qint64>(0, seconds);
    return QStringLiteral("%1:%2").arg(value / 3600).arg((value / 60) % 60, 2, 10, QLatin1Char('0'));
}

QString Format::secondsSuffix(qint64 seconds) {
    return QStringLiteral(":%1").arg(qMax<qint64>(0, seconds) % 60, 2, 10, QLatin1Char('0'));
}

QString Format::clockTime(const QDateTime &value) {
    return value.toLocalTime().toString(QStringLiteral("HH:mm"));
}

QString Format::dayLabel(const QDate &day, const QDate &today) {
    if (day == today) {
        return tr("Today");
    }
    if (day == today.addDays(-1)) {
        return tr("Yesterday");
    }
    const QString format = day.year() == today.year() ? QStringLiteral("ddd d MMM") : QStringLiteral("ddd d MMM yyyy");
    return QLocale().toString(day, format);
}

QString Format::lastWorked(const std::optional<QDateTime> &latestStart, const QDate &today) {
    if (!latestStart) {
        return tr("Never");
    }
    const QDateTime local = latestStart->toLocalTime();
    const qint64 days = local.date().daysTo(today);
    if (days <= 0) {
        return tr("Today %1").arg(clockTime(local));
    }
    if (days == 1) {
        return tr("Yesterday");
    }
    return tr("%n days ago", "", static_cast<int>(days));
}

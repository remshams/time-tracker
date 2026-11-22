#include "Shared/DisplayClock.h"

DisplayClock::DisplayClock(Source source, bool ticking, QObject *parent)
    : QObject(parent), source_(source ? std::move(source) : Source([] { return QDateTime::currentDateTime(); })) {
    day_ = now().date();
    timer_.setInterval(1000);
    timer_.setTimerType(Qt::CoarseTimer);
    connect(&timer_, &QTimer::timeout, this, &DisplayClock::tick);
    if (ticking) {
        timer_.start();
    }
}

QDateTime DisplayClock::now() const {
    return source_();
}

QDateTime DisplayClock::dayStart() const {
    return now().toLocalTime().date().startOfDay();
}

QDateTime DisplayClock::dayEnd() const {
    return now().toLocalTime().date().addDays(1).startOfDay();
}

void DisplayClock::tick() {
    const QDate today = now().toLocalTime().date();
    emit ticked();
    if (today != day_) {
        day_ = today;
        emit dayChanged();
    }
}

#pragma once

#include <QDateTime>
#include <QObject>
#include <QTimer>
#include <functional>

// The app's single source of the current time. Ticks once a second so running timers
// redraw together. Tests pass their own time source and call tick() by hand.
class DisplayClock : public QObject {
    Q_OBJECT
public:
    using Source = std::function<QDateTime()>;

    explicit DisplayClock(Source source = {}, bool ticking = true, QObject *parent = nullptr);

    QDateTime now() const;
    // Local midnight at the start of the day that contains `now()`, and the next one.
    QDateTime dayStart() const;
    QDateTime dayEnd() const;

    void tick();

signals:
    void ticked();
    // The local date changed since the last tick.
    void dayChanged();

private:
    Source source_;
    QTimer timer_;
    QDate day_;
};

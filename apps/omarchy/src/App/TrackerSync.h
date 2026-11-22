#pragma once

#include <QObject>
#include <QTimer>

struct BridgeReply;
class ConnectionStore;
class DailyTotalsStore;
class DisplayClock;
class TaskCatalogStore;
class TrackerGateway;
class TrackingStore;
class WorklogHistoryStore;

// Decides when the app reads: after connecting, after every write, at midnight and on a
// 30-second poll so changes from other clients show up. One read runs at a time; a
// request during a read runs once more when it ends.
class TrackerSync : public QObject {
    Q_OBJECT
public:
    struct Stores {
        ConnectionStore *connection;
        TaskCatalogStore *catalog;
        TrackingStore *tracking;
        DailyTotalsStore *totals;
        WorklogHistoryStore *history;
    };

    TrackerSync(TrackerGateway *gateway, DisplayClock *clock, Stores stores, int pollMilliseconds = 30000,
                QObject *parent = nullptr);

    // Opens the saved connection and starts polling.
    void start();
    // Reads the overview. `reloadHistory` also reloads the selected task's history, which
    // every write needs.
    Q_INVOKABLE void refresh(bool reloadHistory = false);

signals:
    void refreshed();

private:
    void poll();
    void apply(const BridgeReply &reply, bool reloadHistory);

    TrackerGateway *gateway_;
    DisplayClock *clock_;
    Stores stores_;
    QTimer pollTimer_;
    bool inFlight_ = false;
    bool again_ = false;
    bool againReloadsHistory_ = false;
};

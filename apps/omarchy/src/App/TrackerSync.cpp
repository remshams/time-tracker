#include "App/TrackerSync.h"

#include <QPointer>

#include "Contracts/Models.h"
#include "Contracts/TrackerGateway.h"
#include "Features/Connection/ConnectionStore.h"
#include "Features/DailyTotals/DailyTotalsStore.h"
#include "Features/TaskCatalog/TaskCatalogStore.h"
#include "Features/Tracking/TrackingStore.h"
#include "Features/WorklogHistory/WorklogHistoryStore.h"
#include "Shared/DisplayClock.h"

TrackerSync::TrackerSync(TrackerGateway *gateway, DisplayClock *clock, Stores stores, int pollMilliseconds,
                         QObject *parent)
    : QObject(parent), gateway_(gateway), clock_(clock), stores_(stores) {
    pollTimer_.setInterval(pollMilliseconds);
    connect(&pollTimer_, &QTimer::timeout, this, &TrackerSync::poll);
    connect(clock_, &DisplayClock::dayChanged, this, [this] { refresh(true); });
    connect(stores_.tracking, &TrackingStore::refreshRequested, this, [this] { refresh(true); });
    connect(stores_.connection, &ConnectionStore::connected, this, [this] {
        // A new data source shares nothing with the previous one.
        stores_.tracking->reset();
        stores_.totals->reset();
        stores_.history->clear();
        refresh(true);
    });
}

void TrackerSync::start() {
    pollTimer_.start();
    stores_.connection->connectSaved();
}

void TrackerSync::poll() {
    // Without a working connection, try to open it again instead of reading.
    if (stores_.connection->status() == QStringLiteral("unavailable") && !stores_.tracking->known()) {
        stores_.connection->connectSaved();
        return;
    }
    refresh(false);
}

void TrackerSync::refresh(bool reloadHistory) {
    if (inFlight_) {
        again_ = true;
        againReloadsHistory_ = againReloadsHistory_ || reloadHistory;
        return;
    }
    inFlight_ = true;
    QPointer<TrackerSync> self(this);
    gateway_->refreshOverview(clock_->dayStart(), clock_->dayEnd(), clock_->now(),
                              [self, reloadHistory](const BridgeReply &reply) {
        if (self) {
            self->apply(reply, reloadHistory);
        }
    });
}

void TrackerSync::apply(const BridgeReply &reply, bool reloadHistory) {
    inFlight_ = false;
    if (reply.ok) {
        const Overview overview = Overview::fromJson(reply.data);
        const QString selectedBefore = stores_.catalog->selectedTaskId();
        if (overview.tasks) {
            stores_.catalog->accept(*overview.tasks);
        }
        if (overview.trackingKnown) {
            stores_.tracking->accept(overview.running);
        }
        stores_.totals->accept(overview.report);
        stores_.tracking->setStale(false);
        stores_.connection->markReachable();
        // A changed selection loads its own history.
        if (reloadHistory && stores_.catalog->selectedTaskId() == selectedBefore) {
            stores_.history->reload();
        }
    } else {
        stores_.tracking->setStale(true);
        stores_.connection->markUnavailable(reply.error);
    }
    if (again_) {
        const bool history = againReloadsHistory_;
        again_ = false;
        againReloadsHistory_ = false;
        refresh(history);
    }
}

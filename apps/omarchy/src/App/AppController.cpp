#include "App/AppController.h"

#include <QQmlContext>

#include "App/TrackerSync.h"
#include "Features/Connection/ConnectionStore.h"
#include "Features/DailyTotals/DailyTotalsStore.h"
#include "Features/TaskCatalog/TaskCatalogStore.h"
#include "Features/Tracking/TrackingStore.h"
#include "Features/WorklogHistory/WorklogHistoryStore.h"
#include "Shared/DisplayClock.h"
#include "Shared/Format.h"
#include "Shared/Theme.h"

AppController::AppController(TrackerGateway *gateway, Theme *theme, QObject *parent)
    : QObject(parent), gateway_(gateway), theme_(theme),
      settings_(QSettings::IniFormat, QSettings::UserScope, QStringLiteral("tt-desktop"), QStringLiteral("tt-desktop")),
      clock_(new DisplayClock({}, true, this)), format_(new Format(this)),
      connection_(new ConnectionStore(gateway_, &settings_, this)), catalog_(new TaskCatalogStore(clock_, this)),
      tracking_(new TrackingStore(gateway_, clock_, this)),
      totals_(new DailyTotalsStore(catalog_, tracking_, clock_, this)),
      history_(new WorklogHistoryStore(gateway_, catalog_, clock_, this)),
      sync_(new TrackerSync(gateway_, clock_, {connection_, catalog_, tracking_, totals_, history_}, 30000, this)) {}

AppController::~AppController() = default;

void AppController::expose(QQmlContext *context) {
    context->setContextProperty(QStringLiteral("theme"), theme_);
    context->setContextProperty(QStringLiteral("clock"), clock_);
    context->setContextProperty(QStringLiteral("format"), format_);
    context->setContextProperty(QStringLiteral("connection"), connection_);
    context->setContextProperty(QStringLiteral("catalog"), catalog_);
    context->setContextProperty(QStringLiteral("tracking"), tracking_);
    context->setContextProperty(QStringLiteral("totals"), totals_);
    context->setContextProperty(QStringLiteral("history"), history_);
    context->setContextProperty(QStringLiteral("sync"), sync_);
}

void AppController::start() {
    sync_->start();
}

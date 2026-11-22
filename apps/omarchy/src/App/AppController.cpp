#include "App/AppController.h"

#include <QQmlContext>

#include "App/TrackerSync.h"
#include "Features/BulkTaskArchiving/BulkTaskArchivingStore.h"
#include "Features/Connection/ConnectionStore.h"
#include "Features/DailyTotals/DailyTotalsStore.h"
#include "Features/TaskCatalog/TaskCatalogStore.h"
#include "Features/TaskArchiving/TaskArchivingStore.h"
#include "Features/TaskCreation/TaskCreationStore.h"
#include "Features/TaskRename/TaskRenameStore.h"
#include "Features/Tracking/TrackingStore.h"
#include "Features/WorklogCorrection/WorklogCorrectionStore.h"
#include "Features/WorklogDeletion/WorklogDeletionStore.h"
#include "Features/WorklogHistory/WorklogHistoryStore.h"
#include "Features/WorklogMove/WorklogMoveStore.h"
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
      sync_(new TrackerSync(gateway_, clock_, {connection_, catalog_, tracking_, totals_, history_}, 30000, this)),
      taskCreation_(new TaskCreationStore(gateway_, catalog_, clock_, this)),
      taskRename_(new TaskRenameStore(gateway_, catalog_, clock_, this)),
      taskArchiving_(new TaskArchivingStore(gateway_, catalog_, clock_, this)),
      bulkArchiving_(new BulkTaskArchivingStore(gateway_, clock_, this)),
      correction_(new WorklogCorrectionStore(gateway_, history_, clock_, this)),
      move_(new WorklogMoveStore(gateway_, history_, this)),
      deletion_(new WorklogDeletionStore(gateway_, history_, clock_, this)) {
    // Every write is followed by a read, whether it worked or not, so the window shows what the core kept.
    const auto refreshAfterWrites = [this](auto *store) {
        connect(store, &std::remove_pointer_t<decltype(store)>::refreshRequested, this,
                [this] { sync_->refresh(true); });
    };
    refreshAfterWrites(taskCreation_);
    refreshAfterWrites(taskRename_);
    refreshAfterWrites(taskArchiving_);
    refreshAfterWrites(bulkArchiving_);
    refreshAfterWrites(correction_);
    refreshAfterWrites(move_);
    refreshAfterWrites(deletion_);
}

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
    context->setContextProperty(QStringLiteral("taskCreation"), taskCreation_);
    context->setContextProperty(QStringLiteral("taskRename"), taskRename_);
    context->setContextProperty(QStringLiteral("taskArchiving"), taskArchiving_);
    context->setContextProperty(QStringLiteral("bulkArchiving"), bulkArchiving_);
    context->setContextProperty(QStringLiteral("worklogCorrection"), correction_);
    context->setContextProperty(QStringLiteral("worklogMove"), move_);
    context->setContextProperty(QStringLiteral("worklogDeletion"), deletion_);
}

void AppController::start() {
    sync_->start();
}

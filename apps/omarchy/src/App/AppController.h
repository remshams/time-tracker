#pragma once

#include <QObject>
#include <QSettings>
#include <memory>

class BulkTaskArchivingStore;
class ConnectionStore;
class DailyTotalsStore;
class DisplayClock;
class Format;
class QQmlContext;
class TaskCatalogStore;
class TaskArchivingStore;
class TaskCreationStore;
class TaskRenameStore;
class Theme;
class TrackerGateway;
class TrackerSync;
class TrackingStore;
class WorklogCorrectionStore;
class WorklogHistoryStore;

// Creates the gateway, the clock, the theme and every feature store, connects them, and
// hands them to QML as context properties.
class AppController : public QObject {
    Q_OBJECT
public:
    AppController(TrackerGateway *gateway, Theme *theme, QObject *parent = nullptr);
    ~AppController() override;

    void expose(QQmlContext *context);
    void start();

private:
    TrackerGateway *gateway_;
    Theme *theme_;
    QSettings settings_;
    DisplayClock *clock_;
    Format *format_;
    ConnectionStore *connection_;
    TaskCatalogStore *catalog_;
    TrackingStore *tracking_;
    DailyTotalsStore *totals_;
    WorklogHistoryStore *history_;
    TrackerSync *sync_;
    TaskCreationStore *taskCreation_;
    TaskRenameStore *taskRename_;
    TaskArchivingStore *taskArchiving_;
    BulkTaskArchivingStore *bulkArchiving_;
    WorklogCorrectionStore *correction_;
};

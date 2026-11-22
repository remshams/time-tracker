#pragma once

#include <QObject>

#include "Contracts/BridgeReply.h"

class DisplayClock;
class TaskCatalogStore;
class TrackerGateway;

// Archives an active task after a confirmation and unarchives an archived one at once.
// It captures the clicked task, its name and the action when the command starts. Archiving
// sends the name, so the core rejects it when another client renamed the task. Archiving
// the running task is rejected by the core, and its message shows in the dialog.
class TaskArchivingStore : public QObject {
    Q_OBJECT
    Q_PROPERTY(bool unarchive READ unarchive NOTIFY stateChanged)
    Q_PROPERTY(QString taskName READ taskName NOTIFY stateChanged)
    Q_PROPERTY(bool pending READ pending NOTIFY stateChanged)
    Q_PROPERTY(QString error READ error NOTIFY stateChanged)
    Q_PROPERTY(bool canSubmit READ canSubmit NOTIFY stateChanged)
public:
    TaskArchivingStore(TrackerGateway *gateway, TaskCatalogStore *catalog, DisplayClock *clock,
                       QObject *parent = nullptr);

    bool unarchive() const { return unarchive_; }
    QString taskName() const { return taskName_; }
    bool pending() const { return pending_; }
    QString error() const { return error_; }
    bool canSubmit() const { return !taskId_.isEmpty() && !pending_; }

    // Starts the command for `taskId`. An active task asks for confirmation through
    // dialogRequested(); an archived one is unarchived without asking.
    Q_INVOKABLE void begin(const QString &taskId);
    Q_INVOKABLE void submit();

signals:
    void stateChanged();
    // The dialog should open: to confirm an archive, or to show a failure.
    void dialogRequested();
    // The command worked. The dialog closes.
    void succeeded();
    // A write finished, successfully or not; the task list must reload.
    void refreshRequested();

private:
    void finish(const BridgeReply &reply);

    TrackerGateway *gateway_;
    TaskCatalogStore *catalog_;
    DisplayClock *clock_;
    QString taskId_;
    QString taskName_;
    bool unarchive_ = false;
    QString error_;
    bool pending_ = false;
};

#pragma once

#include <QObject>
#include <optional>

#include "Contracts/BridgeReply.h"
#include "Contracts/Models.h"

class DisplayClock;
class TrackerGateway;
class WorklogHistoryStore;

// The confirmation behind the "Delete" dialog. It captures the worklog and its original
// timestamp texts when the dialog opens and shows what will be removed. Only a completed
// worklog can go; the core checks the task and the timestamps again before it deletes.
class WorklogDeletionStore : public QObject {
    Q_OBJECT
    Q_PROPERTY(QString taskId READ taskId NOTIFY begun)
    // "Today", "Yesterday" or a date, for the day the worklog started.
    Q_PROPERTY(QString dayLabel READ dayLabel NOTIFY begun)
    Q_PROPERTY(QString startLabel READ startLabel NOTIFY begun)
    // "now" while the worklog runs.
    Q_PROPERTY(QString endLabel READ endLabel NOTIFY begun)
    Q_PROPERTY(qint64 durationSeconds READ durationSeconds NOTIFY begun)
    Q_PROPERTY(bool canDelete READ canDelete NOTIFY canDeleteChanged)
    Q_PROPERTY(bool pending READ pending NOTIFY pendingChanged)
    Q_PROPERTY(QString error READ error NOTIFY errorChanged)
public:
    WorklogDeletionStore(TrackerGateway *gateway, WorklogHistoryStore *history, DisplayClock *clock,
                         QObject *parent = nullptr);

    QString taskId() const { return original_ ? original_->taskId : QString(); }
    QString dayLabel() const { return dayLabel_; }
    QString startLabel() const { return startLabel_; }
    QString endLabel() const { return endLabel_; }
    qint64 durationSeconds() const { return durationSeconds_; }
    bool canDelete() const { return active_ && !original_->isRunning() && !pending_; }
    bool pending() const { return pending_; }
    QString error() const { return error_; }

    // Captures the worklog from the shown history. False when it is not there any more.
    // A running worklog opens with an error and cannot be deleted.
    Q_INVOKABLE bool begin(const QString &worklogId);
    // Drops the dialog's state. An answer to a deletion that is still open only triggers the refresh.
    Q_INVOKABLE void cancel();
    Q_INVOKABLE void confirm();

signals:
    void begun();
    void canDeleteChanged();
    void pendingChanged();
    void errorChanged();
    // The deletion worked; the dialog closes.
    void deleted();
    // A write finished, successfully or not; the history, the tasks and the totals must reload.
    void refreshRequested();

private:
    void refreshCanDelete();
    void setPending(bool pending);
    void setError(const QString &error);
    void finish(const BridgeReply &reply);

    TrackerGateway *gateway_;
    WorklogHistoryStore *history_;
    DisplayClock *clock_;
    std::optional<WorklogItem> original_;
    bool active_ = false;
    QString dayLabel_;
    QString startLabel_;
    QString endLabel_;
    qint64 durationSeconds_ = 0;
    bool canDelete_ = false;
    bool pending_ = false;
    QString error_;
    // Each dialog and each cancel takes a new session; answers for an older deletion are dropped.
    int session_ = 0;
};

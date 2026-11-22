#pragma once

#include <QObject>
#include <optional>

#include "Contracts/BridgeReply.h"
#include "Contracts/Models.h"

class DisplayClock;
class TrackerGateway;

// The running worklog, its timer, and the start and stop commands.
// Commands capture the target, the expected running worklog and the time when the user
// acts; the core rejects them when another client changed tracking in between.
class TrackingStore : public QObject {
    Q_OBJECT
    Q_PROPERTY(bool known READ known NOTIFY stateChanged)
    Q_PROPERTY(bool running READ running NOTIFY stateChanged)
    Q_PROPERTY(QString runningTaskId READ runningTaskId NOTIFY stateChanged)
    Q_PROPERTY(QString runningWorklogId READ runningWorklogId NOTIFY stateChanged)
    Q_PROPERTY(QDateTime runningSince READ runningSince NOTIFY stateChanged)
    Q_PROPERTY(qint64 elapsedSeconds READ elapsedSeconds NOTIFY elapsedChanged)
    Q_PROPERTY(bool stale READ stale NOTIFY staleChanged)
    Q_PROPERTY(bool available READ available NOTIFY availabilityChanged)
    Q_PROPERTY(QString error READ error NOTIFY errorChanged)
public:
    TrackingStore(TrackerGateway *gateway, DisplayClock *clock, QObject *parent = nullptr);

    // Takes in the tracking state from a refresh.
    void accept(const std::optional<WorklogItem> &running);
    // Marks the shown state as last confirmed while the connection is unavailable.
    void setStale(bool stale);
    // Forgets the state, for a connection change.
    void reset();

    bool known() const { return known_; }
    bool running() const { return running_.has_value(); }
    QString runningTaskId() const { return running_ ? running_->taskId : QString(); }
    QString runningWorklogId() const { return running_ ? running_->id : QString(); }
    QDateTime runningSince() const { return running_ ? running_->start : QDateTime(); }
    const std::optional<WorklogItem> &runningWorklog() const { return running_; }
    qint64 elapsedSeconds() const;
    bool pending() const { return pending_; }
    bool stale() const { return stale_; }
    bool available() const { return known_ && !stale_ && !pending_; }
    QString error() const { return error_; }

    // Starts `taskId`, stopping the running worklog at the same instant.
    Q_INVOKABLE void start(const QString &taskId);
    Q_INVOKABLE void stop();
    // Stops the running worklog, or starts `taskId` while idle.
    Q_INVOKABLE void toggle(const QString &taskId);

signals:
    void stateChanged();
    void elapsedChanged();
    void pendingChanged();
    void staleChanged();
    void availabilityChanged();
    void errorChanged();
    // A write finished, successfully or not; the task list must reload.
    void refreshRequested();

private:
    void setPending(bool pending);
    void setError(const QString &error);
    void finish(const BridgeReply &reply, const QString &unconfirmed);

    TrackerGateway *gateway_;
    DisplayClock *clock_;
    bool known_ = false;
    std::optional<WorklogItem> running_;
    bool pending_ = false;
    bool stale_ = false;
    QString error_;
    QDateTime errorSince_;
    bool reloadedSinceError_ = false;
};

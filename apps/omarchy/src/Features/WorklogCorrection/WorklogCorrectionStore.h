#pragma once

#include <QDateTime>
#include <QObject>
#include <optional>

#include "Contracts/BridgeReply.h"
#include "Contracts/Models.h"

class DisplayClock;
class TrackerGateway;
class WorklogHistoryStore;

// The draft behind the "Edit times" dialog. It captures the worklog and its original
// timestamp texts when the dialog opens, shows them as local date and time fields, and
// sends the originals alongside the replacements so the core can reject a stale edit.
// The core owns the rules: overlap, future time and end before start.
class WorklogCorrectionStore : public QObject {
    Q_OBJECT
    Q_PROPERTY(QString taskId READ taskId NOTIFY begun)
    Q_PROPERTY(bool running READ running NOTIFY begun)
    Q_PROPERTY(QString startDate READ startDate WRITE setStartDate NOTIFY fieldsChanged)
    Q_PROPERTY(QString startTime READ startTime WRITE setStartTime NOTIFY fieldsChanged)
    Q_PROPERTY(QString endDate READ endDate WRITE setEndDate NOTIFY fieldsChanged)
    Q_PROPERTY(QString endTime READ endTime WRITE setEndTime NOTIFY fieldsChanged)
    Q_PROPERTY(QString timeZone READ timeZone NOTIFY begun)
    // The draft's duration in seconds, or 0 while a field cannot be read. It is negative
    // when the end is before the start; the core rejects that on save.
    Q_PROPERTY(qint64 durationSeconds READ durationSeconds NOTIFY previewChanged)
    Q_PROPERTY(QString validationMessage READ validationMessage NOTIFY fieldsChanged)
    Q_PROPERTY(bool changed READ changed NOTIFY fieldsChanged)
    Q_PROPERTY(bool canSave READ canSave NOTIFY canSaveChanged)
    Q_PROPERTY(bool pending READ pending NOTIFY pendingChanged)
    Q_PROPERTY(QString error READ error NOTIFY errorChanged)
public:
    WorklogCorrectionStore(TrackerGateway *gateway, WorklogHistoryStore *history, DisplayClock *clock,
                           QObject *parent = nullptr);

    QString taskId() const { return original_ ? original_->taskId : QString(); }
    bool running() const { return original_ && original_->isRunning(); }
    QString startDate() const { return startDate_; }
    QString startTime() const { return startTime_; }
    QString endDate() const { return endDate_; }
    QString endTime() const { return endTime_; }
    void setStartDate(const QString &text);
    void setStartTime(const QString &text);
    void setEndDate(const QString &text);
    void setEndTime(const QString &text);
    QString timeZone() const { return timeZone_; }
    qint64 durationSeconds() const;
    QString validationMessage() const;
    bool changed() const;
    bool canSave() const;
    bool pending() const { return pending_; }
    QString error() const { return error_; }

    // Captures the worklog from the shown history. False when it is not there any more.
    Q_INVOKABLE bool begin(const QString &worklogId);
    // Drops the draft. An answer to a save that is still open only triggers the refresh.
    Q_INVOKABLE void cancel();
    Q_INVOKABLE void save();

signals:
    // A new draft was captured.
    void begun();
    void fieldsChanged();
    void previewChanged();
    void canSaveChanged();
    void pendingChanged();
    void errorChanged();
    void saved();
    // A write finished, successfully or not; the history and the totals must reload.
    void refreshRequested();

private:
    // The timestamps the draft would send, and the same instants for the preview.
    struct Replacement {
        QString startText;
        QString endText; // Empty for a running worklog.
        QDateTime start;
        std::optional<QDateTime> end;
    };

    bool startChanged() const;
    bool endChanged() const;
    std::optional<Replacement> replacement() const;
    void fieldEdited(QString &field, const QString &text);
    void refreshState();
    void refreshCanSave();
    void setPending(bool pending);
    void setError(const QString &error);
    void finish(const BridgeReply &reply);

    TrackerGateway *gateway_;
    WorklogHistoryStore *history_;
    DisplayClock *clock_;
    std::optional<WorklogItem> original_;
    QString initialStartDate_;
    QString initialStartTime_;
    QString initialEndDate_;
    QString initialEndTime_;
    QString startDate_;
    QString startTime_;
    QString endDate_;
    QString endTime_;
    QString timeZone_;
    bool pending_ = false;
    bool canSave_ = false;
    QString error_;
    // Each draft and each save takes a new session; answers for an older one are dropped.
    int session_ = 0;
    // Whether a draft is open. The preview of a running worklog only ticks while it is.
    bool active_ = false;
};

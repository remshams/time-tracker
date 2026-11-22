#pragma once

#include <QObject>

#include "Contracts/BridgeReply.h"

class DisplayClock;
class TaskCatalogStore;
class TrackerGateway;

// The rename dialog. It captures the clicked task and its name when the dialog opens and
// sends that name as the expected one, so the core rejects the rename when another client
// changed the name in between. The draft survives background refreshes and errors.
class TaskRenameStore : public QObject {
    Q_OBJECT
    Q_PROPERTY(QString draft READ draft WRITE setDraft NOTIFY draftChanged)
    Q_PROPERTY(QString originalName READ originalName NOTIFY stateChanged)
    Q_PROPERTY(bool pending READ pending NOTIFY stateChanged)
    Q_PROPERTY(QString error READ error NOTIFY stateChanged)
    Q_PROPERTY(bool canSubmit READ canSubmit NOTIFY stateChanged)
public:
    TaskRenameStore(TrackerGateway *gateway, TaskCatalogStore *catalog, DisplayClock *clock, QObject *parent = nullptr);

    QString draft() const { return draft_; }
    void setDraft(const QString &draft);
    QString originalName() const { return originalName_; }
    bool pending() const { return pending_; }
    QString error() const { return error_; }
    // A rename needs a new, non-empty name.
    bool canSubmit() const;

    // Captures the task and starts a draft with its current name. False when the task is
    // unknown, in which case the dialog stays closed.
    Q_INVOKABLE bool begin(const QString &taskId);
    Q_INVOKABLE void submit();

signals:
    void draftChanged();
    void stateChanged();
    // The name changed. The dialog closes.
    void succeeded();
    // A write finished, successfully or not; the task list must reload.
    void refreshRequested();

private:
    void finish(const BridgeReply &reply);

    TrackerGateway *gateway_;
    TaskCatalogStore *catalog_;
    DisplayClock *clock_;
    QString taskId_;
    QString originalName_;
    QString draft_;
    QString error_;
    bool pending_ = false;
};

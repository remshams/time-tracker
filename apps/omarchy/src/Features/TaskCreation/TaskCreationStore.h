#pragma once

#include <QDateTime>
#include <QObject>

#include "Contracts/BridgeReply.h"

class DisplayClock;
class TaskCatalogStore;
class TrackerGateway;

// The create-task dialog: the draft name, the write, and selecting the new task.
// Rust validates the name and creates the task ID; the store only trims the draft.
class TaskCreationStore : public QObject {
    Q_OBJECT
    Q_PROPERTY(QString draft READ draft WRITE setDraft NOTIFY draftChanged)
    Q_PROPERTY(bool pending READ pending NOTIFY stateChanged)
    Q_PROPERTY(QString error READ error NOTIFY stateChanged)
    Q_PROPERTY(bool canSubmit READ canSubmit NOTIFY stateChanged)
public:
    TaskCreationStore(TrackerGateway *gateway, TaskCatalogStore *catalog, DisplayClock *clock,
                      QObject *parent = nullptr);

    QString draft() const { return draft_; }
    void setDraft(const QString &draft);
    bool pending() const { return pending_; }
    QString error() const { return error_; }
    bool canSubmit() const { return !pending_ && !draft_.trimmed().isEmpty(); }

    // Starts a fresh draft when the dialog opens.
    Q_INVOKABLE void begin();
    Q_INVOKABLE void submit();

signals:
    void draftChanged();
    void stateChanged();
    // The task exists. The dialog closes.
    void succeeded();
    // A write finished, successfully or not; the task list must reload.
    void refreshRequested();

private:
    void finish(const BridgeReply &reply);
    // Selects the created task as soon as the task list contains it.
    void selectCreated();

    TrackerGateway *gateway_;
    TaskCatalogStore *catalog_;
    DisplayClock *clock_;
    QString draft_;
    QString error_;
    bool pending_ = false;
    QString createdId_;
    QDateTime createdAt_;
};

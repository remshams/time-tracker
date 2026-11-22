#pragma once

#include <QDateTime>
#include <QObject>
#include <QVariantList>
#include <optional>

#include "Contracts/BridgeReply.h"
#include "Contracts/Models.h"

class TrackerGateway;
class WorklogHistoryStore;

// The state behind the "Move to task" dialog: the worklog, a destination search and the
// move itself. The search asks the core once per query change and drops answers for an
// older query. Results of the previous query stay on screen while a new search runs, but
// the move waits until the results belong to the current query.
class WorklogMoveStore : public QObject {
    Q_OBJECT
    Q_PROPERTY(QString sourceTaskId READ sourceTaskId NOTIFY begun)
    Q_PROPERTY(bool running READ running NOTIFY begun)
    Q_PROPERTY(QDateTime start READ start NOTIFY begun)
    // Invalid while the worklog runs.
    Q_PROPERTY(QDateTime end READ end NOTIFY begun)
    Q_PROPERTY(QString query READ query WRITE setQuery NOTIFY queryChanged)
    // [{id, name}] in the order the core ranks them.
    Q_PROPERTY(QVariantList candidates READ candidates NOTIFY candidatesChanged)
    Q_PROPERTY(int currentIndex READ currentIndex NOTIFY selectionChanged)
    Q_PROPERTY(QString selectedTaskId READ selectedTaskId NOTIFY selectionChanged)
    Q_PROPERTY(bool searching READ searching NOTIFY searchStateChanged)
    // Whether the shown candidates answer the current query.
    Q_PROPERTY(bool fresh READ fresh NOTIFY searchStateChanged)
    Q_PROPERTY(bool canMove READ canMove NOTIFY canMoveChanged)
    Q_PROPERTY(bool pending READ pending NOTIFY pendingChanged)
    Q_PROPERTY(QString error READ error NOTIFY errorChanged)
public:
    WorklogMoveStore(TrackerGateway *gateway, WorklogHistoryStore *history, QObject *parent = nullptr);

    QString sourceTaskId() const { return original_ ? original_->taskId : QString(); }
    bool running() const { return original_ && original_->isRunning(); }
    QDateTime start() const { return original_ ? original_->start : QDateTime(); }
    QDateTime end() const { return original_ && original_->end ? *original_->end : QDateTime(); }
    QString query() const { return query_; }
    void setQuery(const QString &text);
    QVariantList candidates() const { return candidates_; }
    int currentIndex() const;
    QString selectedTaskId() const { return selectedId_; }
    bool searching() const { return searching_; }
    bool fresh() const { return fresh_; }
    bool canMove() const;
    bool pending() const { return pending_; }
    QString error() const { return error_; }

    // Captures the worklog from the shown history and starts the search for an empty
    // query. False when the worklog is not there any more.
    Q_INVOKABLE bool begin(const QString &worklogId);
    // Drops the dialog's state. An answer to a move that is still open only triggers the refresh.
    Q_INVOKABLE void cancel();
    // Selects the previous or next candidate, stopping at the ends.
    Q_INVOKABLE void moveSelection(int step);
    Q_INVOKABLE void select(int index);
    Q_INVOKABLE void move();

signals:
    void begun();
    void queryChanged();
    void candidatesChanged();
    void selectionChanged();
    void searchStateChanged();
    void canMoveChanged();
    void pendingChanged();
    void errorChanged();
    // The move worked; the dialog closes.
    void moved(const QString &destinationTaskId);
    // A write finished, successfully or not; the history, the tasks and the totals must reload.
    void refreshRequested();

private:
    void search();
    void acceptCandidates(const BridgeReply &reply);
    void setSelectedTaskId(const QString &taskId);
    void setSearchState(bool searching, bool fresh);
    void refreshCanMove();
    void setPending(bool pending);
    void setError(const QString &error);
    void finish(const BridgeReply &reply);

    TrackerGateway *gateway_;
    WorklogHistoryStore *history_;
    std::optional<WorklogItem> original_;
    bool active_ = false;
    QString query_;
    QVariantList candidates_;
    QString selectedId_;
    bool searching_ = false;
    bool fresh_ = false;
    bool canMove_ = false;
    bool pending_ = false;
    QString error_;
    // Each dialog and each cancel takes a new session; answers for an older move are dropped.
    int session_ = 0;
    // Each search takes a new generation; answers for an older one are dropped.
    int searchGeneration_ = 0;
};

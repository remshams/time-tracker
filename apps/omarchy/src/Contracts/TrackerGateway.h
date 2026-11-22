#pragma once

#include <QDateTime>
#include <QString>
#include <functional>

#include "Contracts/BridgeReply.h"

// Where the tracker data lives: the local database or a tracker server.
struct ConnectionTarget {
    bool remote = false;
    QString serverUrl;

    bool operator==(const ConnectionTarget &other) const = default;
};

// The app's only way to the Rust core. Every call answers once, on the GUI thread.
// Feature stores depend on this interface so that tests can script the replies.
class TrackerGateway {
public:
    using Callback = std::function<void(const BridgeReply &)>;

    virtual ~TrackerGateway() = default;

    // Opens the target and checks it. A failure keeps the previous connection.
    virtual void connectTo(const ConnectionTarget &target, Callback done) = 0;
    // Opens the target, checks it and closes it again, leaving the current connection alone.
    virtual void testConnection(const ConnectionTarget &target, Callback done) = 0;

    // Refreshes resources and answers {tasks, tracking, report} for the given day.
    virtual void refreshOverview(const QDateTime &dayStart, const QDateTime &dayEnd, const QDateTime &now,
                                 Callback done) = 0;
    virtual void history(const QString &taskId, const QString &cursor, Callback done) = 0;

    virtual void createTask(const QString &name, const QDateTime &at, Callback done) = 0;
    virtual void renameTask(const QString &taskId, const QString &expectedName, const QString &name,
                            const QDateTime &at, Callback done) = 0;
    virtual void archiveTask(const QString &taskId, const QString &expectedName, const QDateTime &at,
                             Callback done) = 0;
    virtual void unarchiveTask(const QString &taskId, const QDateTime &at, Callback done) = 0;
    virtual void previewInactiveTasks(int inactiveDays, const QDateTime &asOf, Callback done) = 0;
    virtual void archiveInactiveTasks(int inactiveDays, const QDateTime &asOf, Callback done) = 0;

    // An empty `expectedActiveWorklogId` expects the tracker to be idle.
    virtual void startTracking(const QString &taskId, const QString &expectedActiveWorklogId, const QDateTime &at,
                               Callback done) = 0;
    virtual void stopTracking(const QString &worklogId, const QDateTime &at, Callback done) = 0;

    // Empty end texts describe a running worklog.
    virtual void correctWorklog(const QString &worklogId, const QString &expectedStart, const QString &expectedEnd,
                                const QString &replacementStart, const QString &replacementEnd, const QDateTime &at,
                                Callback done) = 0;
    virtual void moveCandidates(const QString &sourceTaskId, const QString &query, Callback done) = 0;
    virtual void moveWorklog(const QString &worklogId, const QString &expectedTaskId, const QString &expectedStart,
                             const QString &expectedEnd, const QString &destinationTaskId, Callback done) = 0;
    virtual void deleteWorklog(const QString &worklogId, const QString &expectedTaskId, const QString &expectedStart,
                               const QString &expectedEnd, Callback done) = 0;
};

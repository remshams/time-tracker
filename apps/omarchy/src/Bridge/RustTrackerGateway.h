#pragma once

#include <QObject>
#include <QThreadPool>
#include <functional>

#include "Contracts/TrackerGateway.h"

struct Bridge;

// Runs every bridge call on one worker thread, in order, and answers on the GUI thread.
// The worker owns the Rust handle; the GUI thread never touches it.
class RustTrackerGateway : public QObject, public TrackerGateway {
    Q_OBJECT
public:
    explicit RustTrackerGateway(QObject *parent = nullptr);
    ~RustTrackerGateway() override;

    void connectTo(const ConnectionTarget &target, Callback done) override;
    void testConnection(const ConnectionTarget &target, Callback done) override;
    void refreshOverview(const QDateTime &dayStart, const QDateTime &dayEnd, const QDateTime &now,
                         Callback done) override;
    void history(const QString &taskId, const QString &cursor, Callback done) override;
    void createTask(const QString &name, const QDateTime &at, Callback done) override;
    void renameTask(const QString &taskId, const QString &expectedName, const QString &name, const QDateTime &at,
                    Callback done) override;
    void archiveTask(const QString &taskId, const QString &expectedName, const QDateTime &at, Callback done) override;
    void unarchiveTask(const QString &taskId, const QDateTime &at, Callback done) override;
    void previewInactiveTasks(int inactiveDays, const QDateTime &asOf, Callback done) override;
    void archiveInactiveTasks(int inactiveDays, const QDateTime &asOf, Callback done) override;
    void startTracking(const QString &taskId, const QString &expectedActiveWorklogId, const QDateTime &at,
                       Callback done) override;
    void stopTracking(const QString &worklogId, const QDateTime &at, Callback done) override;
    void correctWorklog(const QString &worklogId, const QString &expectedStart, const QString &expectedEnd,
                        const QString &replacementStart, const QString &replacementEnd, const QDateTime &at,
                        Callback done) override;
    void moveCandidates(const QString &sourceTaskId, const QString &query, Callback done) override;
    void moveWorklog(const QString &worklogId, const QString &expectedTaskId, const QString &expectedStart,
                     const QString &expectedEnd, const QString &destinationTaskId, Callback done) override;
    void deleteWorklog(const QString &worklogId, const QString &expectedTaskId, const QString &expectedStart,
                       const QString &expectedEnd, Callback done) override;

private:
    using Job = std::function<BridgeReply(Bridge *&bridge)>;
    void run(Job job, Callback done);

    QThreadPool worker_;
    Bridge *bridge_ = nullptr; // Touched only by jobs on `worker_`.
};

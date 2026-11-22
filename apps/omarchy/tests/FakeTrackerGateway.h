#pragma once

#include <QHash>
#include <QList>
#include <QStringList>

#include "Contracts/TrackerGateway.h"

// Records every call and holds its callback until the test answers it, so tests can check
// pending states. Methods listed in `autoReplies` answer at once.
class FakeTrackerGateway : public TrackerGateway {
public:
    struct Call {
        QString method;
        QStringList args;
        Callback done;
    };

    QList<Call> calls;
    QHash<QString, BridgeReply> autoReplies;

    // Answers the oldest open call to `method`. Returns false when there is none.
    bool respond(const QString &method, const BridgeReply &reply) {
        for (int index = 0; index < calls.size(); ++index) {
            if (calls[index].method == method && calls[index].done) {
                Callback done = std::move(calls[index].done);
                calls[index].done = nullptr;
                done(reply);
                return true;
            }
        }
        return false;
    }

    int count(const QString &method) const {
        int total = 0;
        for (const Call &call : calls) {
            total += call.method == method ? 1 : 0;
        }
        return total;
    }

    QStringList lastArgs(const QString &method) const {
        for (auto it = calls.crbegin(); it != calls.crend(); ++it) {
            if (it->method == method) {
                return it->args;
            }
        }
        return {};
    }

    void connectTo(const ConnectionTarget &target, Callback done) override {
        record("connectTo", {target.remote ? "remote" : "local", target.serverUrl}, std::move(done));
    }
    void testConnection(const ConnectionTarget &target, Callback done) override {
        record("testConnection", {target.remote ? "remote" : "local", target.serverUrl}, std::move(done));
    }
    void refreshOverview(const QDateTime &dayStart, const QDateTime &dayEnd, const QDateTime &now,
                         Callback done) override {
        record("refreshOverview", {stamp(dayStart), stamp(dayEnd), stamp(now)}, std::move(done));
    }
    void history(const QString &taskId, const QString &cursor, Callback done) override {
        record("history", {taskId, cursor}, std::move(done));
    }
    void createTask(const QString &name, const QDateTime &at, Callback done) override {
        record("createTask", {name, stamp(at)}, std::move(done));
    }
    void renameTask(const QString &taskId, const QString &expectedName, const QString &name, const QDateTime &at,
                    Callback done) override {
        record("renameTask", {taskId, expectedName, name, stamp(at)}, std::move(done));
    }
    void archiveTask(const QString &taskId, const QString &expectedName, const QDateTime &at, Callback done) override {
        record("archiveTask", {taskId, expectedName, stamp(at)}, std::move(done));
    }
    void unarchiveTask(const QString &taskId, const QDateTime &at, Callback done) override {
        record("unarchiveTask", {taskId, stamp(at)}, std::move(done));
    }
    void previewInactiveTasks(int inactiveDays, const QDateTime &asOf, Callback done) override {
        record("previewInactiveTasks", {QString::number(inactiveDays), stamp(asOf)}, std::move(done));
    }
    void archiveInactiveTasks(int inactiveDays, const QDateTime &asOf, Callback done) override {
        record("archiveInactiveTasks", {QString::number(inactiveDays), stamp(asOf)}, std::move(done));
    }
    void startTracking(const QString &taskId, const QString &expectedActiveWorklogId, const QDateTime &at,
                       Callback done) override {
        record("startTracking", {taskId, expectedActiveWorklogId, stamp(at)}, std::move(done));
    }
    void stopTracking(const QString &worklogId, const QDateTime &at, Callback done) override {
        record("stopTracking", {worklogId, stamp(at)}, std::move(done));
    }
    void correctWorklog(const QString &worklogId, const QString &expectedStart, const QString &expectedEnd,
                        const QString &replacementStart, const QString &replacementEnd, const QDateTime &at,
                        Callback done) override {
        record("correctWorklog", {worklogId, expectedStart, expectedEnd, replacementStart, replacementEnd, stamp(at)},
               std::move(done));
    }
    void moveCandidates(const QString &sourceTaskId, const QString &query, Callback done) override {
        record("moveCandidates", {sourceTaskId, query}, std::move(done));
    }
    void moveWorklog(const QString &worklogId, const QString &expectedTaskId, const QString &expectedStart,
                     const QString &expectedEnd, const QString &destinationTaskId, Callback done) override {
        record("moveWorklog", {worklogId, expectedTaskId, expectedStart, expectedEnd, destinationTaskId},
               std::move(done));
    }
    void deleteWorklog(const QString &worklogId, const QString &expectedTaskId, const QString &expectedStart,
                       const QString &expectedEnd, Callback done) override {
        record("deleteWorklog", {worklogId, expectedTaskId, expectedStart, expectedEnd}, std::move(done));
    }

private:
    static QString stamp(const QDateTime &value) { return value.toUTC().toString(Qt::ISODateWithMs); }

    void record(const QString &method, const QStringList &args, Callback done) {
        if (autoReplies.contains(method)) {
            done(autoReplies.value(method));
            calls.append({method, args, nullptr});
            return;
        }
        calls.append({method, args, std::move(done)});
    }
};

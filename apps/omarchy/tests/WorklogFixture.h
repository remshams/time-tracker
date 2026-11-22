#pragma once

#include <QJsonArray>
#include <QJsonObject>
#include <QTimeZone>

#include "FakeTrackerGateway.h"
#include "Features/TaskCatalog/TaskCatalogStore.h"
#include "Features/WorklogHistory/WorklogHistoryStore.h"
#include "Shared/DisplayClock.h"

// A catalog with two tasks and a history for `task-a`, built the way the app builds them,
// so the worklog dialogs' stores read real worklogs from the history store.
struct WorklogFixture {
    FakeTrackerGateway gateway;
    QDateTime now = QDateTime(QDate(2026, 11, 20), QTime(12, 0), QTimeZone::UTC);
    DisplayClock clock{[this] { return now; }, false};
    TaskCatalogStore catalog{&clock};
    WorklogHistoryStore history{&gateway, &catalog, &clock};

    WorklogFixture() {
        TaskItem alpha;
        alpha.id = QStringLiteral("task-a");
        alpha.name = QStringLiteral("Alpha");
        TaskItem beta;
        beta.id = QStringLiteral("task-b");
        beta.name = QStringLiteral("Beta");
        catalog.accept({alpha, beta});
    }

    // An empty `end` is a running worklog.
    static QJsonObject worklog(const QString &id, const QString &start, const QString &end = QString()) {
        return {{QStringLiteral("id"), id},
                {QStringLiteral("taskId"), QStringLiteral("task-a")},
                {QStringLiteral("start"), start},
                {QStringLiteral("end"), end.isEmpty() ? QJsonValue(QJsonValue::Null) : QJsonValue(end)}};
    }

    // Selects `task-a` and answers its history request with `worklogs`.
    void showHistory(const QList<QJsonObject> &worklogs) {
        catalog.select(QStringLiteral("task-a"));
        QJsonArray rows;
        for (const QJsonObject &row : worklogs) {
            rows.append(row);
        }
        gateway.respond(QStringLiteral("history"),
                        BridgeReply::success(QJsonObject{{QStringLiteral("worklogs"), rows},
                                                         {QStringLiteral("nextCursor"), QJsonValue::Null}}));
    }
};

// Answers like a write the server committed: {"worklog": {...}}.
inline BridgeReply writeSucceeded() {
    return BridgeReply::success(QJsonObject{{QStringLiteral("worklog"), QJsonObject()}});
}

// Answers like a write whose outcome the client cannot know.
inline BridgeReply writeUncertain() {
    BridgeReply reply = BridgeReply::failure(QStringLiteral("The server timed out"), QStringLiteral("timeout"));
    reply.uncertain = true;
    return reply;
}

#include "Bridge/RustTrackerGateway.h"

#include <QJsonObject>
#include <QMetaObject>

#include "Contracts/Models.h"

extern "C" {
#include "TrackerBridge.h"
}

namespace {

// Takes ownership of a string the bridge returned and decodes its envelope.
BridgeReply take(char *raw) {
    if (raw == nullptr) {
        return BridgeReply::failure(QStringLiteral("The tracker returned no answer"));
    }
    const QByteArray json(raw);
    tt_bridge_string_free(raw);
    return BridgeReply::fromEnvelope(json);
}

// Opens a bridge for `target`, or returns the error the bridge reported.
Bridge *open(const ConnectionTarget &target, QString *error) {
    char *message = nullptr;
    Bridge *bridge = nullptr;
    if (target.remote) {
        const QByteArray url = target.serverUrl.trimmed().toUtf8();
        bridge = tt_bridge_open_remote(url.constData(), &message);
    } else {
        bridge = tt_bridge_open(&message);
    }
    if (message != nullptr) {
        *error = QString::fromUtf8(message);
        tt_bridge_string_free(message);
    }
    return bridge;
}

// Opens and checks `target`. On success the caller owns the returned bridge.
Bridge *openChecked(const ConnectionTarget &target, BridgeReply *reply) {
    QString error;
    Bridge *candidate = open(target, &error);
    if (candidate == nullptr) {
        *reply = BridgeReply::failure(error.isEmpty() ? QStringLiteral("Could not open the connection") : error);
        return nullptr;
    }
    *reply = take(tt_bridge_check_connection(candidate));
    if (!reply->ok) {
        tt_bridge_close(candidate);
        return nullptr;
    }
    return candidate;
}

QByteArray utf8(const QString &text) {
    return text.toUtf8();
}

// Null stands for "absent" in the bridge's optional arguments.
const char *optional(const QByteArray &text) {
    return text.isEmpty() ? nullptr : text.constData();
}

QByteArray stamp(const QDateTime &value) {
    return Timestamps::format(value).toUtf8();
}

} // namespace

RustTrackerGateway::RustTrackerGateway(QObject *parent) : QObject(parent) {
    // One thread keeps the bridge calls in order, which the bridge requires.
    worker_.setMaxThreadCount(1);
    worker_.setExpiryTimeout(-1);
}

RustTrackerGateway::~RustTrackerGateway() {
    // Queued writes still reach the data source, so a stop pressed just before quitting
    // isn't lost. Queued reads are skipped, because nobody waits for their answers.
    stopping_ = true;
    worker_.waitForDone();
    tt_bridge_close(bridge_);
}

void RustTrackerGateway::run(Job job, Callback done, Call call) {
    worker_.start([this, job = std::move(job), done = std::move(done), call]() mutable {
        if (call == Call::Read && stopping_) {
            return;
        }
        const BridgeReply reply = job(bridge_);
        QMetaObject::invokeMethod(this, [done = std::move(done), reply]() { done(reply); }, Qt::QueuedConnection);
    });
}

void RustTrackerGateway::connectTo(const ConnectionTarget &target, Callback done) {
    run([target](Bridge *&bridge) {
        BridgeReply reply;
        if (Bridge *candidate = openChecked(target, &reply)) {
            tt_bridge_close(bridge);
            bridge = candidate;
        }
        return reply;
    }, std::move(done));
}

void RustTrackerGateway::testConnection(const ConnectionTarget &target, Callback done) {
    run([target](Bridge *&) {
        BridgeReply reply;
        tt_bridge_close(openChecked(target, &reply));
        return reply;
    }, std::move(done), Call::Read);
}

void RustTrackerGateway::refreshOverview(const QDateTime &dayStart, const QDateTime &dayEnd, const QDateTime &now,
                                         Callback done) {
    run([start = stamp(dayStart), end = stamp(dayEnd), cutoff = stamp(now)](Bridge *&bridge) {
        BridgeReply refreshed = take(tt_bridge_refresh_resources(bridge));
        if (!refreshed.ok) {
            return refreshed;
        }
        const BridgeReply tasks = take(tt_bridge_tasks(bridge));
        const BridgeReply tracking = take(tt_bridge_tracking(bridge));
        if (!tasks.ok || !tracking.ok) {
            return tasks.ok ? tracking : tasks;
        }
        QJsonObject overview{{QStringLiteral("tasks"), tasks.data}, {QStringLiteral("tracking"), tracking.data}};
        // A failed report leaves today's totals unknown but keeps the task list usable.
        if (take(tt_bridge_refresh_totals(bridge, start.constData(), end.constData(), cutoff.constData())).ok) {
            const BridgeReply report = take(tt_bridge_report_observation(bridge));
            overview.insert(QStringLiteral("report"), report.data);
        }
        return BridgeReply::success(overview);
    }, std::move(done), Call::Read);
}

void RustTrackerGateway::history(const QString &taskId, const QString &cursor, Callback done) {
    run([task = utf8(taskId), page = utf8(cursor)](Bridge *&bridge) {
        return take(tt_bridge_history(bridge, task.constData(), optional(page)));
    }, std::move(done), Call::Read);
}

void RustTrackerGateway::createTask(const QString &name, const QDateTime &at, Callback done) {
    run([text = utf8(name), when = stamp(at)](Bridge *&bridge) {
        return take(tt_bridge_create_task_at(bridge, text.constData(), when.constData()));
    }, std::move(done));
}

void RustTrackerGateway::renameTask(const QString &taskId, const QString &expectedName, const QString &name,
                                    const QDateTime &at, Callback done) {
    run([task = utf8(taskId), expected = utf8(expectedName), text = utf8(name), when = stamp(at)](Bridge *&bridge) {
        return take(tt_bridge_rename_task_with_expected_name_at(bridge, task.constData(), expected.constData(),
                                                                text.constData(), when.constData()));
    }, std::move(done));
}

void RustTrackerGateway::archiveTask(const QString &taskId, const QString &expectedName, const QDateTime &at,
                                     Callback done) {
    run([task = utf8(taskId), expected = utf8(expectedName), when = stamp(at)](Bridge *&bridge) {
        return take(tt_bridge_archive_task_with_expected_name_at(bridge, task.constData(), expected.constData(),
                                                                 when.constData()));
    }, std::move(done));
}

void RustTrackerGateway::unarchiveTask(const QString &taskId, const QDateTime &at, Callback done) {
    run([task = utf8(taskId), when = stamp(at)](Bridge *&bridge) {
        return take(tt_bridge_unarchive_task_at(bridge, task.constData(), when.constData()));
    }, std::move(done));
}

void RustTrackerGateway::previewInactiveTasks(int inactiveDays, const QDateTime &asOf, Callback done) {
    run([days = static_cast<uint32_t>(qMax(0, inactiveDays)), when = stamp(asOf)](Bridge *&bridge) {
        return take(tt_bridge_preview_inactive_tasks_at(bridge, days, when.constData()));
    }, std::move(done), Call::Read);
}

void RustTrackerGateway::archiveInactiveTasks(int inactiveDays, const QDateTime &asOf, Callback done) {
    run([days = static_cast<uint32_t>(qMax(0, inactiveDays)), when = stamp(asOf)](Bridge *&bridge) {
        return take(tt_bridge_archive_inactive_tasks_at(bridge, days, when.constData()));
    }, std::move(done));
}

void RustTrackerGateway::startTracking(const QString &taskId, const QString &expectedActiveWorklogId,
                                       const QDateTime &at, Callback done) {
    run([task = utf8(taskId), expected = utf8(expectedActiveWorklogId), when = stamp(at)](Bridge *&bridge) {
        return take(
            tt_bridge_start_tracking_if_active_at(bridge, task.constData(), optional(expected), when.constData()));
    }, std::move(done));
}

void RustTrackerGateway::stopTracking(const QString &worklogId, const QDateTime &at, Callback done) {
    run([worklog = utf8(worklogId), when = stamp(at)](Bridge *&bridge) {
        return take(tt_bridge_stop_tracking_at(bridge, worklog.constData(), when.constData()));
    }, std::move(done));
}

void RustTrackerGateway::correctWorklog(const QString &worklogId, const QString &expectedStart,
                                        const QString &expectedEnd, const QString &replacementStart,
                                        const QString &replacementEnd, const QDateTime &at, Callback done) {
    run(
        [worklog = utf8(worklogId), oldStart = utf8(expectedStart), oldEnd = utf8(expectedEnd),
         newStart = utf8(replacementStart), newEnd = utf8(replacementEnd), when = stamp(at)](Bridge *&bridge) {
        return take(tt_bridge_correct_worklog_at(bridge, worklog.constData(), oldStart.constData(), optional(oldEnd),
                                                 newStart.constData(), optional(newEnd), when.constData()));
    },
        std::move(done));
}

void RustTrackerGateway::moveCandidates(const QString &sourceTaskId, const QString &query, Callback done) {
    run([task = utf8(sourceTaskId), text = utf8(query)](Bridge *&bridge) {
        return take(tt_bridge_move_candidates(bridge, task.constData(), text.constData()));
    }, std::move(done), Call::Read);
}

void RustTrackerGateway::moveWorklog(const QString &worklogId, const QString &expectedTaskId,
                                     const QString &expectedStart, const QString &expectedEnd,
                                     const QString &destinationTaskId, Callback done) {
    run(
        [worklog = utf8(worklogId), task = utf8(expectedTaskId), start = utf8(expectedStart), end = utf8(expectedEnd),
         destination = utf8(destinationTaskId)](Bridge *&bridge) {
        return take(tt_bridge_move_worklog(bridge, worklog.constData(), task.constData(), start.constData(),
                                           optional(end), destination.constData()));
    },
        std::move(done));
}

void RustTrackerGateway::deleteWorklog(const QString &worklogId, const QString &expectedTaskId,
                                       const QString &expectedStart, const QString &expectedEnd, Callback done) {
    run(
        [worklog = utf8(worklogId), task = utf8(expectedTaskId), start = utf8(expectedStart),
         end = utf8(expectedEnd)](Bridge *&bridge) {
        return take(
            tt_bridge_delete_worklog(bridge, worklog.constData(), task.constData(), start.constData(), optional(end)));
    },
        std::move(done));
}

#include "Contracts/BridgeReply.h"

#include <QJsonDocument>
#include <QJsonObject>

BridgeReply BridgeReply::success(const QJsonValue &data) {
    BridgeReply reply;
    reply.ok = true;
    reply.data = data;
    return reply;
}

BridgeReply BridgeReply::failure(const QString &message, const QString &kind) {
    BridgeReply reply;
    reply.error = message;
    reply.kind = kind;
    return reply;
}

BridgeReply BridgeReply::fromEnvelope(const QByteArray &json) {
    QJsonParseError parseError;
    const QJsonDocument document = QJsonDocument::fromJson(json, &parseError);
    if (parseError.error != QJsonParseError::NoError || !document.isObject()) {
        return failure(QStringLiteral("The tracker returned an unreadable answer"));
    }
    const QJsonObject envelope = document.object();
    if (envelope.contains(QStringLiteral("data"))) {
        return success(envelope.value(QStringLiteral("data")));
    }
    BridgeReply reply = failure(envelope.value(QStringLiteral("error")).toString(QStringLiteral("Unknown error")),
                                envelope.value(QStringLiteral("kind")).toString(QStringLiteral("general")));
    reply.uncertain = envelope.value(QStringLiteral("uncertain")).toBool();
    reply.requiresRefresh = envelope.value(QStringLiteral("requiresRefresh")).toBool();
    return reply;
}

QString BridgeReply::userMessage(const QString &unconfirmed) const {
    return uncertain ? unconfirmed : error;
}

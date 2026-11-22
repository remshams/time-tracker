#pragma once

#include <QJsonValue>
#include <QString>

// One decoded bridge envelope. The bridge answers every call with either
// {"data": ...} or {"error", "kind", "uncertain", "requiresRefresh"}.
struct BridgeReply {
    bool ok = false;
    QJsonValue data;
    QString error;
    QString kind;
    // The server may have committed the write although the client saw a failure.
    bool uncertain = false;
    // The bridge rejects further writes until a refresh succeeds.
    bool requiresRefresh = false;

    static BridgeReply success(const QJsonValue &data = QJsonValue());
    static BridgeReply failure(const QString &message, const QString &kind = QStringLiteral("general"));
    static BridgeReply fromEnvelope(const QByteArray &json);

    // The message a dialog shows. An uncertain write says that it could not be confirmed,
    // because the change may exist on the server.
    QString userMessage(const QString &unconfirmed) const;
};

#include <QJsonArray>
#include <QJsonObject>
#include <QTest>

#include "Contracts/BridgeReply.h"
#include "TestRegistry.h"

class BridgeReplyTests : public QObject {
    Q_OBJECT
private slots:
    void successKeepsTheData() {
        const BridgeReply reply = BridgeReply::success(QJsonObject{{"a", 1}});
        QVERIFY(reply.ok);
        QCOMPARE(reply.data.toObject().value("a").toInt(), 1);
        QVERIFY(!reply.uncertain);
        QVERIFY(!reply.requiresRefresh);
    }

    void successWithoutDataIsNull() {
        const BridgeReply reply = BridgeReply::success();
        QVERIFY(reply.ok);
        QVERIFY(reply.data.isNull());
    }

    void failureDefaultsToTheGeneralKind() {
        const BridgeReply reply = BridgeReply::failure(QStringLiteral("nope"));
        QVERIFY(!reply.ok);
        QCOMPARE(reply.error, QStringLiteral("nope"));
        QCOMPARE(reply.kind, QStringLiteral("general"));
    }

    void failureKeepsAGivenKind() {
        QCOMPARE(BridgeReply::failure(QStringLiteral("x"), QStringLiteral("conflict")).kind,
                 QStringLiteral("conflict"));
    }

    void envelopeWithDataSucceeds() {
        const BridgeReply reply = BridgeReply::fromEnvelope(R"({"data":{"tasks":[1,2]}})");
        QVERIFY(reply.ok);
        QCOMPARE(reply.data.toObject().value("tasks").toArray().size(), 2);
    }

    void envelopeWithNullDataStillSucceeds() {
        const BridgeReply reply = BridgeReply::fromEnvelope(R"({"data":null})");
        QVERIFY(reply.ok);
        QVERIFY(reply.data.isNull());
    }

    void dataWinsOverAnErrorField() { QVERIFY(BridgeReply::fromEnvelope(R"({"data":1,"error":"x"})").ok); }

    void envelopeWithAnErrorReadsEveryField() {
        const BridgeReply reply = BridgeReply::fromEnvelope(
            R"({"error":"Server said no","kind":"conflict","uncertain":true,"requiresRefresh":true})");
        QVERIFY(!reply.ok);
        QCOMPARE(reply.error, QStringLiteral("Server said no"));
        QCOMPARE(reply.kind, QStringLiteral("conflict"));
        QVERIFY(reply.uncertain);
        QVERIFY(reply.requiresRefresh);
    }

    void errorFlagsCanBeSetSeparately() {
        const BridgeReply uncertain = BridgeReply::fromEnvelope(R"({"error":"x","uncertain":true})");
        QVERIFY(uncertain.uncertain);
        QVERIFY(!uncertain.requiresRefresh);
        const BridgeReply refresh = BridgeReply::fromEnvelope(R"({"error":"x","requiresRefresh":true})");
        QVERIFY(!refresh.uncertain);
        QVERIFY(refresh.requiresRefresh);
    }

    void envelopeWithoutDetailsGetsDefaults() {
        const BridgeReply reply = BridgeReply::fromEnvelope("{}");
        QVERIFY(!reply.ok);
        QCOMPARE(reply.error, QStringLiteral("Unknown error"));
        QCOMPARE(reply.kind, QStringLiteral("general"));
        QVERIFY(!reply.uncertain);
        QVERIFY(!reply.requiresRefresh);
    }

    void unreadableJsonFails() {
        const BridgeReply reply = BridgeReply::fromEnvelope("{not json");
        QVERIFY(!reply.ok);
        QCOMPARE(reply.error, QStringLiteral("The tracker returned an unreadable answer"));
        QCOMPARE(reply.kind, QStringLiteral("general"));
    }

    void emptyInputFails() { QVERIFY(!BridgeReply::fromEnvelope(QByteArray()).ok); }

    void anEnvelopeThatIsNotAnObjectFails() {
        const BridgeReply reply = BridgeReply::fromEnvelope("[1,2]");
        QVERIFY(!reply.ok);
        QCOMPARE(reply.error, QStringLiteral("The tracker returned an unreadable answer"));
        QVERIFY(!BridgeReply::fromEnvelope("\"text\"").ok);
    }

    void userMessageShowsTheErrorForACertainFailure() {
        const BridgeReply reply = BridgeReply::failure(QStringLiteral("Task not found"));
        QCOMPARE(reply.userMessage(QStringLiteral("Could not confirm")), QStringLiteral("Task not found"));
    }

    void userMessageSaysUnconfirmedForAnUncertainWrite() {
        BridgeReply reply = BridgeReply::failure(QStringLiteral("timeout"));
        reply.uncertain = true;
        QCOMPARE(reply.userMessage(QStringLiteral("Could not confirm")), QStringLiteral("Could not confirm"));
    }
};

REGISTER_TEST(BridgeReplyTests)
#include "BridgeReplyTests.moc"

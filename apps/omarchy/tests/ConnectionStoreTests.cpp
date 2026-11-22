#include <QSettings>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>

#include "FakeTrackerGateway.h"
#include "Features/Connection/ConnectionStore.h"
#include "TestRegistry.h"

namespace {

// A settings file in a temporary directory, so no test touches the user's settings.
struct Fixture {
    QTemporaryDir dir;
    FakeTrackerGateway gateway;
    QSettings settings{dir.filePath("settings.ini"), QSettings::IniFormat};
    ConnectionStore store{&gateway, &settings};

    QSettings saved() const { return QSettings(dir.filePath("settings.ini"), QSettings::IniFormat); }
};

} // namespace

class ConnectionStoreTests : public QObject {
    Q_OBJECT
private slots:
    // Saved settings

    void startsOnTheLocalDatabaseWithoutSettings() {
        Fixture f;
        QVERIFY(!f.store.remote());
        QVERIFY(f.store.serverUrl().isEmpty());
        QCOMPARE(f.store.target(), ConnectionTarget());
        QCOMPARE(f.store.status(), QStringLiteral("connecting"));
        QVERIFY(f.store.statusMessage().isEmpty());
        QVERIFY(!f.store.draftRemote());
        QVERIFY(f.store.draftServerUrl().isEmpty());
        QVERIFY(!f.store.draftPending());
        QVERIFY(f.store.draftResult().isEmpty());
        QVERIFY(!f.store.draftResultOk());
    }

    void loadsTheSavedTarget() {
        QTemporaryDir dir;
        {
            QSettings seed(dir.filePath("settings.ini"), QSettings::IniFormat);
            seed.setValue("connection/remote", true);
            seed.setValue("connection/serverUrl", "http://100.64.0.1:8080");
        }
        FakeTrackerGateway gateway;
        QSettings settings(dir.filePath("settings.ini"), QSettings::IniFormat);
        ConnectionStore store(&gateway, &settings);
        QVERIFY(store.remote());
        QCOMPARE(store.serverUrl(), QStringLiteral("http://100.64.0.1:8080"));
        // The draft starts as a copy of the saved target.
        QVERIFY(store.draftRemote());
        QCOMPARE(store.draftServerUrl(), QStringLiteral("http://100.64.0.1:8080"));
    }

    // Connecting the saved target

    void connectSavedOpensTheSavedLocalTarget() {
        Fixture f;
        QSignalSpy connected(&f.store, &ConnectionStore::connected);
        QSignalSpy status(&f.store, &ConnectionStore::statusChanged);
        f.store.connectSaved();
        QCOMPARE(f.gateway.lastArgs("connectTo"), QStringList({"local", ""}));
        QCOMPARE(f.store.status(), QStringLiteral("connecting"));
        QCOMPARE(status.count(), 0);
        f.gateway.respond("connectTo", BridgeReply::success());
        QCOMPARE(f.store.status(), QStringLiteral("connected"));
        QCOMPARE(status.count(), 1);
        QCOMPARE(connected.count(), 1);
    }

    void connectSavedOpensTheSavedServer() {
        QTemporaryDir dir;
        QSettings seed(dir.filePath("settings.ini"), QSettings::IniFormat);
        seed.setValue("connection/remote", true);
        seed.setValue("connection/serverUrl", "https://tracker.example:9000");
        seed.sync();
        FakeTrackerGateway gateway;
        QSettings settings(dir.filePath("settings.ini"), QSettings::IniFormat);
        ConnectionStore store(&gateway, &settings);
        store.connectSaved();
        QCOMPARE(gateway.lastArgs("connectTo"), QStringList({"remote", "https://tracker.example:9000"}));
    }

    void aFailedConnectSavedMarksTheSourceUnavailable() {
        Fixture f;
        QSignalSpy connected(&f.store, &ConnectionStore::connected);
        f.store.connectSaved();
        f.gateway.respond("connectTo", BridgeReply::failure("Database is locked"));
        QCOMPARE(f.store.status(), QStringLiteral("unavailable"));
        QCOMPARE(f.store.statusMessage(), QStringLiteral("Database is locked"));
        QCOMPARE(connected.count(), 0);
    }

    void connectSavedAfterAFailureShowsConnectingAgain() {
        Fixture f;
        f.store.connectSaved();
        f.gateway.respond("connectTo", BridgeReply::failure("down"));
        f.store.connectSaved();
        QCOMPARE(f.store.status(), QStringLiteral("connecting"));
        QVERIFY(f.store.statusMessage().isEmpty());
        f.gateway.respond("connectTo", BridgeReply::success());
        QCOMPARE(f.store.status(), QStringLiteral("connected"));
    }

    void connectSavedDoesNotChangeTheSavedSettings() {
        Fixture f;
        f.store.connectSaved();
        f.gateway.respond("connectTo", BridgeReply::success());
        QVERIFY(!f.saved().contains("connection/remote"));
    }

    void aConnectSavedAnswerAfterTheStoreIsGoneIsIgnored() {
        FakeTrackerGateway gateway;
        QTemporaryDir dir;
        QSettings settings(dir.filePath("settings.ini"), QSettings::IniFormat);
        auto *store = new ConnectionStore(&gateway, &settings);
        store->connectSaved();
        delete store;
        QVERIFY(gateway.respond("connectTo", BridgeReply::success()));
    }

    // Status from the sync

    void reachabilityTogglesTheStatus() {
        Fixture f;
        QSignalSpy status(&f.store, &ConnectionStore::statusChanged);
        f.store.markReachable();
        QCOMPARE(f.store.status(), QStringLiteral("connected"));
        QCOMPARE(status.count(), 1);
        f.store.markReachable();
        QCOMPARE(status.count(), 1);
        f.store.markUnavailable("Server unreachable");
        QCOMPARE(f.store.status(), QStringLiteral("unavailable"));
        QCOMPARE(f.store.statusMessage(), QStringLiteral("Server unreachable"));
        QCOMPARE(status.count(), 2);
        f.store.markUnavailable("Server unreachable");
        QCOMPARE(status.count(), 2);
        f.store.markUnavailable("Another message");
        QCOMPARE(status.count(), 3);
        f.store.markReachable();
        QVERIFY(f.store.statusMessage().isEmpty());
        QCOMPARE(status.count(), 4);
    }

    // The draft

    void changingTheDraftClearsTheLastResult() {
        Fixture f;
        f.store.setDraftServerUrl("http://a:1");
        f.store.testDraft();
        f.gateway.respond("testConnection", BridgeReply::failure("refused"));
        QCOMPARE(f.store.draftResult(), QStringLiteral("refused"));
        QSignalSpy draft(&f.store, &ConnectionStore::draftChanged);
        f.store.setDraftRemote(true);
        QVERIFY(f.store.draftRemote());
        QVERIFY(f.store.draftResult().isEmpty());
        QCOMPARE(draft.count(), 1);
        f.store.testDraft();
        f.gateway.respond("testConnection", BridgeReply::failure("refused"));
        f.store.setDraftServerUrl("http://b:2");
        QCOMPARE(f.store.draftServerUrl(), QStringLiteral("http://b:2"));
        QVERIFY(f.store.draftResult().isEmpty());
    }

    void settingTheSameDraftValueDoesNothing() {
        Fixture f;
        QSignalSpy draft(&f.store, &ConnectionStore::draftChanged);
        f.store.setDraftRemote(false);
        f.store.setDraftServerUrl(QString());
        QCOMPARE(draft.count(), 0);
        f.store.setDraftServerUrl("http://a:1");
        f.store.setDraftServerUrl("http://a:1");
        QCOMPARE(draft.count(), 1);
    }

    void editingTheDraftDoesNotChangeTheTarget() {
        Fixture f;
        f.store.setDraftRemote(true);
        f.store.setDraftServerUrl("http://a:1");
        QVERIFY(!f.store.remote());
        QVERIFY(f.store.serverUrl().isEmpty());
    }

    void resetDraftCopiesTheTargetAndClearsTheResult() {
        Fixture f;
        f.store.setDraftRemote(true);
        f.store.setDraftServerUrl("http://a:1");
        f.store.testDraft();
        QSignalSpy draft(&f.store, &ConnectionStore::draftChanged);
        f.store.resetDraft();
        QVERIFY(!f.store.draftRemote());
        QVERIFY(f.store.draftServerUrl().isEmpty());
        QVERIFY(!f.store.draftPending());
        QVERIFY(f.store.draftResult().isEmpty());
        QVERIFY(!f.store.draftResultOk());
        QCOMPARE(draft.count(), 1);
    }

    void resetDraftClearsASuccessResult() {
        Fixture f;
        f.store.testDraft();
        f.gateway.respond("testConnection", BridgeReply::success());
        QVERIFY(f.store.draftResultOk());
        f.store.resetDraft();
        QVERIFY(!f.store.draftResultOk());
        QVERIFY(f.store.draftResult().isEmpty());
    }

    // Test connection

    void testDraftChecksTheDraftWithoutChangingTheConnection() {
        Fixture f;
        f.store.setDraftRemote(true);
        f.store.setDraftServerUrl("http://a:1");
        QSignalSpy draft(&f.store, &ConnectionStore::draftChanged);
        f.store.testDraft();
        QCOMPARE(f.gateway.lastArgs("testConnection"), QStringList({"remote", "http://a:1"}));
        QVERIFY(f.store.draftPending());
        QCOMPARE(draft.count(), 1);
        f.gateway.respond("testConnection", BridgeReply::success());
        QVERIFY(!f.store.draftPending());
        QVERIFY(f.store.draftResultOk());
        QCOMPARE(f.store.draftResult(), QStringLiteral("Connection works"));
        QCOMPARE(draft.count(), 2);
        QCOMPARE(f.gateway.count("connectTo"), 0);
        QVERIFY(!f.store.remote());
        QVERIFY(!f.saved().contains("connection/remote"));
    }

    void aFailedTestShowsTheError() {
        Fixture f;
        f.store.testDraft();
        f.gateway.respond("testConnection", BridgeReply::failure("Connection refused"));
        QVERIFY(!f.store.draftPending());
        QVERIFY(!f.store.draftResultOk());
        QCOMPARE(f.store.draftResult(), QStringLiteral("Connection refused"));
    }

    void aSecondTestWaitsForTheFirst() {
        Fixture f;
        f.store.testDraft();
        f.store.testDraft();
        f.store.applyDraft();
        QCOMPARE(f.gateway.calls.size(), 1);
    }

    void aNewTestClearsAnOlderResultWhileItRuns() {
        Fixture f;
        f.store.testDraft();
        f.gateway.respond("testConnection", BridgeReply::failure("Connection refused"));
        f.store.testDraft();
        QVERIFY(f.store.draftResult().isEmpty());
        QVERIFY(f.store.draftPending());
    }

    void aTestAnswerAfterTheStoreIsGoneIsIgnored() {
        FakeTrackerGateway gateway;
        QTemporaryDir dir;
        QSettings settings(dir.filePath("settings.ini"), QSettings::IniFormat);
        auto *store = new ConnectionStore(&gateway, &settings);
        store->testDraft();
        delete store;
        QVERIFY(gateway.respond("testConnection", BridgeReply::success()));
    }

    // Apply

    void applyConnectsAndSavesTheNewTarget() {
        Fixture f;
        f.store.setDraftRemote(true);
        f.store.setDraftServerUrl("http://a:1");
        QSignalSpy target(&f.store, &ConnectionStore::targetChanged);
        QSignalSpy connected(&f.store, &ConnectionStore::connected);
        QSignalSpy status(&f.store, &ConnectionStore::statusChanged);
        f.store.applyDraft();
        QCOMPARE(f.gateway.lastArgs("connectTo"), QStringList({"remote", "http://a:1"}));
        QVERIFY(f.store.draftPending());
        // Nothing changes before the connection works.
        QVERIFY(!f.store.remote());
        QCOMPARE(target.count(), 0);
        f.gateway.respond("connectTo", BridgeReply::success());
        QVERIFY(f.store.remote());
        QCOMPARE(f.store.serverUrl(), QStringLiteral("http://a:1"));
        QCOMPARE(target.count(), 1);
        QCOMPARE(connected.count(), 1);
        QCOMPARE(status.count(), 1);
        QCOMPARE(f.store.status(), QStringLiteral("connected"));
        QVERIFY(!f.store.draftPending());
        QVERIFY(f.store.draftResultOk());
        QCOMPARE(f.store.draftResult(), QStringLiteral("Connected"));
        const QSettings saved = f.saved();
        QCOMPARE(saved.value("connection/remote").toBool(), true);
        QCOMPARE(saved.value("connection/serverUrl").toString(), QStringLiteral("http://a:1"));
    }

    void aSavedTargetSurvivesARestart() {
        QTemporaryDir dir;
        FakeTrackerGateway gateway;
        {
            QSettings settings(dir.filePath("settings.ini"), QSettings::IniFormat);
            ConnectionStore store(&gateway, &settings);
            store.setDraftRemote(true);
            store.setDraftServerUrl("http://a:1");
            store.applyDraft();
            gateway.respond("connectTo", BridgeReply::success());
        }
        QSettings settings(dir.filePath("settings.ini"), QSettings::IniFormat);
        ConnectionStore again(&gateway, &settings);
        QVERIFY(again.remote());
        QCOMPARE(again.serverUrl(), QStringLiteral("http://a:1"));
    }

    void aFailedApplyKeepsTheOldTargetAndSettings() {
        QTemporaryDir dir;
        {
            QSettings seed(dir.filePath("settings.ini"), QSettings::IniFormat);
            seed.setValue("connection/remote", true);
            seed.setValue("connection/serverUrl", "http://old:1");
        }
        FakeTrackerGateway gateway;
        QSettings settings(dir.filePath("settings.ini"), QSettings::IniFormat);
        ConnectionStore store(&gateway, &settings);
        store.markReachable();
        store.setDraftServerUrl("http://new:2");
        QSignalSpy target(&store, &ConnectionStore::targetChanged);
        QSignalSpy connected(&store, &ConnectionStore::connected);
        QSignalSpy status(&store, &ConnectionStore::statusChanged);
        store.applyDraft();
        gateway.respond("connectTo", BridgeReply::failure("Connection refused"));
        QCOMPARE(store.serverUrl(), QStringLiteral("http://old:1"));
        QVERIFY(store.remote());
        QCOMPARE(target.count(), 0);
        QCOMPARE(connected.count(), 0);
        QCOMPARE(status.count(), 0);
        QCOMPARE(store.status(), QStringLiteral("connected"));
        QVERIFY(!store.draftPending());
        QVERIFY(!store.draftResultOk());
        QCOMPARE(store.draftResult(), QStringLiteral("Connection refused"));
        // The draft keeps what the user typed so they can fix it.
        QCOMPARE(store.draftServerUrl(), QStringLiteral("http://new:2"));
        const QSettings saved(dir.filePath("settings.ini"), QSettings::IniFormat);
        QCOMPARE(saved.value("connection/serverUrl").toString(), QStringLiteral("http://old:1"));
    }

    void applyUsesTheDraftAsItWasWhenTheUserPressedConnect() {
        Fixture f;
        f.store.setDraftRemote(true);
        f.store.setDraftServerUrl("http://a:1");
        f.store.applyDraft();
        f.store.setDraftServerUrl("http://b:2");
        f.gateway.respond("connectTo", BridgeReply::success());
        QCOMPARE(f.store.serverUrl(), QStringLiteral("http://a:1"));
    }

    void applyBackToLocalSavesLocal() {
        QTemporaryDir dir;
        {
            QSettings seed(dir.filePath("settings.ini"), QSettings::IniFormat);
            seed.setValue("connection/remote", true);
            seed.setValue("connection/serverUrl", "http://old:1");
        }
        FakeTrackerGateway gateway;
        QSettings settings(dir.filePath("settings.ini"), QSettings::IniFormat);
        ConnectionStore store(&gateway, &settings);
        store.setDraftRemote(false);
        store.applyDraft();
        QCOMPARE(gateway.lastArgs("connectTo").first(), QStringLiteral("local"));
        gateway.respond("connectTo", BridgeReply::success());
        QVERIFY(!store.remote());
        const QSettings saved(dir.filePath("settings.ini"), QSettings::IniFormat);
        QCOMPARE(saved.value("connection/remote").toBool(), false);
    }

    void aSecondApplyWaitsForTheFirst() {
        Fixture f;
        f.store.applyDraft();
        f.store.applyDraft();
        f.store.testDraft();
        QCOMPARE(f.gateway.calls.size(), 1);
    }

    void aNewApplyClearsTheOldResult() {
        Fixture f;
        f.store.applyDraft();
        f.gateway.respond("connectTo", BridgeReply::failure("Connection refused"));
        f.store.applyDraft();
        QVERIFY(f.store.draftResult().isEmpty());
        QVERIFY(f.store.draftPending());
    }

    void anApplyAnswerAfterTheStoreIsGoneIsIgnored() {
        FakeTrackerGateway gateway;
        QTemporaryDir dir;
        QSettings settings(dir.filePath("settings.ini"), QSettings::IniFormat);
        auto *store = new ConnectionStore(&gateway, &settings);
        store->applyDraft();
        delete store;
        QVERIFY(gateway.respond("connectTo", BridgeReply::success()));
        QVERIFY(!QSettings(dir.filePath("settings.ini"), QSettings::IniFormat).contains("connection/remote"));
    }
};

REGISTER_TEST(ConnectionStoreTests)
#include "ConnectionStoreTests.moc"

#pragma once

#include <QObject>
#include <QSettings>

#include "Contracts/TrackerGateway.h"

// The data source the app uses and the Settings draft for changing it. The saved choice
// changes only after a connection to the new target succeeded; a failure keeps the
// previous source. A server never falls back to the local database.
class ConnectionStore : public QObject {
    Q_OBJECT
    Q_PROPERTY(bool remote READ remote NOTIFY targetChanged)
    Q_PROPERTY(QString serverUrl READ serverUrl NOTIFY targetChanged)
    Q_PROPERTY(QString status READ status NOTIFY statusChanged)
    Q_PROPERTY(QString statusMessage READ statusMessage NOTIFY statusChanged)
    Q_PROPERTY(bool draftRemote READ draftRemote WRITE setDraftRemote NOTIFY draftChanged)
    Q_PROPERTY(QString draftServerUrl READ draftServerUrl WRITE setDraftServerUrl NOTIFY draftChanged)
    Q_PROPERTY(bool draftPending READ draftPending NOTIFY draftChanged)
    Q_PROPERTY(QString draftResult READ draftResult NOTIFY draftChanged)
    Q_PROPERTY(bool draftResultOk READ draftResultOk NOTIFY draftChanged)
public:
    // `settings` stores the chosen target between launches.
    ConnectionStore(TrackerGateway *gateway, QSettings *settings, QObject *parent = nullptr);

    ConnectionTarget target() const { return target_; }
    bool remote() const { return target_.remote; }
    QString serverUrl() const { return target_.serverUrl; }
    // "connecting", "connected" or "unavailable".
    QString status() const { return status_; }
    QString statusMessage() const { return statusMessage_; }

    bool draftRemote() const { return draft_.remote; }
    void setDraftRemote(bool remote);
    QString draftServerUrl() const { return draft_.serverUrl; }
    void setDraftServerUrl(const QString &url);
    bool draftPending() const { return draftPending_; }
    QString draftResult() const { return draftResult_; }
    bool draftResultOk() const { return draftResultOk_; }

    // Opens the saved target. Emits connected() on success.
    void connectSaved();
    // The sync reports whether the latest read reached the data source.
    void markReachable();
    void markUnavailable(const QString &message);

    // Copies the current target into the draft and clears the last result.
    Q_INVOKABLE void resetDraft();
    Q_INVOKABLE void testDraft();
    // Connects to the draft target and saves it once the connection works.
    Q_INVOKABLE void applyDraft();

signals:
    void targetChanged();
    void statusChanged();
    void draftChanged();
    // The app now reads from `target()`: after launch or after a change in Settings.
    void connected();

private:
    void setStatus(const QString &status, const QString &message);
    void finishDraft(bool ok, const QString &message);
    void save();

    TrackerGateway *gateway_;
    QSettings *settings_;
    ConnectionTarget target_;
    ConnectionTarget draft_;
    QString status_ = QStringLiteral("connecting");
    QString statusMessage_;
    bool draftPending_ = false;
    QString draftResult_;
    bool draftResultOk_ = false;
};

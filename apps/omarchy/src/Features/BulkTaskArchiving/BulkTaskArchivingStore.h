#pragma once

#include <QDateTime>
#include <QObject>
#include <QVariantList>

#include "Contracts/BridgeReply.h"

class DisplayClock;
class TrackerGateway;

// The "archive inactive tasks" dialog. Every change of the inactivity period loads one
// preview, and a generation counter drops the answers to older requests. Confirming
// sends the period and the time of the preview that the user saw; the core archives
// whatever is eligible for them and reports the actual count. A failed confirmation
// needs a fresh preview, because the bridge discards the old one when it confirms.
class BulkTaskArchivingStore : public QObject {
    Q_OBJECT
    Q_PROPERTY(QString daysText READ daysText WRITE setDaysText NOTIFY daysTextChanged)
    Q_PROPERTY(bool loading READ loading NOTIFY stateChanged)
    Q_PROPERTY(bool previewLoaded READ previewLoaded NOTIFY stateChanged)
    // Each entry has id, name and colorName.
    Q_PROPERTY(QVariantList tasks READ tasks NOTIFY stateChanged)
    Q_PROPERTY(bool pending READ pending NOTIFY stateChanged)
    Q_PROPERTY(QString error READ error NOTIFY stateChanged)
    // The count the core reported, or -1 before a confirmation succeeded.
    Q_PROPERTY(int archivedCount READ archivedCount NOTIFY stateChanged)
    Q_PROPERTY(bool canSubmit READ canSubmit NOTIFY stateChanged)
public:
    static constexpr int kDefaultDays = 14;

    BulkTaskArchivingStore(TrackerGateway *gateway, DisplayClock *clock, QObject *parent = nullptr);

    QString daysText() const { return daysText_; }
    void setDaysText(const QString &text);
    bool loading() const { return loading_; }
    bool previewLoaded() const { return previewLoaded_; }
    QVariantList tasks() const { return tasks_; }
    bool pending() const { return pending_; }
    QString error() const { return error_; }
    int archivedCount() const { return archivedCount_; }
    // Only the preview that is on screen can be confirmed, and only when it has tasks.
    bool canSubmit() const { return previewLoaded_ && !tasks_.isEmpty() && !pending_ && archivedCount_ < 0; }

    // A positive whole number of days, or 0 for any other text.
    static int parseDays(const QString &text);

    // Starts with the default period and loads its preview.
    Q_INVOKABLE void begin();
    // Loads a new preview for the current period.
    Q_INVOKABLE void refresh();
    // Drops the preview and ignores answers that are still on their way.
    Q_INVOKABLE void cancel();
    Q_INVOKABLE void submit();

signals:
    void daysTextChanged();
    void stateChanged();
    // A write finished, successfully or not; the task list must reload.
    void refreshRequested();

private:
    void loadPreview(int days);
    void clearPreview();
    void acceptPreview(quint64 generation, int days, const QDateTime &asOf, const BridgeReply &reply);
    void finish(const BridgeReply &reply);

    TrackerGateway *gateway_;
    DisplayClock *clock_;
    QString daysText_;
    quint64 generation_ = 0;
    bool loading_ = false;
    bool previewLoaded_ = false;
    int previewDays_ = 0;
    QDateTime previewAsOf_;
    QVariantList tasks_;
    bool pending_ = false;
    QString error_;
    int archivedCount_ = -1;
};

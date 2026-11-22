#pragma once

#include <QObject>
#include <QVariantMap>
#include <optional>

#include "Contracts/Models.h"
#include "Features/TaskCatalog/TaskListModel.h"

class DisplayClock;

// The confirmed task list, the sidebar tab, its filter and the selected task.
// Each tab remembers its own selection, like the macOS app.
class TaskCatalogStore : public QObject {
    Q_OBJECT
    Q_PROPERTY(bool loaded READ loaded NOTIFY tasksChanged)
    Q_PROPERTY(bool archivedTab READ archivedTab WRITE setArchivedTab NOTIFY tabChanged)
    Q_PROPERTY(QString filterText READ filterText WRITE setFilterText NOTIFY filterTextChanged)
    Q_PROPERTY(TaskListModel *model READ model CONSTANT)
    Q_PROPERTY(int activeCount READ activeCount NOTIFY tasksChanged)
    // Task names by ID. QML reads names through this map, so bindings update after a rename.
    Q_PROPERTY(QVariantMap names READ names NOTIFY tasksChanged)
    Q_PROPERTY(QString selectedTaskId READ selectedTaskId NOTIFY selectionChanged)
    Q_PROPERTY(QString selectedName READ selectedName NOTIFY selectionChanged)
    Q_PROPERTY(bool selectedArchived READ selectedArchived NOTIFY selectionChanged)
    Q_PROPERTY(QString selectedColorName READ selectedColorName NOTIFY selectionChanged)
public:
    explicit TaskCatalogStore(DisplayClock *clock, QObject *parent = nullptr);

    // Takes in a confirmed task list from a refresh.
    void accept(const QList<TaskItem> &tasks);

    bool loaded() const { return loaded_; }
    bool archivedTab() const { return archivedTab_; }
    void setArchivedTab(bool archived);
    QString filterText() const { return filterText_; }
    void setFilterText(const QString &text);
    TaskListModel *model() { return &model_; }
    int activeCount() const;
    QVariantMap names() const;

    QString selectedTaskId() const { return selectedId(); }
    QString selectedName() const;
    bool selectedArchived() const;
    QString selectedColorName() const;

    const QList<TaskItem> &tasks() const { return tasks_; }
    std::optional<TaskItem> task(const QString &taskId) const;
    QString nameOf(const QString &taskId) const;
    Q_INVOKABLE QString colorNameOf(const QString &taskId) const;

    // Selects a task in its tab, switching tabs when needed.
    Q_INVOKABLE void select(const QString &taskId);
    // Moves the selection by `step` rows in the current tab, stopping at the ends.
    Q_INVOKABLE void moveSelection(int step);

    // Tasks without logged time follow logged ones. Equal times fall back to name, then ID.
    static QList<TaskItem> ordered(QList<TaskItem> tasks);
    // Case-insensitive subsequence match, like the terminal client's search.
    static bool matches(const QString &name, const QString &query);

signals:
    void tasksChanged();
    void tabChanged();
    void filterTextChanged();
    void selectionChanged();

private:
    QString selectedId() const { return archivedTab_ ? archivedSelection_ : activeSelection_; }
    void setSelectedId(const QString &taskId);
    void rebuild();

    DisplayClock *clock_;
    QList<TaskItem> tasks_;
    bool loaded_ = false;
    bool archivedTab_ = false;
    QString filterText_;
    QString activeSelection_;
    QString archivedSelection_;
    TaskListModel model_;
};

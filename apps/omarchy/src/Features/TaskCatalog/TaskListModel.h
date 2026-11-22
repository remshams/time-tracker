#pragma once

#include <QAbstractListModel>
#include <QList>

struct TaskRow {
    QString id;
    QString name;
    QString colorName;
    QString lastWorked;
    bool archived = false;

    bool operator==(const TaskRow &other) const = default;
};

// The rows of one sidebar tab, in display order.
class TaskListModel : public QAbstractListModel {
    Q_OBJECT
    Q_PROPERTY(int count READ rowCount NOTIFY countChanged)
public:
    enum Role { IdRole = Qt::UserRole + 1, NameRole, ColorNameRole, LastWorkedRole, ArchivedRole };

    using QAbstractListModel::QAbstractListModel;

    int rowCount(const QModelIndex &parent = QModelIndex()) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

    // Replaces the rows. Leaves the model alone when nothing changed, so views keep their state.
    void setRows(const QList<TaskRow> &rows);
    const QList<TaskRow> &rows() const { return rows_; }
    Q_INVOKABLE int indexOf(const QString &taskId) const;

signals:
    void countChanged();

private:
    QList<TaskRow> rows_;
};

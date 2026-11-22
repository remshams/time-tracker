#include "Features/TaskCatalog/TaskListModel.h"

int TaskListModel::rowCount(const QModelIndex &parent) const {
    return parent.isValid() ? 0 : static_cast<int>(rows_.size());
}

QVariant TaskListModel::data(const QModelIndex &index, int role) const {
    if (!index.isValid() || index.row() >= rows_.size()) {
        return {};
    }
    const TaskRow &row = rows_.at(index.row());
    switch (role) {
    case IdRole:
        return row.id;
    case NameRole:
        return row.name;
    case ColorNameRole:
        return row.colorName;
    case LastWorkedRole:
        return row.lastWorked;
    case ArchivedRole:
        return row.archived;
    default:
        return {};
    }
}

QHash<int, QByteArray> TaskListModel::roleNames() const {
    return {{IdRole, "taskId"},
            {NameRole, "name"},
            {ColorNameRole, "colorName"},
            {LastWorkedRole, "lastWorked"},
            {ArchivedRole, "archived"}};
}

void TaskListModel::setRows(const QList<TaskRow> &rows) {
    if (rows == rows_) {
        return;
    }
    const bool sameShape = rows.size() == rows_.size() &&
                           std::equal(rows.cbegin(), rows.cend(), rows_.cbegin(),
                                      [](const TaskRow &left, const TaskRow &right) { return left.id == right.id; });
    if (sameShape) {
        // Same tasks in the same order: update in place so the list keeps its scroll and focus.
        rows_ = rows;
        emit dataChanged(index(0), index(static_cast<int>(rows_.size()) - 1));
        return;
    }
    beginResetModel();
    rows_ = rows;
    endResetModel();
}

int TaskListModel::indexOf(const QString &taskId) const {
    for (int row = 0; row < rows_.size(); ++row) {
        if (rows_.at(row).id == taskId) {
            return row;
        }
    }
    return -1;
}

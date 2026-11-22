#include "Features/TaskCatalog/TaskCatalogStore.h"

#include <algorithm>

#include "Shared/DisplayClock.h"
#include "Shared/Format.h"
#include "Shared/TaskColor.h"

TaskCatalogStore::TaskCatalogStore(DisplayClock *clock, QObject *parent) : QObject(parent), clock_(clock) {
    // "Today 13:12" becomes "Yesterday" at midnight.
    connect(clock_, &DisplayClock::dayChanged, this, &TaskCatalogStore::rebuild);
}

void TaskCatalogStore::accept(const QList<TaskItem> &tasks) {
    const QString before = selectedId();
    const QString nameBefore = selectedName();
    const bool archivedBefore = selectedArchived();
    tasks_ = ordered(tasks);
    loaded_ = true;
    rebuild();
    emit tasksChanged();
    if (selectedId() == before && (selectedName() != nameBefore || selectedArchived() != archivedBefore)) {
        emit selectionChanged();
    }
}

void TaskCatalogStore::reset() {
    tasks_.clear();
    loaded_ = false;
    activeSelection_.clear();
    archivedSelection_.clear();
    rebuild();
    emit tasksChanged();
    emit selectionChanged();
}

void TaskCatalogStore::rememberForActive(const QString &taskId) {
    if (archivedTab_) {
        activeSelection_ = taskId;
    }
}

void TaskCatalogStore::setArchivedTab(bool archived) {
    if (archivedTab_ == archived) {
        return;
    }
    archivedTab_ = archived;
    rebuild();
    emit tabChanged();
    emit selectionChanged();
}

void TaskCatalogStore::setFilterText(const QString &text) {
    if (filterText_ == text) {
        return;
    }
    filterText_ = text;
    rebuild();
    emit filterTextChanged();
}

int TaskCatalogStore::activeCount() const {
    return static_cast<int>(
        std::count_if(tasks_.cbegin(), tasks_.cend(), [](const TaskItem &task) { return !task.archived; }));
}

QVariantMap TaskCatalogStore::names() const {
    QVariantMap result;
    for (const TaskItem &item : tasks_) {
        result.insert(item.id, item.name);
    }
    return result;
}

QString TaskCatalogStore::selectedName() const {
    return nameOf(selectedId());
}

bool TaskCatalogStore::selectedArchived() const {
    const std::optional<TaskItem> selected = task(selectedId());
    return selected && selected->archived;
}

QString TaskCatalogStore::selectedLastWorked() const {
    const std::optional<TaskItem> selected = task(selectedId());
    if (!selected || !selected->latestStart) {
        return {};
    }
    return Format::lastWorked(selected->latestStart, clock_->now().toLocalTime().date());
}

QString TaskCatalogStore::selectedColorName() const {
    return selectedId().isEmpty() ? QString() : TaskColor::nameFor(selectedId());
}

std::optional<TaskItem> TaskCatalogStore::task(const QString &taskId) const {
    for (const TaskItem &item : tasks_) {
        if (item.id == taskId) {
            return item;
        }
    }
    return std::nullopt;
}

QString TaskCatalogStore::nameOf(const QString &taskId) const {
    const std::optional<TaskItem> item = task(taskId);
    return item ? item->name : QString();
}

QString TaskCatalogStore::colorNameOf(const QString &taskId) const {
    return taskId.isEmpty() ? QString() : TaskColor::nameFor(taskId);
}

void TaskCatalogStore::select(const QString &taskId) {
    const std::optional<TaskItem> item = task(taskId);
    if (!item) {
        return;
    }
    if (item->archived != archivedTab_) {
        // Remember the task in its own tab before switching, so the switch lands on it.
        (item->archived ? archivedSelection_ : activeSelection_) = taskId;
        if (!filterText_.isEmpty() && !matches(item->name, filterText_)) {
            filterText_.clear();
            emit filterTextChanged();
        }
        archivedTab_ = item->archived;
        rebuild();
        emit tabChanged();
        emit selectionChanged();
        return;
    }
    if (!filterText_.isEmpty() && !matches(item->name, filterText_)) {
        filterText_.clear();
        rebuild();
        emit filterTextChanged();
    }
    setSelectedId(taskId);
}

void TaskCatalogStore::moveSelection(int step) {
    const QList<TaskRow> &rows = model_.rows();
    if (rows.isEmpty()) {
        return;
    }
    const int current = model_.indexOf(selectedId());
    const int target = current < 0 ? 0 : std::clamp(current + step, 0, static_cast<int>(rows.size()) - 1);
    setSelectedId(rows.at(target).id);
}

QList<TaskItem> TaskCatalogStore::ordered(QList<TaskItem> tasks) {
    std::stable_sort(tasks.begin(), tasks.end(), [](const TaskItem &left, const TaskItem &right) {
        if (left.latestStart && right.latestStart && *left.latestStart != *right.latestStart) {
            return *left.latestStart > *right.latestStart;
        }
        if (left.latestStart.has_value() != right.latestStart.has_value()) {
            return left.latestStart.has_value();
        }
        if (left.name != right.name) {
            return left.name < right.name;
        }
        return left.id < right.id;
    });
    return tasks;
}

bool TaskCatalogStore::matches(const QString &name, const QString &query) {
    qsizetype position = 0;
    for (const QChar character : query) {
        if (character.isSpace()) {
            continue;
        }
        position = name.indexOf(character, position, Qt::CaseInsensitive);
        if (position < 0) {
            return false;
        }
        ++position;
    }
    return true;
}

void TaskCatalogStore::setSelectedId(const QString &taskId) {
    if (selectedId() == taskId) {
        return;
    }
    (archivedTab_ ? archivedSelection_ : activeSelection_) = taskId;
    emit selectionChanged();
}

void TaskCatalogStore::rebuild() {
    const QDate today = clock_->now().toLocalTime().date();
    QList<TaskRow> rows;
    for (const TaskItem &item : tasks_) {
        if (item.archived != archivedTab_ || !matches(item.name, filterText_)) {
            continue;
        }
        rows.append({item.id, item.name, TaskColor::nameFor(item.id), Format::lastWorked(item.latestStart, today),
                     item.archived});
    }
    model_.setRows(rows);
    // Keep the selection on a visible task: the same task if it is still in the tab,
    // otherwise the first row.
    if (model_.indexOf(selectedId()) < 0) {
        setSelectedId(rows.isEmpty() ? QString() : rows.first().id);
    }
}

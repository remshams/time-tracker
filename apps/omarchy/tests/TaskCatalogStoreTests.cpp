#include <QSignalSpy>
#include <QTest>

#include "Features/TaskCatalog/TaskCatalogStore.h"
#include "Shared/TaskColor.h"
#include "TestRegistry.h"
#include "TestSupport.h"

using namespace TestSupport;

namespace {

TaskItem item(const QString &id, const QString &name, bool archived = false, const QString &latest = QString()) {
    TaskItem task;
    task.id = id;
    task.name = name;
    task.archived = archived;
    if (!latest.isEmpty()) {
        task.latestStart = utc(latest);
    }
    return task;
}

QStringList ids(const QList<TaskItem> &tasks) {
    QStringList result;
    for (const TaskItem &task : tasks) {
        result << task.id;
    }
    return result;
}

QStringList rowIds(TaskCatalogStore &store) {
    QStringList result;
    for (const TaskRow &row : store.model()->rows()) {
        result << row.id;
    }
    return result;
}

} // namespace

class TaskCatalogStoreTests : public QObject {
    Q_OBJECT
private slots:
    // Ordering

    void ordersLatestFirst() {
        const QList<TaskItem> sorted = TaskCatalogStore::ordered({item("old", "A", false, "2026-11-01T09:00:00.000Z"),
                                                                  item("new", "B", false, "2026-11-09T09:00:00.000Z"),
                                                                  item("mid", "C", false, "2026-11-05T09:00:00.000Z")});
        QCOMPARE(ids(sorted), QStringList({"new", "mid", "old"}));
    }

    void ordersTasksWithoutTimeAfterTrackedOnesInEitherInputOrder() {
        QCOMPARE(ids(TaskCatalogStore::ordered(
                     {item("none", "A"), item("tracked", "Z", false, "2026-11-01T09:00:00.000Z")})),
                 QStringList({"tracked", "none"}));
        QCOMPARE(ids(TaskCatalogStore::ordered(
                     {item("tracked", "Z", false, "2026-11-01T09:00:00.000Z"), item("none", "A")})),
                 QStringList({"tracked", "none"}));
    }

    void ordersEqualTimesByNameThenId() {
        const QString same = QStringLiteral("2026-11-01T09:00:00.000Z");
        QCOMPARE(ids(TaskCatalogStore::ordered({item("x", "Beta", false, same), item("y", "Alpha", false, same)})),
                 QStringList({"y", "x"}));
        QCOMPARE(ids(TaskCatalogStore::ordered({item("2", "Same", false, same), item("1", "Same", false, same)})),
                 QStringList({"1", "2"}));
    }

    void ordersUntrackedTasksByNameThenId() {
        QCOMPARE(ids(TaskCatalogStore::ordered({item("3", "Beta"), item("2", "Alpha"), item("1", "Alpha")})),
                 QStringList({"1", "2", "3"}));
    }

    // Filter

    void matchesASubsequenceIgnoringCase() {
        QVERIFY(TaskCatalogStore::matches("Write the report", "wtr"));
        QVERIFY(TaskCatalogStore::matches("Write the report", "WRITE"));
        QVERIFY(TaskCatalogStore::matches("Write the report", "report"));
        QVERIFY(TaskCatalogStore::matches("abc", "abc"));
    }

    void matchesNeedsTheLettersInOrder() {
        QVERIFY(!TaskCatalogStore::matches("abc", "ba"));
        QVERIFY(!TaskCatalogStore::matches("abc", "abd"));
        QVERIFY(!TaskCatalogStore::matches("a", "aa"));
        QVERIFY(TaskCatalogStore::matches("aba", "aa"));
    }

    void matchesIgnoresSpacesInTheQuery() {
        QVERIFY(TaskCatalogStore::matches("Write report", "w r"));
        QVERIFY(TaskCatalogStore::matches("abc", "  "));
        QVERIFY(!TaskCatalogStore::matches("abc", " d "));
    }

    void anEmptyQueryMatchesEverything() {
        QVERIFY(TaskCatalogStore::matches("anything", QString()));
        QVERIFY(TaskCatalogStore::matches(QString(), QString()));
        QVERIFY(!TaskCatalogStore::matches(QString(), "a"));
    }

    // Accepting tasks

    void startsEmptyAndNotLoaded() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        QVERIFY(!store.loaded());
        QCOMPARE(store.activeCount(), 0);
        QVERIFY(store.model()->rows().isEmpty());
        QVERIFY(store.selectedTaskId().isEmpty());
        QVERIFY(store.selectedName().isEmpty());
        QVERIFY(!store.selectedArchived());
        QVERIFY(store.selectedColorName().isEmpty());
        QVERIFY(!store.archivedTab());
    }

    void acceptLoadsTasksInOrderAndSelectsTheFirst() {
        TestClock test("2026-11-10T12:00:00.000Z");
        TaskCatalogStore store(test.clock());
        QSignalSpy changed(&store, &TaskCatalogStore::tasksChanged);
        QSignalSpy selection(&store, &TaskCatalogStore::selectionChanged);
        store.accept({item("b", "Beta"), item("a", "Alpha", false, "2026-11-10T09:00:00.000Z"),
                      item("c", "Old", true, "2026-11-01T09:00:00.000Z")});
        QVERIFY(store.loaded());
        QCOMPARE(changed.count(), 1);
        QCOMPARE(selection.count(), 1);
        QCOMPARE(store.activeCount(), 2);
        QCOMPARE(rowIds(store), QStringList({"a", "b"}));
        QCOMPARE(store.selectedTaskId(), QStringLiteral("a"));
        QCOMPARE(store.selectedName(), QStringLiteral("Alpha"));
        QCOMPARE(store.selectedColorName(), TaskColor::nameFor("a"));
        QCOMPARE(store.tasks().size(), 3);
    }

    void rowsCarryColorAndLastWorkedText() {
        TestClock test("2026-11-10T12:00:00.000Z");
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha", false, "2026-11-10T09:12:00.000Z"), item("b", "Beta")});
        const QList<TaskRow> rows = store.model()->rows();
        QCOMPARE(rows.at(0).colorName, TaskColor::nameFor("a"));
        QCOMPARE(rows.at(0).lastWorked, QStringLiteral("Today 10:12"));
        QCOMPARE(rows.at(1).lastWorked, QStringLiteral("Never"));
        QVERIFY(!rows.at(0).archived);
    }

    void aNewDayRewritesRelativeDates() {
        TestClock test("2026-11-10T12:00:00.000Z");
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha", false, "2026-11-10T09:12:00.000Z")});
        QCOMPARE(store.model()->rows().first().lastWorked, QStringLiteral("Today 10:12"));
        test.advanceTo("2026-11-11T12:00:00.000Z");
        QCOMPARE(store.model()->rows().first().lastWorked, QStringLiteral("Yesterday"));
    }

    void taskLooksUpByIdAndNameOfFallsBackToEmpty() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha")});
        QVERIFY(store.task("a"));
        QVERIFY(!store.task("missing"));
        QCOMPARE(store.nameOf("a"), QStringLiteral("Alpha"));
        QVERIFY(store.nameOf("missing").isEmpty());
        QCOMPARE(store.colorNameOf("a"), TaskColor::nameFor("a"));
        QVERIFY(store.colorNameOf(QString()).isEmpty());
    }

    // QML reads names through this map, so a rename must change it and announce it.
    void namesFollowEveryAcceptedTaskList() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        QSignalSpy changed(&store, &TaskCatalogStore::tasksChanged);
        store.accept({item("a", "Alpha"), item("b", "Beta", true)});
        QCOMPARE(store.names(), (QVariantMap{{"a", "Alpha"}, {"b", "Beta"}}));
        store.accept({item("a", "Renamed"), item("b", "Beta", true)});
        QCOMPARE(store.names().value("a").toString(), QStringLiteral("Renamed"));
        QCOMPARE(changed.count(), 2);
    }

    void acceptKeepsTheSelectionWhenTheTaskStays() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha"), item("b", "Beta")});
        store.select("b");
        QSignalSpy selection(&store, &TaskCatalogStore::selectionChanged);
        store.accept({item("c", "Aardvark"), item("a", "Alpha"), item("b", "Beta")});
        QCOMPARE(store.selectedTaskId(), QStringLiteral("b"));
        QCOMPARE(selection.count(), 0);
    }

    void acceptFallsBackToTheFirstRowWhenTheSelectionDisappears() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha"), item("b", "Beta")});
        store.select("b");
        QSignalSpy selection(&store, &TaskCatalogStore::selectionChanged);
        store.accept({item("a", "Alpha"), item("c", "Gamma")});
        QCOMPARE(store.selectedTaskId(), QStringLiteral("a"));
        QCOMPARE(selection.count(), 1);
    }

    void acceptWithNoTasksClearsTheSelection() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha")});
        store.accept({});
        QVERIFY(store.loaded());
        QVERIFY(store.selectedTaskId().isEmpty());
        QVERIFY(store.model()->rows().isEmpty());
    }

    void renamingTheSelectedTaskAnnouncesTheSelectionOnce() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha")});
        QSignalSpy selection(&store, &TaskCatalogStore::selectionChanged);
        store.accept({item("a", "Alpha two")});
        QCOMPARE(selection.count(), 1);
        QCOMPARE(store.selectedName(), QStringLiteral("Alpha two"));
        store.accept({item("a", "Alpha two")});
        QCOMPARE(selection.count(), 1);
    }

    void archivingTheSelectedTaskMovesTheSelectionOnce() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha"), item("b", "Beta")});
        QSignalSpy selection(&store, &TaskCatalogStore::selectionChanged);
        store.accept({item("a", "Alpha", true), item("b", "Beta")});
        QCOMPARE(store.selectedTaskId(), QStringLiteral("b"));
        QCOMPARE(selection.count(), 1);
    }

    // Tabs

    void tabsShowTheirOwnTasks() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha"), item("o", "Old", true)});
        QSignalSpy tab(&store, &TaskCatalogStore::tabChanged);
        QSignalSpy selection(&store, &TaskCatalogStore::selectionChanged);
        store.setArchivedTab(true);
        QVERIFY(store.archivedTab());
        QCOMPARE(rowIds(store), QStringList({"o"}));
        QCOMPARE(store.selectedTaskId(), QStringLiteral("o"));
        QVERIFY(store.selectedArchived());
        QCOMPARE(tab.count(), 1);
        QVERIFY(selection.count() >= 1);
        store.setArchivedTab(false);
        QCOMPARE(rowIds(store), QStringList({"a"}));
        QCOMPARE(store.selectedTaskId(), QStringLiteral("a"));
        QVERIFY(!store.selectedArchived());
    }

    void settingTheSameTabDoesNothing() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha")});
        QSignalSpy tab(&store, &TaskCatalogStore::tabChanged);
        QSignalSpy selection(&store, &TaskCatalogStore::selectionChanged);
        store.setArchivedTab(false);
        QCOMPARE(tab.count(), 0);
        QCOMPARE(selection.count(), 0);
    }

    void eachTabRemembersItsSelection() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept(
            {item("a1", "Alpha"), item("a2", "Beta"), item("o1", "Old one", true), item("o2", "Old two", true)});
        store.select("a2");
        store.setArchivedTab(true);
        QCOMPARE(store.selectedTaskId(), QStringLiteral("o1"));
        store.select("o2");
        store.setArchivedTab(false);
        QCOMPARE(store.selectedTaskId(), QStringLiteral("a2"));
        store.setArchivedTab(true);
        QCOMPARE(store.selectedTaskId(), QStringLiteral("o2"));
    }

    void anEmptyTabHasNoSelection() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha")});
        store.setArchivedTab(true);
        QVERIFY(store.selectedTaskId().isEmpty());
        QVERIFY(store.selectedName().isEmpty());
        QVERIFY(store.selectedColorName().isEmpty());
    }

    // Filter

    void filteringHidesRowsAndKeepsTheSelectionOnAVisibleTask() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha"), item("b", "Beta"), item("c", "Gamma")});
        QSignalSpy filter(&store, &TaskCatalogStore::filterTextChanged);
        store.setFilterText("bt");
        QCOMPARE(filter.count(), 1);
        QCOMPARE(store.filterText(), QStringLiteral("bt"));
        QCOMPARE(rowIds(store), QStringList({"b"}));
        QCOMPARE(store.selectedTaskId(), QStringLiteral("b"));
        // Clearing the filter keeps the task selected.
        store.setFilterText(QString());
        QCOMPARE(rowIds(store), QStringList({"a", "b", "c"}));
        QCOMPARE(store.selectedTaskId(), QStringLiteral("b"));
    }

    void settingTheSameFilterDoesNothing() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha")});
        store.setFilterText("al");
        QSignalSpy filter(&store, &TaskCatalogStore::filterTextChanged);
        store.setFilterText("al");
        QCOMPARE(filter.count(), 0);
    }

    void aFilterWithoutMatchesEmptiesTheListAndTheSelection() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha")});
        store.setFilterText("zzz");
        QVERIFY(store.model()->rows().isEmpty());
        QVERIFY(store.selectedTaskId().isEmpty());
    }

    // Selecting

    void selectIgnoresUnknownTasks() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha")});
        QSignalSpy selection(&store, &TaskCatalogStore::selectionChanged);
        store.select("missing");
        QCOMPARE(selection.count(), 0);
        QCOMPARE(store.selectedTaskId(), QStringLiteral("a"));
    }

    void selectChoosesATaskInTheCurrentTab() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha"), item("b", "Beta")});
        QSignalSpy selection(&store, &TaskCatalogStore::selectionChanged);
        QSignalSpy tab(&store, &TaskCatalogStore::tabChanged);
        store.select("b");
        QCOMPARE(store.selectedTaskId(), QStringLiteral("b"));
        QCOMPARE(selection.count(), 1);
        QCOMPARE(tab.count(), 0);
        store.select("b");
        QCOMPARE(selection.count(), 1);
    }

    void selectClearsAFilterThatHidesTheTask() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha"), item("b", "Beta")});
        store.setFilterText("alp");
        QSignalSpy filter(&store, &TaskCatalogStore::filterTextChanged);
        store.select("b");
        QVERIFY(store.filterText().isEmpty());
        QCOMPARE(filter.count(), 1);
        QCOMPARE(store.selectedTaskId(), QStringLiteral("b"));
        QCOMPARE(rowIds(store), QStringList({"a", "b"}));
    }

    void selectKeepsAFilterThatMatchesTheTask() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha"), item("b", "Alphabet"), item("c", "Gamma")});
        store.setFilterText("alp");
        QSignalSpy filter(&store, &TaskCatalogStore::filterTextChanged);
        store.select("b");
        QCOMPARE(store.filterText(), QStringLiteral("alp"));
        QCOMPARE(filter.count(), 0);
        QCOMPARE(store.selectedTaskId(), QStringLiteral("b"));
    }

    void selectSwitchesToTheTabOfTheTask() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha"), item("o1", "Old one", true), item("o2", "Old two", true)});
        QSignalSpy tab(&store, &TaskCatalogStore::tabChanged);
        QSignalSpy selection(&store, &TaskCatalogStore::selectionChanged);
        store.select("o2");
        QVERIFY(store.archivedTab());
        QCOMPARE(store.selectedTaskId(), QStringLiteral("o2"));
        QCOMPARE(tab.count(), 1);
        QVERIFY(selection.count() >= 1);
        QCOMPARE(rowIds(store), QStringList({"o1", "o2"}));
        // And back, to the task the active tab remembered.
        store.select("a");
        QVERIFY(!store.archivedTab());
        QCOMPARE(store.selectedTaskId(), QStringLiteral("a"));
    }

    void selectInAnotherTabClearsAFilterThatHidesTheTask() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha"), item("o", "Old", true)});
        store.setFilterText("alp");
        QSignalSpy filter(&store, &TaskCatalogStore::filterTextChanged);
        store.select("o");
        QVERIFY(store.archivedTab());
        QVERIFY(store.filterText().isEmpty());
        QCOMPARE(filter.count(), 1);
        QCOMPARE(store.selectedTaskId(), QStringLiteral("o"));
    }

    void selectInAnotherTabKeepsAFilterThatMatchesTheTask() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha"), item("o", "Old alpha", true)});
        store.setFilterText("alp");
        QSignalSpy filter(&store, &TaskCatalogStore::filterTextChanged);
        store.select("o");
        QCOMPARE(store.filterText(), QStringLiteral("alp"));
        QCOMPARE(filter.count(), 0);
        QCOMPARE(store.selectedTaskId(), QStringLiteral("o"));
    }

    void selectInAnotherTabWithoutAFilterKeepsTheFilterEmpty() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha"), item("o", "Old", true)});
        QSignalSpy filter(&store, &TaskCatalogStore::filterTextChanged);
        store.select("o");
        QCOMPARE(filter.count(), 0);
    }

    // Moving the selection

    void moveSelectionStepsThroughTheRows() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha"), item("b", "Beta"), item("c", "Gamma")});
        store.moveSelection(1);
        QCOMPARE(store.selectedTaskId(), QStringLiteral("b"));
        store.moveSelection(1);
        QCOMPARE(store.selectedTaskId(), QStringLiteral("c"));
        store.moveSelection(-1);
        QCOMPARE(store.selectedTaskId(), QStringLiteral("b"));
    }

    void moveSelectionStopsAtTheEnds() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha"), item("b", "Beta")});
        QSignalSpy selection(&store, &TaskCatalogStore::selectionChanged);
        store.moveSelection(-1);
        QCOMPARE(store.selectedTaskId(), QStringLiteral("a"));
        QCOMPARE(selection.count(), 0);
        store.moveSelection(5);
        QCOMPARE(store.selectedTaskId(), QStringLiteral("b"));
        store.moveSelection(1);
        QCOMPARE(store.selectedTaskId(), QStringLiteral("b"));
        QCOMPARE(selection.count(), 1);
        store.moveSelection(-9);
        QCOMPARE(store.selectedTaskId(), QStringLiteral("a"));
    }

    void moveSelectionWithoutRowsDoesNothing() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        QSignalSpy selection(&store, &TaskCatalogStore::selectionChanged);
        store.moveSelection(1);
        QCOMPARE(selection.count(), 0);
        QVERIFY(store.selectedTaskId().isEmpty());
    }

    void moveSelectionFollowsTheFilteredRows() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha"), item("b", "Beta"), item("c", "Alpine")});
        store.setFilterText("alp");
        QCOMPARE(store.selectedTaskId(), QStringLiteral("a"));
        store.moveSelection(1);
        QCOMPARE(store.selectedTaskId(), QStringLiteral("c"));
    }

    // The model

    void theModelStaysUntouchedWhenNothingChanged() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        const QList<TaskItem> tasks{item("a", "Alpha"), item("b", "Beta")};
        store.accept(tasks);
        QSignalSpy reset(store.model(), &QAbstractItemModel::modelReset);
        QSignalSpy data(store.model(), &QAbstractItemModel::dataChanged);
        QSignalSpy count(store.model(), &TaskListModel::countChanged);
        store.accept(tasks);
        QCOMPARE(reset.count(), 0);
        QCOMPARE(data.count(), 0);
        QCOMPARE(count.count(), 0);
    }

    void aRenameUpdatesTheRowsInPlace() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha"), item("b", "Beta")});
        QSignalSpy reset(store.model(), &QAbstractItemModel::modelReset);
        QSignalSpy data(store.model(), &QAbstractItemModel::dataChanged);
        store.accept({item("a", "Alpha"), item("b", "Beta renamed")});
        QCOMPARE(reset.count(), 0);
        QCOMPARE(data.count(), 1);
        QCOMPARE(store.model()->rows().at(1).name, QStringLiteral("Beta renamed"));
    }

    void aDifferentSetOfRowsResetsTheModel() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha"), item("b", "Beta")});
        QSignalSpy reset(store.model(), &QAbstractItemModel::modelReset);
        QSignalSpy count(store.model(), &TaskListModel::countChanged);
        store.accept({item("a", "Alpha"), item("b", "Beta"), item("c", "Gamma")});
        QCOMPARE(reset.count(), 1);
        QCOMPARE(count.count(), 1);
        // The same number of rows with other tasks is a reset without a count change.
        store.accept({item("a", "Alpha"), item("b", "Beta"), item("d", "Delta")});
        QCOMPARE(reset.count(), 2);
        QCOMPARE(count.count(), 1);
    }

    void aDifferentOrderResetsTheModel() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha", false, "2026-11-01T09:00:00.000Z"),
                      item("b", "Beta", false, "2026-11-02T09:00:00.000Z")});
        QSignalSpy reset(store.model(), &QAbstractItemModel::modelReset);
        store.accept({item("a", "Alpha", false, "2026-11-03T09:00:00.000Z"),
                      item("b", "Beta", false, "2026-11-02T09:00:00.000Z")});
        QCOMPARE(reset.count(), 1);
    }

    void modelExposesEveryRole() {
        TestClock test("2026-11-10T12:00:00.000Z");
        TaskCatalogStore store(test.clock());
        store.accept({item("o", "Old", true, "2026-11-10T09:12:00.000Z")});
        store.setArchivedTab(true);
        TaskListModel *model = store.model();
        QCOMPARE(model->rowCount(), 1);
        QCOMPARE(model->property("count").toInt(), 1);
        const QModelIndex index = model->index(0);
        QCOMPARE(model->data(index, TaskListModel::IdRole).toString(), QStringLiteral("o"));
        QCOMPARE(model->data(index, TaskListModel::NameRole).toString(), QStringLiteral("Old"));
        QCOMPARE(model->data(index, TaskListModel::ColorNameRole).toString(), TaskColor::nameFor("o"));
        QCOMPARE(model->data(index, TaskListModel::LastWorkedRole).toString(), QStringLiteral("Today 10:12"));
        QCOMPARE(model->data(index, TaskListModel::ArchivedRole).toBool(), true);
        QVERIFY(!model->data(index, Qt::UserRole + 99).isValid());
        QVERIFY(!model->data(QModelIndex(), TaskListModel::IdRole).isValid());
        QVERIFY(!model->data(model->index(0).siblingAtRow(3), TaskListModel::IdRole).isValid());
    }

    void modelNamesItsRolesForQml() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        const QHash<int, QByteArray> names = store.model()->roleNames();
        QCOMPARE(names.value(TaskListModel::IdRole), QByteArray("taskId"));
        QCOMPARE(names.value(TaskListModel::NameRole), QByteArray("name"));
        QCOMPARE(names.value(TaskListModel::ColorNameRole), QByteArray("colorName"));
        QCOMPARE(names.value(TaskListModel::LastWorkedRole), QByteArray("lastWorked"));
        QCOMPARE(names.value(TaskListModel::ArchivedRole), QByteArray("archived"));
    }

    void modelFindsRowsAndIsFlat() {
        TestClock test;
        TaskCatalogStore store(test.clock());
        store.accept({item("a", "Alpha"), item("b", "Beta")});
        TaskListModel *model = store.model();
        QCOMPARE(model->indexOf("a"), 0);
        QCOMPARE(model->indexOf("b"), 1);
        QCOMPARE(model->indexOf("missing"), -1);
        QCOMPARE(model->rowCount(model->index(0)), 0);
    }
};

REGISTER_TEST(TaskCatalogStoreTests)
#include "TaskCatalogStoreTests.moc"

import XCTest
@testable import TrackerClient

final class MenuTaskSelectionTests: XCTestCase {
    func testEmptyMenuHasNoSelectionAndNavigationDoesNothing() {
        var selection = MenuTaskSelection(taskIDs: [], initialTaskID: "missing")
        selection.moveUp()
        selection.moveDown()
        selection.select(taskID: "missing")
        XCTAssertNil(selection.selectedTaskID)
    }

    func testInitialSelectionUsesTheRunningTaskAndFallsBackToTheFirstTask() {
        let ids = ["first", "running", "last"]
        XCTAssertEqual(MenuTaskSelection(taskIDs: ids, initialTaskID: "running").selectedTaskID, "running")
        XCTAssertEqual(MenuTaskSelection(taskIDs: ids, initialTaskID: "missing").selectedTaskID, "first")
        XCTAssertEqual(MenuTaskSelection(taskIDs: ids).selectedTaskID, "first")
    }

    func testNavigationStopsAtTheFirstAndLastTask() {
        var selection = MenuTaskSelection(taskIDs: ["first", "middle", "last"])
        selection.moveUp()
        XCTAssertEqual(selection.selectedTaskID, "first")
        selection.moveDown()
        XCTAssertEqual(selection.selectedTaskID, "middle")
        selection.moveDown()
        selection.moveDown()
        XCTAssertEqual(selection.selectedTaskID, "last")
        selection.moveUp()
        XCTAssertEqual(selection.selectedTaskID, "middle")
        selection.moveUp()
        selection.moveUp()
        XCTAssertEqual(selection.selectedTaskID, "first")
    }

    func testSelectionRejectsIDsOutsideTheMenuAndRemovesDuplicateTasks() {
        var selection = MenuTaskSelection(taskIDs: ["first", "second", "first"])
        XCTAssertEqual(selection.taskIDs, ["first", "second"])
        selection.select(taskID: "second")
        XCTAssertEqual(selection.selectedTaskID, "second")
        selection.select(taskID: "missing")
        XCTAssertEqual(selection.selectedTaskID, "second")
        selection.moveDown()
        XCTAssertEqual(selection.selectedTaskID, "second")
    }
}

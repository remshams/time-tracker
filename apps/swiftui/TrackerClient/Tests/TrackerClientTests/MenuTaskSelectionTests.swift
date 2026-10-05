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

    func testKeyboardScrollingDoesNotSelectTheRowUnderAStationaryPointer() {
        var selection = MenuTaskSelection(taskIDs: ["first", "second", "third"])
        selection.hover(taskID: "first", inside: true)
        selection.moveDown()
        selection.hover(taskID: "first", inside: false)
        selection.hover(taskID: "third", inside: true)
        XCTAssertEqual(selection.selectedTaskID, "second")
        selection.moveDown()
        XCTAssertEqual(selection.selectedTaskID, "third")
        selection.hover(taskID: "first", inside: true)
        XCTAssertEqual(selection.selectedTaskID, "third")
        selection.pointerMoved()
        XCTAssertEqual(selection.selectedTaskID, "first")
        selection.hover(taskID: "second", inside: true)
        XCTAssertEqual(selection.selectedTaskID, "second")
    }

    func testPointerExitCannotClearAnotherHoveredTaskOrSelectAnUnknownTask() {
        var selection = MenuTaskSelection(taskIDs: ["first", "second"])
        selection.moveDown()
        selection.hover(taskID: "first", inside: true)
        selection.hover(taskID: "second", inside: false)
        selection.pointerMoved()
        XCTAssertEqual(selection.selectedTaskID, "first")
        selection.hover(taskID: "first", inside: false)
        selection.select(taskID: "second")
        selection.pointerMoved()
        XCTAssertEqual(selection.selectedTaskID, "second")
        selection.hover(taskID: "unknown", inside: true)
        selection.pointerMoved()
        XCTAssertEqual(selection.selectedTaskID, "second")
    }
}

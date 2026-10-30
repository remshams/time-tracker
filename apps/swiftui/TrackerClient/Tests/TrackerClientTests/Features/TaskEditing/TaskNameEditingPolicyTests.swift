import Foundation
import XCTest
@testable import TrackerClient

final class TaskNameEditingPolicyTests: XCTestCase {
    @MainActor
    func testWhitespaceTrimmingPreservesZeroWidthAndInteriorWhitespace() {
        XCTAssertEqual(TaskNameEditingPolicy.normalized(""), "")
        XCTAssertEqual(TaskNameEditingPolicy.normalized("Task name"), "Task name")
        XCTAssertEqual(
            TaskNameEditingPolicy.normalized(
                " \t\n\r\u{0085}\u{00a0}\u{1680}\u{2000}\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}"), "")
        XCTAssertEqual(
            TaskNameEditingPolicy.normalized(" \t\u{200b} Task\nname \u{200b}\u{00a0}"),
            "\u{200b} Task\nname \u{200b}")
        XCTAssertEqual(TaskNameEditingPolicy.normalized("\u{200b}"), "\u{200b}")
        XCTAssertEqual(TaskNameEditingPolicy.normalized("\u{feff}"), "\u{feff}")
    }

    @MainActor
    func testCreationTreatsRustWhitespaceAsEmptyButPreservesValidZeroWidthName() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start()
        fixture.session.openTaskCreation()
        fixture.session.setTaskCreationName("\u{00a0}\u{2000}\u{3000}")
        XCTAssertFalse(fixture.session.taskCreation.canSubmit)
        fixture.session.setTaskCreationName("\u{200b}")
        XCTAssertTrue(fixture.session.taskCreation.canSubmit)
        fixture.session.submitTaskCreation()
        let creation = try await fixture.client.next()
        XCTAssertEqual(creation.operation, .create(name: "\u{200b}", at: "2025-01-01T00:00:00.000Z"))
        let task = TaskItem(id: "zero-width-task", name: "\u{200b}", archived: false, latestStart: nil)
        creation.created(taskID: task.id, resources: TaskListResources(tasks: [task], active: nil))
        let history = try await fixture.client.next()
        history.succeed(emptyPage)
        try await fixture.settled()
        XCTAssertEqual(fixture.session.selectedTask?.name, task.name)
        XCTAssertFalse(fixture.session.taskCreation.isPresented)
    }

    @MainActor
    func testRenamePreservesZeroWidthNameEdges() async throws {
        let fixture = Fixture()
        defer { fixture.cleanup() }
        try await fixture.start(TaskListResources(tasks: [firstTask], active: nil))
        fixture.session.openTaskRename()
        let rawName = " \u{200b}Renamed task\u{200b}\u{00a0}"
        fixture.session.setTaskRenameName(rawName)
        fixture.session.submitTaskRename()
        let rename = try await fixture.client.next()
        XCTAssertEqual(
            rename.operation,
            .rename(task: firstTask.id, expectedName: firstTask.name, name: rawName, at: "2025-01-01T00:00:00.000Z")
        )
        let task = TaskItem(id: firstTask.id, name: "\u{200b}Renamed task\u{200b}", archived: false, latestStart: nil)
        try await fixture.update("the renamed task") {
            rename.succeed(TaskListResources(tasks: [task], active: nil))
        }
        XCTAssertFalse(fixture.session.taskRename.isPresented)
        XCTAssertNil(fixture.session.taskRename.error)
        XCTAssertEqual(fixture.session.selectedTask?.name, task.name)
    }
}

import AppKit
import XCTest

@MainActor
extension TrackerUITestCase {
    func assertRunning(_ task: FixtureTask, file: StaticString = #filePath, line: UInt = #line) {
        waitUntil("The backend tracks \(task.name)", file: file, line: line) {
            (try? self.fixture.activeWorklog())?.taskID == task.id
        }
    }

    func assertStopped(file: StaticString = #filePath, line: UInt = #line) {
        waitUntil("The backend timer stops", file: file, line: line) {
            do { return try self.fixture.activeWorklog() == nil } catch { return false }
        }
    }
}

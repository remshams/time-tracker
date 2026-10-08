import XCTest

class ConnectionUITestCase: TrackerUITestCase {}

final class ConnectionUITests: ConnectionUITestCase {
    func testTestingServerDoesNotAdoptItAndConnectingPersistsAfterRelaunch() throws {
        let local = try fixture.create("Local planning")
        let remote = try TrackerFixture(source: .server)
        addTeardownBlock { try remote.cleanup() }
        let serverTask = try remote.create("Server planning")
        launch()
        XCTAssertTrue(element("task-sidebar.task.\(local.id)").waitForExistence(timeout: timeout))
        openConnectionSettings()
        chooseConnection("Server", endpoint: remote.serverURL!)
        app.buttons["connection.test"].click()
        waitUntil("Test succeeds without adopting server") {
            self.elementText("connection.result").contains("Connection succeeded.")
        }
        XCTAssertTrue(element("task-sidebar.task.\(local.id)").exists)
        XCTAssertFalse(element("task-sidebar.task.\(serverTask.id)").exists)
        app.buttons["connection.connect"].click()
        waitUntil("Connect finishes") { self.elementText("connection.result").contains("Connected.") }
        closeConnectionSettings()
        XCTAssertTrue(element("task-sidebar.task.\(serverTask.id)").waitForExistence(timeout: timeout))
        XCTAssertFalse(element("task-sidebar.task.\(local.id)").exists)
        app.terminate()
        launch()
        assertConnectionStatus("Connected")
        XCTAssertTrue(element("task-sidebar.task.\(serverTask.id)").exists)
        XCTAssertEqual(try fixture.tasks().map(\.id), [local.id])
        XCTAssertEqual(try remote.tasks().map(\.id), [serverTask.id])
    }

    func testSwitchingSourcesClearsPreviousHistoryAndTotalsWithoutCopyingData() throws {
        let local = try fixture.create("Local completed work")
        let now = Date()
        let localLog = try fixture.completed(local, start: now.addingTimeInterval(-7200), end: now.addingTimeInterval(-3600))
        let remote = try TrackerFixture(source: .server)
        addTeardownBlock { try remote.cleanup() }
        let serverTask = try remote.create("Server without history")
        launch()
        element("task-sidebar.task.\(local.id)").click()
        XCTAssertTrue(element("worklog-history.row.\(localLog.id)").waitForExistence(timeout: timeout))
        connectTo(remote.serverURL!)
        element("task-sidebar.task.\(serverTask.id)").click()
        XCTAssertTrue(element("worklog-history.empty").waitForExistence(timeout: timeout))
        XCTAssertFalse(element("worklog-history.row.\(localLog.id)").exists)
        connectTo(nil)
        element("task-sidebar.task.\(local.id)").click()
        XCTAssertTrue(element("worklog-history.row.\(localLog.id)").waitForExistence(timeout: timeout))
        XCTAssertFalse(element("task-sidebar.task.\(serverTask.id)").exists)
        XCTAssertEqual(try fixture.worklogs(local), [localLog])
        XCTAssertTrue(try remote.worklogs(serverTask).isEmpty)
    }

    func testInvalidUnreachableAndIncompatibleCandidatesPreserveLocalConnection() throws {
        let local = try fixture.create("Preserved local task")
        let remote = try TrackerFixture(source: .server)
        addTeardownBlock { try remote.cleanup() }
        let proxy = try remote.enableProxy()
        launch()
        openConnectionSettings()
        chooseConnection("Server", endpoint: "")
        XCTAssertFalse(app.buttons["connection.connect"].isEnabled)
        XCTAssertFalse(app.buttons["connection.test"].isEnabled)
        for endpoint in ["not a URL", "http://127.0.0.1:\(try TrackerFixture.availablePort())"] {
            chooseConnection("Server", endpoint: endpoint)
            app.buttons["connection.connect"].click()
            waitUntil("Invalid candidate reports failure") {
                self.element("connection.result").exists && self.app.buttons["connection.connect"].isEnabled
            }
            XCTAssertFalse(element("connection.result").label.contains("Connected."))
        }
        try proxy.arm(method: "GET", path: "/v1/health", mode: .invalidProtocol)
        chooseConnection("Server", endpoint: proxy.endpoint)
        app.buttons["connection.connect"].click()
        waitUntil("Incompatible candidate reports failure") {
            self.element("connection.result").exists && self.app.buttons["connection.connect"].isEnabled
        }
        XCTAssertFalse(element("connection.result").label.contains("Connected."))
        closeConnectionSettings()
        app.terminate()
        launch()
        assertConnectionStatus("Local database")
        XCTAssertTrue(element("task-sidebar.task.\(local.id)").exists)
    }

    func testUnavailableConfiguredServerNeverFallsBackToLocalStorage() throws {
        let local = try fixture.create("Private local task")
        let remote = try TrackerFixture(source: .server)
        addTeardownBlock { try remote.cleanup() }
        try fixture.saveConnection(serverURL: remote.serverURL!)
        remote.stopServer()
        launch()
        assertConnectionStatus("Unavailable")
        XCTAssertFalse(element("task-sidebar.task.\(local.id)").exists)
        XCTAssertFalse(app.buttons["New task"].isEnabled)
        XCTAssertEqual(try fixture.tasks().map(\.id), [local.id])
    }
}

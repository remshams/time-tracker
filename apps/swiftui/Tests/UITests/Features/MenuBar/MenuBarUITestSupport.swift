import AppKit
import CoreML
import Vision
import XCTest

@MainActor
extension TrackerUITestCase {
    var statusButton: XCUIElement { element("menu.status") }

    func chooseMenuBarDisplay(_ label: String) {
        element("menu.display").click()
        let item = app.menuItems[label].firstMatch
        XCTAssertTrue(item.waitForExistence(timeout: timeout))
        item.click()
    }

    func assertStatusTooltip(_ expected: String, file: StaticString = #filePath, line: UInt = #line) {
        trackerWindow.coordinate(withNormalizedOffset: CGVector(dx: 0.7, dy: 0.6)).hover()
        statusButton.hover()
        var screenshot: XCUIScreenshot?
        var recognizedText = ""
        let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            let captured = XCUIScreen.main.screenshot()
            screenshot = captured
            let request = VNRecognizeTextRequest()
            request.revision = VNRecognizeTextRequestRevision1
            request.recognitionLevel = .fast
            request.recognitionLanguages = ["en-US"]
            request.usesLanguageCorrection = false
            request.regionOfInterest = CGRect(x: 0, y: 0.75, width: 1, height: 0.25)
            do {
                for (stage, devices) in try request.supportedComputeStageDevices {
                    guard let cpu = devices.first(where: {
                        if case .cpu = $0 { return true }
                        return false
                    }) else {
                        recognizedText = "Text recognition has no CPU device for \(stage)"
                        return false
                    }
                    request.setComputeDevice(cpu, for: stage)
                }
                try VNImageRequestHandler(data: captured.pngRepresentation, options: [:]).perform([request])
                recognizedText = (request.results ?? []).compactMap { $0.topCandidates(1).first?.string }
                    .joined(separator: " ").split(whereSeparator: \.isWhitespace).joined(separator: " ")
                return recognizedText.contains(expected)
            } catch {
                recognizedText = "Text recognition failed: \(error)"
                return false
            }
        }, object: nil)
        let result = XCTWaiter.wait(for: [expectation], timeout: timeout)
        if result != .completed {
            if let screenshot {
                let attachment = XCTAttachment(screenshot: screenshot)
                attachment.name = "Status tooltip desktop"
                attachment.lifetime = .keepAlways
                add(attachment)
            }
            let attachment = XCTAttachment(string: recognizedText)
            attachment.name = "Status tooltip recognized text"
            attachment.lifetime = .keepAlways
            add(attachment)
        }
        trackerWindow.coordinate(withNormalizedOffset: CGVector(dx: 0.7, dy: 0.6)).hover()
        XCTAssertEqual(result, .completed, "Hover shows the full tracking status", file: file, line: line)
    }

    func openStatusMenu() {
        app.typeKey("t", modifierFlags: [.control, .option])
        XCTAssertTrue(app.menuItems["Open Time Tracker"].waitForExistence(timeout: timeout))
    }

    func dismissStatusMenu() {
        app.typeKey(.escape, modifierFlags: [])
        waitUntil("The native menu closes") { !self.app.menuItems["Open Time Tracker"].exists }
    }

    func menuTask(_ task: FixtureTask) -> XCUIElement { element("menu.task.\(task.id)") }

    func copyFromMenu(_ action: String, task: FixtureTask) {
        let row = menuTask(task)
        XCTAssertTrue(row.waitForExistence(timeout: timeout))
        row.hover()
        let item = row.menuItems[action].firstMatch
        XCTAssertTrue(item.waitForExistence(timeout: timeout))
        XCTAssertTrue(item.isEnabled)
        clickTaskSubmenuItem(item, task: task)
    }

    func clickTaskSubmenuItem(_ item: XCUIElement, task: FixtureTask) {
        waitUntil("The task submenu action is reachable") { item.exists && item.isHittable }
        let row = menuTask(task)
        // Enter the submenu at the parent row's height before moving to the action.
        // A diagonal move can cross another parent row when the submenu opens to the left.
        row.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
            .withOffset(CGVector(dx: item.frame.midX - row.frame.midX, dy: 0)).hover()
        item.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).click()
    }

    func withRestoredClipboard(_ body: () throws -> Void) rethrows {
        let pasteboard = NSPasteboard.general
        let saved = (pasteboard.pasteboardItems ?? []).map { item in
            item.types.compactMap { type in item.data(forType: type).map { (type, $0) } }
        }
        let restore: @MainActor () -> Void = {
            let pasteboard = NSPasteboard.general
            pasteboard.clearContents()
            let items = saved.map { values -> NSPasteboardItem in
                let item = NSPasteboardItem()
                for (type, data) in values { item.setData(data, forType: type) }
                return item
            }
            pasteboard.writeObjects(items)
        }
        addTeardownBlock { await restore() }
        defer { restore() }
        try body()
    }
}

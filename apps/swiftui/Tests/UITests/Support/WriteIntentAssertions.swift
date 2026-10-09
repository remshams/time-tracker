import XCTest

@MainActor
extension TrackerUITestCase {
    func assertAcceptedWriteIntent(_ proxy: ControlledProxy, method: String, path: String,
                                   file: StaticString = #filePath, line: UInt = #line) throws {
        let attempts = try proxy.requests(method: method, path: path)
        XCTAssertEqual(attempts.count, 2, "The initial write and its transport retry are the only attempts.",
                       file: file, line: line)
        let first = try XCTUnwrap(attempts.first, file: file, line: line)
        let body = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(first.body.utf8)) as? [String: Any],
                                 file: file, line: line)
        let requestID = try XCTUnwrap(body["request_id"] as? String, file: file, line: line)
        XCTAssertNotNil(UUID(uuidString: requestID), "Each accepted intent has a request ID.", file: file, line: line)
        for attempt in attempts {
            XCTAssertEqual(attempt.body, first.body, "Transport retries preserve the entire write intent.",
                           file: file, line: line)
            let status = try XCTUnwrap(attempt.status, file: file, line: line)
            XCTAssertTrue((200..<300).contains(status), "The server accepted each idempotent attempt.",
                          file: file, line: line)
        }
    }
}

import Foundation

struct TrackerLaunchConfiguration {
    let defaults: UserDefaults
    let localDatabasePath: String?

    static var current: Self {
        #if DEBUG
        let process = ProcessInfo.processInfo
        if process.arguments.contains("-tt-ui-testing") {
            guard let path = process.environment["TT_UI_TEST_DATABASE_PATH"],
                  path.hasPrefix("/"), !path.utf8.contains(0),
                  let suite = process.environment["TT_UI_TEST_DEFAULTS_SUITE"],
                  suite.hasPrefix("TimeTrackerUITests."),
                  UUID(uuidString: String(suite.dropFirst("TimeTrackerUITests.".count))) != nil,
                  let defaults = UserDefaults(suiteName: suite) else {
                fatalError("UI tests require an absolute database path and an isolated preferences suite.")
            }
            return Self(defaults: defaults, localDatabasePath: path)
        }
        #endif
        return Self(defaults: .standard, localDatabasePath: nil)
    }
}

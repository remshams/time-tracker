import Foundation

struct TrackerLaunchConfiguration {
    let defaults: UserDefaults
    let localDatabasePath: String?

    #if DEBUG
        /// The isolated preferences suite of a UI test launch, or nil when the app runs outside UI tests.
        static let uiTestSuite: String? = {
            let process = ProcessInfo.processInfo
            guard process.arguments.contains("-tt-ui-testing") else { return nil }
            guard let suite = process.environment["TT_UI_TEST_DEFAULTS_SUITE"],
                suite.hasPrefix("TimeTrackerUITests."),
                UUID(uuidString: String(suite.dropFirst("TimeTrackerUITests.".count))) != nil
            else {
                fatalError("UI tests require an isolated preferences suite.")
            }
            return suite
        }()
    #endif

    static var current: Self {
        #if DEBUG
            if let suite = uiTestSuite {
                guard let path = ProcessInfo.processInfo.environment["TT_UI_TEST_DATABASE_PATH"],
                    path.hasPrefix("/"), !path.utf8.contains(0),
                    let defaults = UserDefaults(suiteName: suite)
                else {
                    fatalError("UI tests require an absolute database path and an isolated preferences suite.")
                }
                return Self(defaults: defaults, localDatabasePath: path)
            }
        #endif
        return Self(defaults: .standard, localDatabasePath: nil)
    }
}

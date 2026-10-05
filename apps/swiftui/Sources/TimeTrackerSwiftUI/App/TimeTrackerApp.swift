import AppKit

@main
@MainActor
enum TimeTrackerApp {
    static func main() {
        let application = NSApplication.shared
        let delegate = TrackerApplicationDelegate()
        application.delegate = delegate
        _ = application.setActivationPolicy(.regular)
        withExtendedLifetime(delegate) {
            application.run()
        }
    }
}

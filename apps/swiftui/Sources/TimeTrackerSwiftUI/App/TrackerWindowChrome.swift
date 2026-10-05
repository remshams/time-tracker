import AppKit

@MainActor
enum TrackerWindowChrome {
    static func makeWindow() -> NSWindow {
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1100, height: 760),
                              styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
                              backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.title = "Time Tracker"
        window.titleVisibility = .hidden
        window.toolbarStyle = .unified
        window.contentMinSize = NSSize(width: 760, height: 480)
        return window
    }
}

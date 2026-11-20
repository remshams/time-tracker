import AppKit
import SwiftUI
import TrackerClient

// Renders the tracker window offscreen with fixture data and writes PNGs, so a design can be reviewed without
// launching the app or taking over the desktop. It replaces the Rust worker with an in-memory client.

private let fixtureNow = parseTimestamp("2026-11-13T15:42:10.000Z")!

private func stamp(_ offset: TimeInterval) -> String {
    let formatter = ISO8601DateFormatter()
    formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    return formatter.string(from: fixtureNow.addingTimeInterval(offset))
}

private let hour: TimeInterval = 3600
private let day: TimeInterval = 86400

private let fixtureTasks: [TaskItem] = [
    TaskItem(id: "t1", name: "Checkout redesign", archived: false, latestStart: stamp(-1.2 * hour)),
    TaskItem(id: "t2", name: "Code review", archived: false, latestStart: stamp(-3.5 * hour)),
    TaskItem(id: "t3", name: "Team sync and planning", archived: false, latestStart: stamp(-5 * hour)),
    TaskItem(
        id: "t4", name: "Invoice export for the quarterly finance report", archived: false,
        latestStart: stamp(-1 * day)),
    TaskItem(id: "t5", name: "Support inbox", archived: false, latestStart: stamp(-2 * day)),
    TaskItem(id: "t6", name: "Database migration", archived: false, latestStart: stamp(-6 * day)),
    TaskItem(id: "t7", name: "Hiring interviews", archived: false, latestStart: nil),
    TaskItem(id: "t8", name: "Old landing page", archived: true, latestStart: stamp(-40 * day)),
]

private let fixtureActive = WorklogItem(id: "w-run", taskId: "t1", start: stamp(-1.2 * hour), end: nil)

private let fixtureHistory: [WorklogItem] = [
    fixtureActive,
    WorklogItem(id: "w2", taskId: "t1", start: stamp(-4.1 * hour), end: stamp(-2.9 * hour)),
    WorklogItem(id: "w3", taskId: "t1", start: stamp(-6.5 * hour), end: stamp(-5.25 * hour)),
    WorklogItem(id: "w4", taskId: "t1", start: stamp(-1 * day - 2 * hour), end: stamp(-1 * day + 0.5 * hour)),
    WorklogItem(id: "w5", taskId: "t1", start: stamp(-1 * day - 5 * hour), end: stamp(-1 * day - 3 * hour)),
    WorklogItem(id: "w6", taskId: "t1", start: stamp(-2 * day - 1 * hour), end: stamp(-2 * day + 1.75 * hour)),
    WorklogItem(id: "w7", taskId: "t1", start: stamp(-4 * day - 3 * hour), end: stamp(-4 * day - 2.5 * hour)),
    WorklogItem(id: "w8", taskId: "t1", start: stamp(-5 * day - 6 * hour), end: stamp(-5 * day - 1 * hour)),
]

private let fixtureTotals: [TaskReportTotal] = [
    TaskReportTotal(taskId: "t1", durationMicroseconds: Int64(3.6 * hour * 1_000_000)),
    TaskReportTotal(taskId: "t2", durationMicroseconds: Int64(0.75 * hour * 1_000_000)),
    TaskReportTotal(taskId: "t3", durationMicroseconds: Int64(1.5 * hour * 1_000_000)),
]

private struct Scenario {
    let name: String
    let tasks: [TaskItem]
    let active: WorklogItem?
    let select: String?
}

private let scenarios: [Scenario] = [
    Scenario(name: "tracking", tasks: fixtureTasks, active: fixtureActive, select: "t1"),
    Scenario(name: "idle", tasks: fixtureTasks, active: nil, select: "t2"),
    Scenario(name: "empty", tasks: [], active: nil, select: nil),
]

private struct FixtureClient: TrackerClient, ReportClient {
    let tasks: [TaskItem]
    let active: WorklogItem?

    private var resources: TaskListResources {
        TaskListResources(catalog: .init(value: tasks), tracking: .init(value: active))
    }

    private func unsupported() -> BridgeFailure { BridgeFailure(message: "The design snapshot is read-only.") }

    func openConfigured(_ settings: ConnectionSettings) async throws -> TaskListResources { resources }
    func test(_ settings: ConnectionSettings) async throws {}
    func connect(_ settings: ConnectionSettings) async throws -> TaskListResources { resources }
    func refreshTaskList(settings: ConnectionSettings) async throws -> TaskListResources { resources }
    func refreshTaskListWithTotals(settings: ConnectionSettings, start: String, end: String, now: String)
        async throws -> TaskListTotalsRefresh
    {
        let ids = Set(tasks.map(\.id))
        return TaskListTotalsRefresh(
            taskList: resources, report: TrackerReport(rows: fixtureTotals.filter { ids.contains($0.taskId) }, now: now)
        )
    }
    func createTask(name: String, occurredAt: String) async throws -> TaskCreationResult { throw unsupported() }
    func previewInactiveTasks(inactiveDays: Int, asOf: String) async throws -> InactiveTaskPreview {
        throw unsupported()
    }
    func archiveInactiveTasks(preview: InactiveTaskPreview) async throws -> InactiveTaskArchiveResult {
        throw unsupported()
    }
    func archiveTask(taskID: String, expectedName: String, occurredAt: String) async throws -> TaskCommandResult {
        throw unsupported()
    }
    func unarchiveTask(taskID: String, occurredAt: String) async throws -> TaskCommandResult { throw unsupported() }
    func renameTask(taskID: String, expectedName: String, name: String, occurredAt: String) async throws
        -> TaskCommandResult
    { throw unsupported() }
    func correctWorklog(expected: WorklogItem, replacementStart: String, replacementEnd: String?, occurredAt: String)
        async throws -> WorklogCorrectionResult
    { throw unsupported() }
    func moveCandidates(sourceTaskID: String, query: String) async throws -> [WorklogMoveCandidate] { [] }
    func moveWorklog(expected: WorklogItem, destinationTaskID: String) async throws -> WorklogMoveResult {
        throw unsupported()
    }
    func deleteWorklog(expected: WorklogItem) async throws -> WorklogDeletionResult { throw unsupported() }
    func startTracking(taskID: String, expectedActiveID: String?, occurredAt: String) async throws
        -> TrackingCommandResult
    { throw unsupported() }
    func stopTracking(worklogID: String, occurredAt: String) async throws -> TrackingCommandResult {
        throw unsupported()
    }
    func pauseTracking(worklogID: String, occurredAt: String) async throws -> TrackingPauseResult {
        throw unsupported()
    }
    func resumeTracking(taskID: String, occurredAt: String) async throws -> TrackingCommandResult {
        throw unsupported()
    }
    func history(taskID: String, cursor: String?) async throws -> HistoryPage {
        HistoryPage(worklogs: fixtureHistory.filter { $0.taskId == taskID }, nextCursor: nil, reset: false)
    }
}

private struct FixedClock: TrackerClock {
    var now: Date { fixtureNow }
    var uptime: TimeInterval { 1000 }
}

private final class MemorySettings: ConnectionSettingsRepository {
    func load() -> ConnectionSettings? { .local }
    func save(_ settings: ConnectionSettings) {}
}

private final class MemoryPreferences: TrackingPreferencesRepository {
    func load() -> TrackingPreferences { TrackingPreferences() }
    func save(_ preferences: TrackingPreferences) {}
}

/// Stands in for the app runtime that `TrackerWindow` reads. The real one owns the Rust worker and the status item.
final class TrackerAppRuntime {
    let features: TrackerFeatures

    init(features: TrackerFeatures) {
        self.features = features
    }

    func installSceneOpener(_ openWindow: OpenWindowAction) {}
}

private func spinRunLoop(_ seconds: TimeInterval) {
    RunLoop.main.run(until: Date().addingTimeInterval(seconds))
}

/// The app's default window size and the minimum content size that `TrackerWindow` allows.
private let windowSizes = [NSSize(width: 1100, height: 760), NSSize(width: 760, height: 480)]

/// Returns the PNG and whether it's flat: drawn in-process without glass because the screen was locked.
private func render(_ scenario: Scenario, appearance: NSAppearance.Name, size: NSSize, to directory: URL) throws
    -> (url: URL, flat: Bool)
{
    let client = FixtureClient(tasks: scenario.tasks, active: scenario.active)
    let features = TrackerFeatures(
        client: client, clock: FixedClock(), scheduler: RunLoopTrackerScheduler(), settings: MemorySettings(),
        trackingPreferences: MemoryPreferences(), reports: client, calendar: Calendar(identifier: .gregorian))
    let runtime = TrackerAppRuntime(features: features)
    features.sync.start()
    features.sync.setWindowVisible(true)
    spinRunLoop(0.5)
    if let select = scenario.select { features.taskCatalog.select(select) }
    spinRunLoop(0.5)

    let root = TrackerWindow(runtime: runtime)
        .environment(features.taskCatalog)
        .environment(features.worklogHistory)
        .environment(features.connection)
        .environment(features.tracking)
        .environment(features.trackingPill)
        .environment(features.dailyTotals)
        .environment(features.trackingAutomation)
        .environment(features.taskRename)
        .environment(features.taskArchiving)
        .environment(features.taskCreation)
        .environment(features.worklogDeletion)
        .environment(features.worklogMove)
        .environment(features.bulkTaskArchiving)
        .environment(features.worklogCorrection)

    let controller = NSHostingController(rootView: root)
    controller.sceneBridgingOptions = [.toolbars, .title]
    let window = OffscreenWindow(
        contentRect: NSRect(origin: NSPoint(x: -20000, y: -20000), size: size),
        styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
        backing: .buffered, defer: false)
    window.titlebarAppearsTransparent = true
    window.toolbarStyle = .unified
    NSApplication.shared.appearance = NSAppearance(named: appearance)
    window.appearance = NSAppearance(named: appearance)
    window.contentViewController = controller
    window.setContentSize(size)
    window.setFrameOrigin(NSPoint(x: -20000, y: -20000))
    window.orderFrontRegardless()
    spinRunLoop(1.5)

    // The second lock check catches a screen that locked during the capture, which leaves the image blank.
    let flat: Bool
    let bitmap: NSBitmapImageRep
    if !isScreenLocked(), let image = windowServerImage(of: window), !isScreenLocked() {
        flat = false
        bitmap = NSBitmapImageRep(cgImage: image)
    } else if isScreenLocked() {
        flat = true
        bitmap = try inProcessImage(of: window)
    } else {
        throw BridgeFailure(message: "The window server returned no image for the snapshot window.")
    }
    let suffix = appearance == .darkAqua ? "dark" : "light"
    // The file name uses the window's final size, because SwiftUI adds the toolbar to the content's minimum height.
    let captured = window.frame.size
    func snapshotURL(flat: Bool) -> URL {
        directory.appendingPathComponent(
            "\(scenario.name)-\(suffix)\(flat ? "-flat" : "")-\(Int(captured.width))x\(Int(captured.height)).png")
    }
    let url = snapshotURL(flat: flat)
    guard let png = bitmap.representation(using: .png, properties: [:]) else {
        throw BridgeFailure(message: "AppKit couldn't encode the snapshot as a PNG.")
    }
    try png.write(to: url)
    // A capture of the other kind from an earlier run would show older code next to this one.
    let other = snapshotURL(flat: !flat)
    if FileManager.default.fileExists(atPath: other.path) { try FileManager.default.removeItem(at: other) }
    print(url.path)
    window.orderOut(nil)
    window.close()
    features.sync.setWindowVisible(false)
    return (url, flat)
}

/// AppKit moves a titled window back onto a screen when it's shown or resized, which put the snapshot windows on the
/// user's desktop. Keeping the requested frame leaves the window offscreen, and the window server still renders it.
private final class OffscreenWindow: NSWindow {
    override func constrainFrameRect(_ frameRect: NSRect, to screen: NSScreen?) -> NSRect { frameRect }
}

private typealias WindowImageFunction = @convention(c) (CGRect, UInt32, UInt32, UInt32) -> Unmanaged<CGImage>?

/// Asks the window server for the window's pixels, which include the glass sidebar and vibrant text that drawing the
/// views into a bitmap can't reproduce. The SDK hides `CGWindowListCreateImage`, but it still exists, and a process
/// may capture its own windows without the Screen Recording permission.
private func windowServerImage(of window: NSWindow) -> CGImage? {
    let handle = dlopen("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics", RTLD_NOW)
    guard let symbol = dlsym(handle, "CGWindowListCreateImage") else { return nil }
    let createImage = unsafeBitCast(symbol, to: WindowImageFunction.self)
    let includingWindow: UInt32 = 1 << 3
    let boundsIgnoreFraming: UInt32 = 1 << 0
    let bestResolution: UInt32 = 1 << 3
    return createImage(
        .null, includingWindow, UInt32(window.windowNumber), boundsIgnoreFraming | bestResolution
    )?.takeRetainedValue()
}

/// Draws the window's views into a bitmap. It works while the screen is locked, but only the window server composites
/// glass, so the sidebar list and the toolbar lose their glass, selection, and colors. `CALayer.render(in:)` is worse:
/// it leaves the whole sidebar blank.
private func inProcessImage(of window: NSWindow) throws -> NSBitmapImageRep {
    guard let frameView = window.contentView?.superview,
        let bitmap = frameView.bitmapImageRepForCachingDisplay(in: frameView.bounds)
    else {
        throw BridgeFailure(message: "AppKit couldn't create a bitmap for the snapshot window.")
    }
    frameView.cacheDisplay(in: frameView.bounds, to: bitmap)
    return bitmap
}

/// The window server stops compositing while the screen is locked and returns blank images. The run checks before
/// each capture, because the screen can lock while it runs.
private func isScreenLocked() -> Bool {
    let session = CGSessionCopyCurrentDictionary() as? [String: Any]
    return session?["CGSSessionScreenIsLocked"] as? Bool == true
}

/// Agents read this to tell the user which design checks the flat snapshots can't support.
private func reportFlatSnapshots(_ files: [URL]) {
    let names = files.map { "  \($0.lastPathComponent)" }.joined(separator: "\n")
    let message = """
        The screen was locked, so the window server didn't draw the snapshot window. These \(files.count) snapshots \
        were drawn in-process instead and have "-flat" in their names:
        \(names)
        The detail pane, the worklog history, the sidebar's daily total and bar, the Active/Archived picker, and \
        the connection footer look as they do in the app. Content inside the glass is wrong: the sidebar and \
        toolbar have no glass, the selected task row is a solid black bar that hides its name and time, the task \
        color dots in the list and the tracking pill are white, and in dark mode the task list and pill text is \
        dark on a dark background. Don't judge the task list, the tracking pill, glass, vibrancy, or contrast \
        against the glass from these snapshots. Tell the user that these checks were skipped because the screen \
        was locked, or ask them to unlock the Mac and run the script again.

        """
    FileHandle.standardError.write(Data(message.utf8))
}

@main
struct DesignSnapshots {
    static func main() throws {
        let app = NSApplication.shared
        // Prohibited keeps the snapshot process out of the Dock and away from keyboard focus.
        app.setActivationPolicy(.prohibited)
        let arguments = CommandLine.arguments
        let directory = URL(fileURLWithPath: arguments.count > 1 ? arguments[1] : "design-snapshots")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        var flatFiles: [URL] = []
        // A later capture can fail, and the agent still needs to know which checks the earlier flat ones skip.
        defer { if !flatFiles.isEmpty { reportFlatSnapshots(flatFiles) } }
        for scenario in scenarios {
            for appearance in [NSAppearance.Name.aqua, .darkAqua] {
                for size in windowSizes {
                    let (url, flat) = try render(
                        scenario, appearance: appearance, size: size, to: directory)
                    if flat { flatFiles.append(url) }
                }
            }
        }
    }
}

enum TrackerSceneID {
    static let tracker = "tracker"
    static let settings = "settings"
}

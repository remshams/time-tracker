import AppKit
import SwiftUI

@main
@MainActor
struct PaneLayoutChecks: App {
    @StateObject private var checks = LayoutChecks()

    var body: some Scene {
        WindowGroup("Layout check", id: "native-layout") {
            LayoutFixture(checks: checks)
        }
        .defaultSize(width: 1100, height: 760)
        .windowStyle(.hiddenTitleBar)
        .windowToolbarStyle(.unified)
    }
}

@MainActor
private struct LayoutFixture: View {
    @ObservedObject var checks: LayoutChecks
    @StateObject private var fixture = FixtureState()
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        TrackerSplitLayout(columnVisibility: $fixture.columnVisibility) {
            VStack(spacing: 0) {
                Text("Tasks").padding(12)
                Spacer()
                Text("Connected")
                    .padding(12)
                    .background(LayoutProbe(fixture: fixture, kind: .footer))
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(LayoutProbe(fixture: fixture, kind: .sidebar))
            .toolbar {
                ToolbarItemGroup(placement: .navigation) {
                    Button("Settings") {}
                    Button("New task") {}
                }
            }
        } detail: {
            VStack(alignment: .leading, spacing: 0) {
                Text("A task heading that must remain below the toolbar")
                    .font(.title)
                    .background(LayoutProbe(fixture: fixture, kind: .heading))
                    .padding(24)
                Divider()
                ScrollView {
                    Text("Worklogs").padding(24)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .background(LayoutProbe(fixture: fixture, kind: .detail))
            .toolbar {
                ToolbarItemGroup(placement: .primaryAction) {
                    Button("Edit name") {}
                    Button("Start tracking") {}
                }
            }
        }
        .frame(minWidth: 760, minHeight: 480)
        .onAppear {
            checks.register(fixture) { openWindow(id: "native-layout") }
        }
    }
}

@MainActor
private final class FixtureState: ObservableObject {
    @Published var columnVisibility: NavigationSplitViewVisibility = .all
    weak var window: NSWindow?
    weak var heading: NSView?
    weak var footer: NSView?
    weak var sidebar: NSView?
    weak var detail: NSView?
}

@MainActor
private struct LayoutProbe: NSViewRepresentable {
    enum Kind { case heading, footer, sidebar, detail }
    let fixture: FixtureState
    let kind: Kind

    func makeNSView(context: Context) -> ProbeView {
        let view = ProbeView()
        view.onWindowChange = { [weak fixture] window in
            if let window { fixture?.window = window }
        }
        switch kind {
        case .heading: fixture.heading = view
        case .footer: fixture.footer = view
        case .sidebar: fixture.sidebar = view
        case .detail: fixture.detail = view
        }
        return view
    }

    func updateNSView(_ nsView: ProbeView, context: Context) {}
}

@MainActor
private final class ProbeView: NSView {
    var onWindowChange: ((NSWindow?) -> Void)?

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        onWindowChange?(window)
    }
}

@MainActor
private final class LayoutChecks: ObservableObject {
    private var fixtures: [FixtureState] = []
    private var started = false

    func register(_ fixture: FixtureState, openSecondWindow: @escaping () -> Void) {
        guard !fixtures.contains(where: { $0 === fixture }) else { return }
        fixtures.append(fixture)
        guard !started else { return }
        started = true
        openSecondWindow()
        Task { @MainActor in
            do {
                try await run()
                print("SwiftUI scene layout checks passed.")
                NSApplication.shared.terminate(nil)
            } catch {
                fputs("SwiftUI scene layout checks failed: \(error)\n", stderr)
                exit(1)
            }
        }
    }

    private func run() async throws {
        for _ in 0..<100 {
            if fixtures.count == 2 && fixtures.allSatisfy({ $0.window != nil && $0.heading != nil }) {
                break
            }
            try await settle()
        }
        guard fixtures.count == 2,
              let firstWindow = fixtures[0].window,
              let secondWindow = fixtures[1].window,
              firstWindow !== secondWindow else {
            throw LayoutFailure(message: "SwiftUI did not create two independent tracker windows")
        }

        for fixture in fixtures {
            try await waitForLayout(fixture)
            guard let window = fixture.window, let toolbar = window.toolbar else {
                throw LayoutFailure(message: "SwiftUI did not install the window toolbar")
            }
            for style in [NSWindow.ToolbarStyle.unifiedCompact, .expanded] {
                window.toolbarStyle = style
                for size in [NSSize(width: 760, height: 480), NSSize(width: 1100, height: 760)] {
                    window.setContentSize(size)
                    for visible in [true, false, true] {
                        toolbar.isVisible = visible
                        try await waitForLayout(fixture)
                    }
                }
            }
        }

        let first = fixtures[0]
        let second = fixtures[1]
        let expandedWidth = try frame(first.detail, in: firstWindow).width
        let secondSidebar = try frame(second.sidebar, in: secondWindow)
        first.columnVisibility = .detailOnly
        try await waitForLayout(first, sidebarVisible: false) {
            try self.frame(first.detail, in: firstWindow).width > expandedWidth + 200
        }
        guard second.columnVisibility == .all,
              approximatelyEqual(secondSidebar, try frame(second.sidebar, in: secondWindow)) else {
            throw LayoutFailure(message: "Collapsing one window changed the other sidebar")
        }
        first.columnVisibility = .all
        try await waitForLayout(first) {
            let restoredWidth = try self.frame(first.detail, in: firstWindow).width
            return abs(restoredWidth - expandedWidth) <= 1
        }
    }

    private func waitForLayout(_ fixture: FixtureState, sidebarVisible: Bool = true,
                               condition: () throws -> Bool = { true }) async throws {
        var previous: [NSRect]?
        var lastFailure: Error?
        for _ in 0..<100 {
            try await settle()
            for current in fixtures {
                current.window?.layoutIfNeeded()
                current.window?.contentView?.layoutSubtreeIfNeeded()
            }
            do {
                try checkContainment(fixture, sidebarVisible: sidebarVisible)
                guard let window = fixture.window, try condition() else {
                    previous = nil
                    lastFailure = nil
                    continue
                }
                var frames = [window.contentLayoutRect,
                              try frame(fixture.heading, in: window),
                              try frame(fixture.detail, in: window)]
                if sidebarVisible {
                    frames.append(try frame(fixture.sidebar, in: window))
                    frames.append(try frame(fixture.footer, in: window))
                }
                if let previous, previous.count == frames.count,
                   zip(previous, frames).allSatisfy({ approximatelyEqual($0.0, $0.1) }) {
                    return
                }
                previous = frames
                lastFailure = nil
            } catch {
                previous = nil
                lastFailure = error
            }
        }
        throw lastFailure ?? LayoutFailure(message: "SwiftUI layout did not reach stable expected geometry")
    }

    private func checkContainment(_ fixture: FixtureState, sidebarVisible: Bool = true) throws {
        guard let window = fixture.window else {
            throw LayoutFailure(message: "The SwiftUI window disappeared during a check")
        }
        let usable = window.contentLayoutRect.insetBy(dx: -1, dy: -1)
        let heading = try frame(fixture.heading, in: window)
        let detail = try frame(fixture.detail, in: window)
        guard !heading.isEmpty, !detail.isEmpty, usable.contains(heading), detail.width >= 379 else {
            throw LayoutFailure(message: "The task heading overlaps the window chrome or detail is too narrow")
        }
        if sidebarVisible {
            let footer = try frame(fixture.footer, in: window)
            let sidebar = try frame(fixture.sidebar, in: window)
            guard !footer.isEmpty, !sidebar.isEmpty, usable.contains(footer), sidebar.width >= 279,
                  sidebar.maxX <= detail.minX + 1 else {
                throw LayoutFailure(message: "The sidebar footer overlaps chrome or the sidebar overlaps detail")
            }
        }
    }

    private func frame(_ view: NSView?, in window: NSWindow) throws -> NSRect {
        guard let view, view.window === window else {
            throw LayoutFailure(message: "A SwiftUI layout marker is missing from its window")
        }
        return view.convert(view.bounds, to: nil)
    }

    private func approximatelyEqual(_ lhs: NSRect, _ rhs: NSRect) -> Bool {
        abs(lhs.minX - rhs.minX) <= 1 && abs(lhs.minY - rhs.minY) <= 1 &&
            abs(lhs.width - rhs.width) <= 1 && abs(lhs.height - rhs.height) <= 1
    }

    private func settle() async throws {
        try await Task.sleep(nanoseconds: 100_000_000)
    }
}

private struct LayoutFailure: Error, CustomStringConvertible {
    let message: String
    var description: String { message }
}

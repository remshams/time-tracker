import SwiftUI
import TrackerClient
#if DEBUG
    import AppKit
#endif

struct TrackerWindow: View {
    let runtime: TrackerAppRuntime
    @Environment(\.openWindow) private var openWindow
    @Environment(\.colorScheme) private var colorScheme
    @Environment(TaskCatalogStore.self) private var catalog
    @State private var columnVisibility: NavigationSplitViewVisibility = .all

    init(runtime: TrackerAppRuntime) {
        self.runtime = runtime
    }

    @ViewBuilder var body: some View {
        #if DEBUG
            if TrackerLaunchConfiguration.uiTestSuite != nil {
                content.background {
                    TrackerWindowAppearanceProbe(appearance: colorScheme == .dark ? "Dark" : "Light")
                        .frame(width: 1, height: 1)
                }
            } else {
                content
            }
        #else
            content
        #endif
    }

    private var content: some View {
        TrackerSplitLayout(columnVisibility: $columnVisibility) {
            TrackerSidebar()
                .toolbar {
                    ToolbarItemGroup(placement: .automatic) {
                        Button {
                            openWindow(id: TrackerSceneID.settings)
                        } label: {
                            Label("Settings", systemImage: "gearshape")
                        }
                        .help("Settings")

                        TrackerSidebarToolbar()
                    }
                }
        } detail: {
            TrackerDetailPresentation()
                .toolbar {
                    ToolbarItemGroup(placement: .primaryAction) {
                        TrackerTaskToolbar(availability: runtime.features.trackingAvailability)
                    }
                }
        }
        .navigationTitle(catalog.selectedTask?.name ?? "Time Tracker")
        .frame(minWidth: 760, minHeight: 480)
        .onAppear { runtime.installSceneOpener(openWindow) }
    }
}

#if DEBUG
    private struct TrackerWindowAppearanceProbe: NSViewRepresentable {
        let appearance: String

        func makeCoordinator() -> Coordinator { Coordinator() }

        func makeNSView(context: Context) -> NSTextField {
            let field = NSTextField(labelWithString: appearance)
            field.textColor = .clear
            field.setAccessibilityIdentifier("tracker.window.appearance")
            field.setAccessibilityLabel("Window appearance")
            context.coordinator.observeResize(for: field)
            return field
        }

        func updateNSView(_ field: NSTextField, context: Context) {
            field.stringValue = appearance
            field.setAccessibilityValue(appearance)
        }

        static func dismantleNSView(_ field: NSTextField, coordinator: Coordinator) {
            coordinator.stop()
        }

        final class Coordinator {
            private weak var field: NSTextField?
            private var resizeObserver: NSObjectProtocol?

            isolated deinit {
                if let resizeObserver {
                    DistributedNotificationCenter.default().removeObserver(resizeObserver)
                }
            }

            func observeResize(for field: NSTextField) {
                guard let suite = TrackerLaunchConfiguration.uiTestSuite else { return }
                self.field = field
                resizeObserver = DistributedNotificationCenter.default().addObserver(
                    forName: Notification.Name("\(suite).resize"), object: nil, queue: .main
                ) { [weak self] notification in
                    // Notification isn't Sendable, so read the requested size before entering the main actor.
                    let width = (notification.userInfo?["width"] as? NSNumber)?.doubleValue
                    let height = (notification.userInfo?["height"] as? NSNumber)?.doubleValue
                    MainActor.assumeIsolated { self?.resize(width: width, height: height) }
                }
            }

            func stop() {
                if let resizeObserver {
                    DistributedNotificationCenter.default().removeObserver(resizeObserver)
                }
                resizeObserver = nil
                field = nil
            }

            private func resize(width: Double?, height: Double?) {
                guard let window = field?.window, window.isKeyWindow,
                    let screen = window.screen,
                    let width, let height,
                    width.isFinite, height.isFinite,
                    width > 0, height > 0
                else { return }
                let available = window.contentRect(forFrameRect: screen.visibleFrame).size
                let minimum = window.contentMinSize
                guard minimum.width <= available.width, minimum.height <= available.height else { return }
                window.setContentSize(
                    NSSize(
                        width: max(minimum.width, min(CGFloat(width), available.width)),
                        height: max(minimum.height, min(CGFloat(height), available.height))
                    ))
            }
        }
    }
#endif

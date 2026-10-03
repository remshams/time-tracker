import AppKit
import TrackerClient

@MainActor
final class MacLifecycleObserver {
    private weak var session: TrackerSession?
    private var observers: [(NotificationCenter, NSObjectProtocol)] = []

    init(session: TrackerSession) { self.session = session }

    func start() {
        guard observers.isEmpty else { return }
        let center = NotificationCenter.default
        let windowEvents: [Notification.Name] = [
            NSWindow.didBecomeKeyNotification, NSWindow.didResignKeyNotification,
            NSWindow.didChangeOcclusionStateNotification, NSWindow.didMiniaturizeNotification,
            NSWindow.didDeminiaturizeNotification, NSWindow.willCloseNotification,
            NSApplication.didBecomeActiveNotification, NSApplication.didResignActiveNotification,
            NSApplication.didChangeOcclusionStateNotification, NSApplication.didHideNotification,
            NSApplication.didUnhideNotification
        ]
        for name in windowEvents { observe(center, name) { $0.updateVisibility() } }
        observe(center, NSMenu.didBeginTrackingNotification) { $0.session?.menuOpened() }
        observe(center, NSMenu.didEndTrackingNotification) { $0.session?.menuClosed() }
        observe(center, NSApplication.willTerminateNotification) { $0.session?.shutdown() }
        let workspace = NSWorkspace.shared.notificationCenter
        observe(workspace, NSWorkspace.willSleepNotification) { $0.session?.sleep() }
        observe(workspace, NSWorkspace.didWakeNotification) {
            $0.updateVisibility()
            $0.session?.wake()
        }
        updateVisibility()
    }

    deinit {
        for (center, observer) in observers { center.removeObserver(observer) }
    }

    private func observe(_ center: NotificationCenter, _ name: Notification.Name,
                         action: @escaping @MainActor (MacLifecycleObserver) -> Void) {
        let observer = center.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
            Task { @MainActor [weak self] in
                guard let self else { return }
                action(self)
            }
        }
        observers.append((center, observer))
    }

    private func updateVisibility() {
        let visible = NSApplication.shared.windows.contains {
            $0.styleMask.contains(.titled) && $0.isVisible && !$0.isMiniaturized &&
                $0.occlusionState.contains(.visible)
        }
        session?.setWindowVisible(visible)
    }
}

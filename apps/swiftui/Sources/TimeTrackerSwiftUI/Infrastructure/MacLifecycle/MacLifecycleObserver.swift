import AppKit
import TrackerClient

@MainActor
final class MacLifecycleObserver {
    private weak var session: TrackerSession?
    private weak var menu: TrackerMenuStore?
    private var observers: [(NotificationCenter, NSObjectProtocol)] = []

    init(session: TrackerSession, menu: TrackerMenuStore) {
        self.session = session
        self.menu = menu
    }

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
        observe(center, NSMenu.didBeginTrackingNotification) {
            $0.menu?.menuOpened()
            $0.session?.menuOpened()
        }
        observe(center, NSMenu.didEndTrackingNotification) {
            $0.session?.menuClosed()
            $0.menu?.menuClosed()
        }
        observe(center, NSApplication.willTerminateNotification) { $0.session?.shutdown() }
        let workspace = NSWorkspace.shared.notificationCenter
        observe(workspace, NSWorkspace.willSleepNotification) { $0.session?.sleep() }
        observe(workspace, NSWorkspace.didWakeNotification) {
            $0.updateVisibility()
            $0.session?.wake()
        }
        let distributed = DistributedNotificationCenter.default()
        observeScreenEvent(distributed, Notification.Name("com.apple.screenIsLocked"), locked: true)
        observeScreenEvent(distributed, Notification.Name("com.apple.screenIsUnlocked"), locked: false)
        updateVisibility()
    }

    deinit {
        for (center, observer) in observers { center.removeObserver(observer) }
    }

    private func observe(_ center: NotificationCenter, _ name: Notification.Name,
                         action: @escaping @MainActor (MacLifecycleObserver) -> Void) {
        let observer = center.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated {
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

    private func observeScreenEvent(_ center: NotificationCenter, _ name: Notification.Name, locked: Bool) {
        let observer = center.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
            let occurredAt = Date()
            // The main queue preserves the notification order and the captured event time.
            MainActor.assumeIsolated {
                if locked { self?.session?.screenLocked(at: occurredAt) }
                else { self?.session?.screenUnlocked(at: occurredAt) }
            }
        }
        observers.append((center, observer))
    }
}

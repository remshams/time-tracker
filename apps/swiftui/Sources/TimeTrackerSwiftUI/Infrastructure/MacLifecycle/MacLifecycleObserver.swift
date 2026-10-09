import AppKit
import TrackerClient

@MainActor
final class MacLifecycleObserver {
    private weak var session: TrackerSession?
    private var observers: [(NotificationCenter, NSObjectProtocol)] = []

    init(session: TrackerSession) {
        self.session = session
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
            NSApplication.didUnhideNotification,
        ]
        for name in windowEvents { observe(center, name) { $0.updateVisibility() } }
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
        #if DEBUG
            if ProcessInfo.processInfo.arguments.contains("-tt-ui-testing"),
                let suite = ProcessInfo.processInfo.environment["TT_UI_TEST_DEFAULTS_SUITE"],
                suite.hasPrefix("TimeTrackerUITests."),
                UUID(uuidString: String(suite.dropFirst("TimeTrackerUITests.".count))) != nil
            {
                for locked in [true, false] {
                    let name = Notification.Name("\(suite).\(locked ? "lock" : "unlock")")
                    observeScreenEvent(distributed, name, locked: locked)
                }
                observe(distributed, Notification.Name("\(suite).sleep")) { $0.session?.sleep() }
                observe(distributed, Notification.Name("\(suite).wake")) {
                    $0.updateVisibility()
                    $0.session?.wake()
                }
            }
        #endif
        updateVisibility()
    }

    deinit {
        for (center, observer) in observers { center.removeObserver(observer) }
    }

    private func observe(
        _ center: NotificationCenter, _ name: Notification.Name,
        action: @escaping @MainActor (MacLifecycleObserver) -> Void
    ) {
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
            $0.styleMask.contains(.titled) && $0.isVisible && !$0.isMiniaturized && $0.occlusionState.contains(.visible)
        }
        session?.setWindowVisible(visible)
    }

    private func observeScreenEvent(_ center: NotificationCenter, _ name: Notification.Name, locked: Bool) {
        let observer = center.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
            let occurredAt = Date()
            // The main queue preserves the notification order and the captured event time.
            MainActor.assumeIsolated {
                if locked {
                    self?.session?.screenLocked(at: occurredAt)
                } else {
                    self?.session?.screenUnlocked(at: occurredAt)
                }
            }
        }
        observers.append((center, observer))
    }
}

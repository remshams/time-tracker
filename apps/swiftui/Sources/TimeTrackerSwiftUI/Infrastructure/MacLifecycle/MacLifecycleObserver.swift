import AppKit
import TrackerClient

final class MacLifecycleObserver {
    // The sync handles visibility, sleep and wake; the auto-pause store handles screen lock events.
    private weak var sync: TrackerSync?
    private weak var automation: TrackingAutomationStore?
    private var observers: [(NotificationCenter, NSObjectProtocol)] = []

    init(sync: TrackerSync, automation: TrackingAutomationStore) {
        self.sync = sync
        self.automation = automation
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
        let workspace = NSWorkspace.shared.notificationCenter
        observe(workspace, NSWorkspace.willSleepNotification) { $0.sync?.sleep() }
        observe(workspace, NSWorkspace.didWakeNotification) {
            $0.updateVisibility()
            $0.sync?.wake()
        }
        let distributed = DistributedNotificationCenter.default()
        observeScreenEvent(distributed, Notification.Name("com.apple.screenIsLocked"), locked: true)
        observeScreenEvent(distributed, Notification.Name("com.apple.screenIsUnlocked"), locked: false)
        #if DEBUG
            if let suite = TrackerLaunchConfiguration.uiTestSuite {
                for locked in [true, false] {
                    let name = Notification.Name("\(suite).\(locked ? "lock" : "unlock")")
                    observeScreenEvent(distributed, name, locked: locked)
                }
                observe(distributed, Notification.Name("\(suite).sleep")) { $0.sync?.sleep() }
                observe(distributed, Notification.Name("\(suite).wake")) {
                    $0.updateVisibility()
                    $0.sync?.wake()
                }
            }
        #endif
        updateVisibility()
    }

    isolated deinit {
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
        sync?.setWindowVisible(visible)
    }

    private func observeScreenEvent(_ center: NotificationCenter, _ name: Notification.Name, locked: Bool) {
        let observer = center.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
            let occurredAt = Date()
            // The main queue preserves the notification order and the captured event time.
            MainActor.assumeIsolated {
                self?.automation?.setScreenLocked(locked, at: occurredAt)
            }
        }
        observers.append((center, observer))
    }
}

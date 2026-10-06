import AppKit
import CoreFoundation
import TrackerClient

@MainActor
final class NativeMenuKeyboardMonitor {
    private let route: (NSEvent) -> MenuTrackingKeyResult
    private let perform: (MenuShortcutAction) -> Void
    private var observer: CFRunLoopObserver?
    private var session: Session?
    private var handlingEvent = false
    private static let queueMode = RunLoop.Mode("TimeTracker.NativeMenuKeyboardQueue")

    @MainActor private final class Session {
        var installed = true
        var tracking = false
    }

    init(route: @escaping (NSEvent) -> MenuTrackingKeyResult,
         perform: @escaping (MenuShortcutAction) -> Void) {
        self.route = route
        self.perform = perform
    }

    deinit { MainActor.assumeIsolated { stop() } }

    func start() {
        guard observer == nil else { return }
        let session = Session()
        self.session = session
        let observer = CFRunLoopObserverCreateWithHandler(
            nil, CFRunLoopActivity.beforeSources.rawValue, true, 0
        ) { [weak self, session] _, _ in
            MainActor.assumeIsolated {
                guard session.installed, session.tracking else { return }
                self?.handlePendingKeys()
            }
        }
        self.observer = observer
        CFRunLoopAddObserver(CFRunLoopGetMain(), observer,
                             CFRunLoopMode(RunLoop.Mode.eventTracking.rawValue as CFString))
        // The queued block runs only after AppKit has entered its menu tracking loop.
        RunLoop.main.perform(inModes: [.eventTracking]) {
            MainActor.assumeIsolated {
                if session.installed { session.tracking = true }
            }
        }
        CFRunLoopWakeUp(CFRunLoopGetMain())
    }

    func stop() {
        session?.installed = false
        session?.tracking = false
        session = nil
        if let observer {
            CFRunLoopRemoveObserver(CFRunLoopGetMain(), observer,
                                    CFRunLoopMode(RunLoop.Mode.eventTracking.rawValue as CFString))
        }
        observer = nil
    }

    private func handlePendingKeys() {
        guard !handlingEvent else { return }
        handlingEvent = true
        defer { handlingEvent = false }
        // Drain claimed copy/release events, yielding immediately for native navigation.
        for _ in 0..<32 {
            guard session?.installed == true, session?.tracking == true else { return }
            // A separate mode avoids dispatching menu timers or sources during the peek.
            let events: NSEvent.EventTypeMask = [.keyDown, .keyUp]
            guard let event = NSApplication.shared.nextEvent(matching: events, until: .distantPast,
                                                            inMode: Self.queueMode, dequeue: false) else { return }
            let result = route(event)
            guard result != .passThrough else { return }
            let replacement: NSEvent?
            if case .navigate(let action) = result {
                let down = action == .moveDown
                let character = down ? "\u{F701}" : "\u{F700}"
                replacement = NSEvent.keyEvent(
                    with: event.type, location: event.locationInWindow,
                    modifierFlags: [.function, .numericPad], timestamp: event.timestamp,
                    windowNumber: event.windowNumber, context: nil,
                    characters: character, charactersIgnoringModifiers: character,
                    isARepeat: event.isARepeat, keyCode: down ? 125 : 126
                )
                guard replacement != nil else { return }
            } else { replacement = nil }
            // Remove the original before posting an arrow, then let AppKit process it first.
            guard let removed = NSApplication.shared.nextEvent(matching: events, until: .distantPast,
                                                              inMode: Self.queueMode, dequeue: true) else { return }
            guard removed === event else {
                NSApplication.shared.postEvent(removed, atStart: true)
                return
            }
            if let replacement {
                NSApplication.shared.postEvent(replacement, atStart: true)
                return
            }
            if case .perform(let action) = result {
                perform(action)
                if action == .openMenu { return }
            }
        }
    }
}

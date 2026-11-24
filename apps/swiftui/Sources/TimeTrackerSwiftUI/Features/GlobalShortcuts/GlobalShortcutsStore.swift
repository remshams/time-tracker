import Observation
import TrackerClient

/// Owns the two global shortcuts, Open menu and Open Quick Track, with their errors. A change registers with macOS
/// first, through `registerShortcut`, and is saved only when that works.
@Observable
final class GlobalShortcutsStore {
    private let preferences: UserDefaultsGlobalShortcutPreferences
    private(set) var shortcuts: GlobalShortcuts
    private var validationError: String?
    private(set) var registrationError: String?
    @ObservationIgnored var registerShortcut: ((GlobalShortcutAction, GlobalShortcut) -> String?)?
    var error: String? { validationError ?? registrationError }

    init(preferences: UserDefaultsGlobalShortcutPreferences) {
        self.preferences = preferences
        shortcuts = preferences.load()
    }

    func setShortcut(_ action: GlobalShortcutAction, shortcut: GlobalShortcut) {
        var candidate = shortcuts
        candidate[action] = shortcut
        // Recording a shortcut again retries a failed registration.
        setShortcuts(candidate, retry: registrationError != nil)
    }

    func resetShortcuts() { setShortcuts(.defaults, retry: true) }

    private func setShortcuts(_ candidate: GlobalShortcuts, retry: Bool) {
        if let error = candidate.validationError {
            reportValidationError(error)
            return
        }
        let actions = GlobalShortcutAction.allCases
        let changed = actions.filter { candidate[$0] != shortcuts[$0] }
        var registered: [GlobalShortcutAction] = []
        for action in changed {
            if let error = registerShortcut?(action, candidate[action]) {
                // Put back the bindings that were registered before this one failed. If one can't come back, its new
                // binding stays active, so Settings says so.
                let rollbackErrors = registered.compactMap { registerShortcut?($0, shortcuts[$0]) }
                if !rollbackErrors.isEmpty { reportRegistrationError(rollbackErrors.joined(separator: " ")) }
                reportValidationError(error)
                return
            }
            registered.append(action)
        }
        // A retry of an unchanged binding that still fails doesn't block saving the changed one; Settings keeps
        // showing its error.
        let retried = retry ? actions.filter { !changed.contains($0) } : []
        let retryErrors = retried.compactMap { registerShortcut?($0, candidate[$0]) }
        shortcuts = candidate
        reportValidationError(nil)
        if !changed.isEmpty || !retried.isEmpty {
            reportRegistrationError(retryErrors.isEmpty ? nil : retryErrors.joined(separator: " "))
        }
        preferences.save(candidate)
    }

    private func reportValidationError(_ message: String?) {
        guard validationError != message else { return }
        validationError = message
    }

    func reportRegistrationError(_ message: String?) {
        guard registrationError != message else { return }
        registrationError = message
    }
}

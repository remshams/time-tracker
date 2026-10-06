public enum MenuTrackingKeyPhase: Sendable {
    case down, up
}

public enum MenuTrackingKeyResult: Equatable, Sendable {
    case passThrough
    case consume
    case navigate(MenuShortcutAction)
    case perform(MenuShortcutAction)
}

public struct MenuTrackingKeyRouter: Sendable {
    private var heldActions: [UInt16: MenuShortcutAction] = [:]
    private static let maximumHeldKeys = 128
    private static let nativeKeyCodes: Set<UInt16> = [36, 49, 53, 76, 123, 124, 125, 126]

    public init() {}

    public mutating func reset() { heldActions.removeAll() }

    public mutating func route(keyCode: UInt16, key: String?, modifiers: MenuShortcut.Modifiers,
                               phase: MenuTrackingKeyPhase, isRepeat: Bool,
                               shortcuts: MenuKeyboardShortcuts) -> MenuTrackingKeyResult {
        guard !Self.nativeKeyCodes.contains(keyCode) else { return .passThrough }
        switch phase {
        case .up:
            guard let action = heldActions.removeValue(forKey: keyCode) else { return .passThrough }
            return result(for: action, phase: .up, isRepeat: false)
        case .down:
            if isRepeat, let action = heldActions[keyCode] {
                return result(for: action, phase: .down, isRepeat: true)
            }
            heldActions.removeValue(forKey: keyCode)
            guard let key,
                  let action = shortcuts.action(forKey: key, modifiers: modifiers, includingGlobal: true),
                  heldActions.count < Self.maximumHeldKeys else { return .passThrough }
            heldActions[keyCode] = action
            return result(for: action, phase: .down, isRepeat: isRepeat)
        }
    }

    private func result(for action: MenuShortcutAction, phase: MenuTrackingKeyPhase,
                        isRepeat: Bool) -> MenuTrackingKeyResult {
        switch action {
        case .moveDown, .moveUp: return .navigate(action)
        case .openMenu, .copyName, .copyExact, .copyRounded:
            return phase == .down && !isRepeat ? .perform(action) : .consume
        }
    }
}

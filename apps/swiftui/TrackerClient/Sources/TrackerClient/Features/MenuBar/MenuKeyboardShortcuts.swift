import Foundation

public struct MenuShortcut: Codable, Equatable, Hashable, Sendable {
    public struct Modifiers: OptionSet, Codable, Hashable, Sendable {
        public let rawValue: UInt8

        public init(rawValue: UInt8) { self.rawValue = rawValue }

        public static let command = Self(rawValue: 1 << 0)
        public static let control = Self(rawValue: 1 << 1)
        public static let option = Self(rawValue: 1 << 2)
        public static let shift = Self(rawValue: 1 << 3)
        public static let allowed: Self = [.command, .control, .option, .shift]

        public init(from decoder: Decoder) throws {
            let container = try decoder.singleValueContainer()
            let value = try container.decode(UInt8.self)
            guard value & ~Self.allowed.rawValue == 0 else {
                throw DecodingError.dataCorruptedError(in: container, debugDescription: "Unsupported shortcut modifiers")
            }
            self.init(rawValue: value)
        }

        public func encode(to encoder: Encoder) throws {
            var container = encoder.singleValueContainer()
            try container.encode(rawValue)
        }
    }

    public var key: String
    public var modifiers: Modifiers

    public init(key: String, modifiers: Modifiers = []) {
        self.key = key.unicodeScalars.allSatisfy { $0.isASCII } ? key.lowercased() : key
        self.modifiers = modifiers
    }

    public var displayText: String {
        var text = ""
        if modifiers.contains(.control) { text += "⌃" }
        if modifiers.contains(.option) { text += "⌥" }
        if modifiers.contains(.shift) { text += "⇧" }
        if modifiers.contains(.command) { text += "⌘" }
        return text + key.uppercased()
    }

    private enum CodingKeys: String, CodingKey { case key, modifiers }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.init(key: try container.decode(String.self, forKey: .key),
                  modifiers: try container.decode(Modifiers.self, forKey: .modifiers))
    }

    var normalized: Self { Self(key: key, modifiers: modifiers) }

    var hasSupportedKey: Bool {
        key.unicodeScalars.count == 1 && key.unicodeScalars.allSatisfy { (33...126).contains($0.value) }
    }
}

public enum MenuShortcutAction: String, CaseIterable, Codable, Sendable {
    case openMenu, moveDown, moveUp, copyName, copyExact, copyRounded

    public var title: String {
        switch self {
        case .openMenu: return "Open menu"
        case .moveDown: return "Move down"
        case .moveUp: return "Move up"
        case .copyName: return "Copy task name"
        case .copyExact: return "Copy exact duration"
        case .copyRounded: return "Copy rounded duration"
        }
    }
}

public struct MenuShortcutValidationError: Error, LocalizedError, Equatable {
    public let errors: [String]
    public var errorDescription: String? { errors.first }
}

public struct MenuKeyboardShortcuts: Codable, Equatable, Sendable {
    public var openMenu: MenuShortcut
    public var moveDown: MenuShortcut
    public var moveUp: MenuShortcut
    public var copyName: MenuShortcut
    public var copyExact: MenuShortcut
    public var copyRounded: MenuShortcut

    public static let defaults = Self()

    public init(openMenu: MenuShortcut = MenuShortcut(key: "t", modifiers: [.control, .option]),
                moveDown: MenuShortcut = MenuShortcut(key: "j"),
                moveUp: MenuShortcut = MenuShortcut(key: "k"),
                copyName: MenuShortcut = MenuShortcut(key: "c"),
                copyExact: MenuShortcut = MenuShortcut(key: "t"),
                copyRounded: MenuShortcut = MenuShortcut(key: "s")) {
        self.openMenu = openMenu
        self.moveDown = moveDown
        self.moveUp = moveUp
        self.copyName = copyName
        self.copyExact = copyExact
        self.copyRounded = copyRounded
    }

    public subscript(action: MenuShortcutAction) -> MenuShortcut {
        get {
            switch action {
            case .openMenu: return openMenu
            case .moveDown: return moveDown
            case .moveUp: return moveUp
            case .copyName: return copyName
            case .copyExact: return copyExact
            case .copyRounded: return copyRounded
            }
        }
        set {
            switch action {
            case .openMenu: openMenu = newValue
            case .moveDown: moveDown = newValue
            case .moveUp: moveUp = newValue
            case .copyName: copyName = newValue
            case .copyExact: copyExact = newValue
            case .copyRounded: copyRounded = newValue
            }
        }
    }

    public var validationError: String? { validationErrors().first }

    public func validationErrors() -> [String] {
        var errors: [String] = []
        var bindings: [MenuShortcut: MenuShortcutAction] = [:]
        for action in MenuShortcutAction.allCases {
            let shortcut = self[action].normalized
            if !shortcut.hasSupportedKey {
                errors.append("\(action.title) needs one printable ASCII key. Space, arrow keys, Return and Escape are reserved.")
            }
            if !shortcut.modifiers.subtracting(.allowed).isEmpty {
                errors.append("\(action.title) only supports Command, Control, Option and Shift modifiers.")
            }
            if action == .openMenu && shortcut.modifiers.intersection([.command, .control, .option]).isEmpty {
                errors.append("Open menu needs Command, Control or Option to avoid capturing normal typing.")
            }
            if let previous = bindings[shortcut] {
                errors.append("\(action.title) and \(previous.title) use the same shortcut, \(shortcut.displayText).")
            } else {
                bindings[shortcut] = action
            }
        }
        return errors
    }

    public func validated() throws -> Self {
        let errors = validationErrors()
        guard errors.isEmpty else { throw MenuShortcutValidationError(errors: errors) }
        var result = self
        for action in MenuShortcutAction.allCases { result[action] = self[action].normalized }
        return result
    }

    public func action(forKey key: String, modifiers: MenuShortcut.Modifiers,
                       includingGlobal: Bool = false) -> MenuShortcutAction? {
        let shortcut = MenuShortcut(key: key, modifiers: modifiers)
        guard shortcut.hasSupportedKey, modifiers.subtracting(.allowed).isEmpty else { return nil }
        return MenuShortcutAction.allCases.first {
            (includingGlobal || $0 != .openMenu) && self[$0].normalized == shortcut
        }
    }

    private enum CodingKeys: String, CodingKey { case openMenu, moveDown, moveUp, copyName, copyExact, copyRounded }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.init(openMenu: try container.decode(MenuShortcut.self, forKey: .openMenu),
                  moveDown: try container.decode(MenuShortcut.self, forKey: .moveDown),
                  moveUp: try container.decode(MenuShortcut.self, forKey: .moveUp),
                  copyName: try container.decode(MenuShortcut.self, forKey: .copyName),
                  copyExact: try container.decode(MenuShortcut.self, forKey: .copyExact),
                  copyRounded: try container.decode(MenuShortcut.self, forKey: .copyRounded))
        let errors = validationErrors()
        guard errors.isEmpty else {
            throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath,
                                                   debugDescription: errors.joined(separator: " ")))
        }
        self = try validated()
    }
}

public enum MenuDurationFormatter {
    public static func exact(_ duration: TimeInterval?) -> String? {
        guard let seconds = seconds(duration) else { return nil }
        let hours = seconds / 3_600
        let minutes = seconds % 3_600 / 60
        let remainder = seconds % 60
        if hours > 0 { return "\(hours)h \(minutes)m \(remainder)s" }
        if minutes > 0 { return "\(minutes)m \(remainder)s" }
        return "\(remainder)s"
    }

    public static func rounded(_ duration: TimeInterval?) -> String? {
        guard let duration, duration.isFinite else { return nil }
        let rounded = (max(0, duration) / 900).rounded(.toNearestOrAwayFromZero) * 900
        guard let seconds = seconds(rounded) else { return nil }
        let hours = seconds / 3_600
        let minutes = seconds % 3_600 / 60
        return hours > 0 ? "\(hours)h \(minutes)m" : "\(minutes)m"
    }

    private static func seconds(_ duration: TimeInterval?) -> Int64? {
        guard let duration, duration.isFinite, duration < Double(Int64.max) else { return nil }
        return Int64(max(0, duration))
    }
}

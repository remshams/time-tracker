import Foundation

public struct GlobalShortcut: Codable, Equatable, Hashable, Sendable {
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
                throw DecodingError.dataCorruptedError(
                    in: container, debugDescription: "Unsupported shortcut modifiers")
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
        return text + (isSpace ? "Space" : key.uppercased())
    }

    private enum CodingKeys: String, CodingKey { case key, modifiers }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.init(
            key: try container.decode(String.self, forKey: .key),
            modifiers: try container.decode(Modifiers.self, forKey: .modifiers))
    }

    var normalized: Self { Self(key: key, modifiers: modifiers) }

    public var isSpace: Bool { key == " " }

    var hasSupportedKey: Bool {
        key.unicodeScalars.count == 1 && key.unicodeScalars.allSatisfy { (33...126).contains($0.value) }
    }
}

public enum GlobalShortcutAction: CaseIterable, Sendable {
    case openMenu, openQuickTrack

    /// Only Quick Track accepts Space.
    var allowsSpace: Bool { self == .openQuickTrack }

    public var title: String {
        switch self {
        case .openMenu: return "Open menu"
        case .openQuickTrack: return "Open Quick Track"
        }
    }
}

public struct GlobalShortcuts: Codable, Equatable, Sendable {
    public var openMenu: GlobalShortcut
    public var openQuickTrack: GlobalShortcut

    public static let defaults = Self()

    public init(
        openMenu: GlobalShortcut = GlobalShortcut(key: "t", modifiers: [.control, .option]),
        openQuickTrack: GlobalShortcut = GlobalShortcut(key: " ", modifiers: [.control, .option])
    ) {
        self.openMenu = openMenu
        self.openQuickTrack = openQuickTrack
    }

    public subscript(action: GlobalShortcutAction) -> GlobalShortcut {
        get {
            switch action {
            case .openMenu: return openMenu
            case .openQuickTrack: return openQuickTrack
            }
        }
        set {
            switch action {
            case .openMenu: openMenu = newValue
            case .openQuickTrack: openQuickTrack = newValue
            }
        }
    }

    public var validationError: String? { validationErrors().first }

    public func validationErrors() -> [String] {
        var errors: [String] = []
        var bindings: [GlobalShortcut: GlobalShortcutAction] = [:]
        for action in GlobalShortcutAction.allCases {
            let shortcut = self[action].normalized
            if !(shortcut.hasSupportedKey || (action.allowsSpace && shortcut.isSpace)) {
                errors.append(
                    action.allowsSpace
                        ? "\(action.title) needs one printable ASCII key or Space. Arrow keys, Return and Escape are reserved."
                        : "\(action.title) needs one printable ASCII key. Space, arrow keys, Return and Escape are reserved."
                )
            }
            if !shortcut.modifiers.subtracting(.allowed).isEmpty {
                errors.append("\(action.title) only supports Command, Control, Option and Shift modifiers.")
            }
            if shortcut.modifiers.intersection([.command, .control, .option]).isEmpty {
                errors.append("\(action.title) needs Command, Control or Option to avoid capturing normal typing.")
            }
            if let previous = bindings[shortcut] {
                errors.append("\(action.title) and \(previous.title) use the same shortcut, \(shortcut.displayText).")
            } else {
                bindings[shortcut] = action
            }
        }
        return errors
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.init(
            openMenu: try container.decode(GlobalShortcut.self, forKey: .openMenu),
            openQuickTrack: try container.decode(GlobalShortcut.self, forKey: .openQuickTrack))
        let errors = validationErrors()
        guard errors.isEmpty else {
            throw DecodingError.dataCorrupted(
                .init(
                    codingPath: decoder.codingPath,
                    debugDescription: errors.joined(separator: " ")))
        }
    }

    private enum CodingKeys: String, CodingKey { case openMenu, openQuickTrack }
}

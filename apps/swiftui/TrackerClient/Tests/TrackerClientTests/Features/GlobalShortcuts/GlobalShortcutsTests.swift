import XCTest
@testable import TrackerClient

@MainActor
final class GlobalShortcutsTests: XCTestCase {
    func testDefaultsAreControlOptionTAndControlOptionSpace() async {
        let shortcuts = GlobalShortcuts.defaults
        XCTAssertEqual(shortcuts.openMenu, GlobalShortcut(key: "t", modifiers: [.control, .option]))
        XCTAssertEqual(shortcuts.openQuickTrack, GlobalShortcut(key: " ", modifiers: [.control, .option]))
        XCTAssertEqual(shortcuts.openQuickTrack.displayText, "⌃⌥Space")
        XCTAssertEqual(GlobalShortcut(key: " ", modifiers: [.shift, .command]).displayText, "⇧⌘Space")
        XCTAssertEqual(GlobalShortcutAction.openMenu.title, "Open menu")
        XCTAssertEqual(GlobalShortcutAction.openQuickTrack.title, "Open Quick Track")
        XCTAssertTrue(shortcuts.validationErrors().isEmpty)
        XCTAssertEqual(shortcuts[.openMenu], shortcuts.openMenu)
        XCTAssertEqual(shortcuts[.openQuickTrack], shortcuts.openQuickTrack)
    }

    func testSubscriptSetsTheMatchingShortcut() async {
        var shortcuts = GlobalShortcuts.defaults
        shortcuts[.openMenu] = GlobalShortcut(key: "y", modifiers: .command)
        shortcuts[.openQuickTrack] = GlobalShortcut(key: "u", modifiers: .option)
        XCTAssertEqual(shortcuts.openMenu, GlobalShortcut(key: "y", modifiers: .command))
        XCTAssertEqual(shortcuts.openQuickTrack, GlobalShortcut(key: "u", modifiers: .option))
    }

    func testOpenMenuRequiresACommandControlOrOptionModifier() async {
        for modifiers: GlobalShortcut.Modifiers in [[], .shift] {
            var shortcuts = GlobalShortcuts.defaults
            shortcuts.openMenu = GlobalShortcut(key: "x", modifiers: modifiers)
            XCTAssertEqual(
                shortcuts.validationError,
                "Open menu needs Command, Control or Option to avoid capturing normal typing.")
        }
        for modifiers: GlobalShortcut.Modifiers in [.command, .control, .option, [.shift, .command]] {
            var shortcuts = GlobalShortcuts.defaults
            shortcuts.openMenu = GlobalShortcut(key: "x", modifiers: modifiers)
            XCTAssertNil(shortcuts.validationError)
        }
    }

    func testQuickTrackNeedsACommandControlOrOptionModifier() async {
        for modifiers: GlobalShortcut.Modifiers in [[], .shift] {
            var shortcuts = GlobalShortcuts.defaults
            shortcuts.openQuickTrack = GlobalShortcut(key: "x", modifiers: modifiers)
            XCTAssertEqual(
                shortcuts.validationError,
                "Open Quick Track needs Command, Control or Option to avoid capturing normal typing.")
            shortcuts.openQuickTrack = GlobalShortcut(key: " ", modifiers: modifiers)
            XCTAssertEqual(
                shortcuts.validationError,
                "Open Quick Track needs Command, Control or Option to avoid capturing normal typing.")
        }
        var shortcuts = GlobalShortcuts.defaults
        shortcuts.openQuickTrack = GlobalShortcut(key: "x", modifiers: .command)
        XCTAssertNil(shortcuts.validationError)
    }

    func testReservedAndInvalidKeysCannotBeConfigured() async {
        for key in [
            "", " ", "\r", "\n", "\u{1b}", "up", "down", "return", "escape", "ab", "\u{f700}", "\u{f701}", "★", "é",
        ] {
            var shortcuts = GlobalShortcuts.defaults
            shortcuts.openMenu = GlobalShortcut(key: key, modifiers: .command)
            XCTAssertTrue(
                shortcuts.validationErrors().contains {
                    $0 == "Open menu needs one printable ASCII key. Space, arrow keys, Return and Escape are reserved."
                }, "Rejected key: \(key.debugDescription)")
        }
        var shortcuts = GlobalShortcuts.defaults
        shortcuts.openMenu = GlobalShortcut(key: "/", modifiers: .command)
        XCTAssertNil(shortcuts.validationError)
    }

    func testKeysAreNormalizedToLowercase() async {
        var shortcuts = GlobalShortcuts.defaults
        shortcuts.openMenu = GlobalShortcut(key: "C", modifiers: [.shift, .command])
        XCTAssertEqual(shortcuts.openMenu.key, "c")
        XCTAssertEqual(shortcuts.openMenu.displayText, "⇧⌘C")
        shortcuts.openMenu.key = "N"
        XCTAssertNil(shortcuts.validationError)
    }

    func testQuickTrackMustDifferFromOpenMenu() async {
        var shortcuts = GlobalShortcuts.defaults
        shortcuts.openQuickTrack = shortcuts.openMenu
        XCTAssertEqual(
            shortcuts.validationError,
            "Open Quick Track and Open menu use the same shortcut, ⌃⌥T.")
        shortcuts = .defaults
        shortcuts.openMenu = shortcuts.openQuickTrack
        XCTAssertFalse(shortcuts.validationErrors().isEmpty)
    }

    func testOnlyQuickTrackAcceptsSpace() async {
        var shortcuts = GlobalShortcuts.defaults
        shortcuts.openMenu = GlobalShortcut(key: " ", modifiers: [.command])
        XCTAssertEqual(
            shortcuts.validationError,
            "Open menu needs one printable ASCII key. Space, arrow keys, Return and Escape are reserved.")
        shortcuts = .defaults
        shortcuts.openQuickTrack = GlobalShortcut(key: "é", modifiers: [.command])
        XCTAssertEqual(
            shortcuts.validationError,
            "Open Quick Track needs one printable ASCII key or Space. Arrow keys, Return and Escape are reserved.")
    }

    func testCustomizedShortcutsRoundTripThroughJSON() async throws {
        var shortcuts = GlobalShortcuts.defaults
        shortcuts.openMenu = GlobalShortcut(key: "y", modifiers: [.control, .option])
        shortcuts.openQuickTrack = GlobalShortcut(key: " ", modifiers: [.command, .shift])
        let loaded = try JSONDecoder().decode(GlobalShortcuts.self, from: JSONEncoder().encode(shortcuts))
        XCTAssertEqual(loaded, shortcuts)
    }

    func testInvalidSavedShortcutsAreRejected() async throws {
        var shortcuts = GlobalShortcuts.defaults
        shortcuts.openQuickTrack = shortcuts.openMenu
        XCTAssertThrowsError(
            try JSONDecoder().decode(GlobalShortcuts.self, from: JSONEncoder().encode(shortcuts)))
    }

    func testUnknownModifierBitsFailValidationAndDecoding() async throws {
        let unknown = GlobalShortcut.Modifiers(rawValue: 1 << 7)
        var shortcuts = GlobalShortcuts.defaults
        shortcuts.openMenu.modifiers = unknown
        XCTAssertTrue(
            shortcuts.validationErrors().contains(
                "Open menu only supports Command, Control, Option and Shift modifiers."))
        XCTAssertThrowsError(try JSONDecoder().decode(GlobalShortcut.Modifiers.self, from: Data("128".utf8)))
        for modifiers: GlobalShortcut.Modifiers in [[], .command, .control, .option, .shift, .allowed] {
            XCTAssertEqual(
                try JSONDecoder().decode(
                    GlobalShortcut.Modifiers.self,
                    from: JSONEncoder().encode(modifiers)), modifiers)
        }
        XCTAssertEqual(GlobalShortcut(key: "x", modifiers: .allowed).displayText, "⌃⌥⇧⌘X")
    }

    func testIncompleteAndMalformedSavedShortcutsAreRejected() async {
        XCTAssertThrowsError(try JSONDecoder().decode(GlobalShortcuts.self, from: Data("{}".utf8)))
        XCTAssertThrowsError(try JSONDecoder().decode(GlobalShortcuts.self, from: Data("null".utf8)))
        XCTAssertThrowsError(try JSONDecoder().decode(GlobalShortcuts.self, from: Data("invalid".utf8)))
        let missingQuickTrack = Data(#"{"openMenu":{"key":"y","modifiers":6}}"#.utf8)
        XCTAssertThrowsError(try JSONDecoder().decode(GlobalShortcuts.self, from: missingQuickTrack))
    }
}

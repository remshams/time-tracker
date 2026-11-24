import XCTest
@testable import TrackerClient

@MainActor
final class MenuKeyboardShortcutsTests: XCTestCase {
    func testDefaultsSeparateGlobalAndMenuBindings() async throws {
        let shortcuts = MenuKeyboardShortcuts.defaults
        XCTAssertEqual(shortcuts.openMenu, MenuShortcut(key: "t", modifiers: [.control, .option]))
        XCTAssertEqual(shortcuts.moveDown, MenuShortcut(key: "j"))
        XCTAssertEqual(shortcuts.moveUp, MenuShortcut(key: "k"))
        XCTAssertEqual(shortcuts.copyName, MenuShortcut(key: "c"))
        XCTAssertEqual(shortcuts.copyExact, MenuShortcut(key: "t"))
        XCTAssertEqual(shortcuts.copyRounded, MenuShortcut(key: "s"))
        XCTAssertTrue(shortcuts.validationErrors().isEmpty)
        XCTAssertEqual(shortcuts.action(forKey: "t", modifiers: []), .copyExact)
        XCTAssertNil(shortcuts.action(forKey: "t", modifiers: [.control, .option]))
        XCTAssertEqual(shortcuts.action(forKey: "t", modifiers: [.control, .option], includingGlobal: true), .openMenu)
    }

    func testCustomizedBindingsRoundTripThroughJSON() async throws {
        var shortcuts = MenuKeyboardShortcuts.defaults
        shortcuts[.copyName] = MenuShortcut(key: "n", modifiers: [.command])
        shortcuts[.copyExact] = MenuShortcut(key: "c")
        shortcuts[.moveDown] = MenuShortcut(key: "d", modifiers: [.shift])
        let data = try JSONEncoder().encode(shortcuts)
        let loaded = try JSONDecoder().decode(MenuKeyboardShortcuts.self, from: data)
        XCTAssertEqual(loaded, shortcuts)
        XCTAssertEqual(loaded.action(forKey: "c", modifiers: []), .copyExact)
        XCTAssertEqual(loaded.action(forKey: "n", modifiers: [.command]), .copyName)
    }

    func testRemappingCopyToExactRequiresMovingTheNameBinding() async throws {
        var shortcuts = MenuKeyboardShortcuts.defaults
        shortcuts.copyExact = MenuShortcut(key: "c")
        XCTAssertEqual(
            shortcuts.validationError,
            "Copy exact duration and Copy task name use the same shortcut, C.")
        XCTAssertThrowsError(
            try JSONDecoder().decode(
                MenuKeyboardShortcuts.self,
                from: JSONEncoder().encode(shortcuts)))
        shortcuts.copyName = MenuShortcut(key: "n")
        XCTAssertNil(shortcuts.validationError)
        XCTAssertEqual(shortcuts.action(forKey: "c", modifiers: []), .copyExact)
    }

    func testGlobalAndLocalCanUseTheSameKeyWithDifferentModifiers() async {
        var shortcuts = MenuKeyboardShortcuts.defaults
        shortcuts.openMenu = MenuShortcut(key: "c", modifiers: [.command])
        XCTAssertTrue(shortcuts.validationErrors().isEmpty)
        XCTAssertEqual(shortcuts.action(forKey: "c", modifiers: []), .copyName)
        XCTAssertEqual(shortcuts.action(forKey: "c", modifiers: [.command], includingGlobal: true), .openMenu)
        shortcuts.copyExact = shortcuts.openMenu
        XCTAssertTrue(
            shortcuts.validationErrors().contains {
                $0 == "Copy exact duration and Open menu use the same shortcut, ⌘C."
            })
    }

    func testGlobalShortcutRequiresACommandControlOrOptionModifier() async {
        for modifiers: MenuShortcut.Modifiers in [[], .shift] {
            var shortcuts = MenuKeyboardShortcuts.defaults
            shortcuts.openMenu = MenuShortcut(key: "x", modifiers: modifiers)
            XCTAssertEqual(
                shortcuts.validationError,
                "Open menu needs Command, Control or Option to avoid capturing normal typing.")
        }
        for modifiers: MenuShortcut.Modifiers in [.command, .control, .option, [.shift, .command]] {
            var shortcuts = MenuKeyboardShortcuts.defaults
            shortcuts.openMenu = MenuShortcut(key: "x", modifiers: modifiers)
            XCTAssertNil(shortcuts.validationError)
        }
    }

    func testReservedAndInvalidKeysCannotBeConfigured() async {
        for key in [
            "", " ", "\r", "\n", "\u{1b}", "up", "down", "return", "escape", "ab", "\u{f700}", "\u{f701}", "★", "é",
            "K",
        ] {
            var shortcuts = MenuKeyboardShortcuts.defaults
            shortcuts.copyName = MenuShortcut(key: key)
            XCTAssertTrue(
                shortcuts.validationErrors().contains {
                    $0
                        == "Copy task name needs one printable ASCII key. Space, arrow keys, Return and Escape are reserved."
                }, "Rejected key: \(key.debugDescription)")
            XCTAssertNil(shortcuts.action(forKey: key, modifiers: []))
        }
        var shortcuts = MenuKeyboardShortcuts.defaults
        shortcuts.copyName = MenuShortcut(key: "/")
        XCTAssertNil(shortcuts.validationError)
        XCTAssertEqual(shortcuts.action(forKey: "/", modifiers: []), .copyName)
    }

    func testMatchingNormalizesCaseAndRequiresExactModifiers() async throws {
        var shortcuts = MenuKeyboardShortcuts.defaults
        shortcuts.copyName = MenuShortcut(key: "C", modifiers: [.shift, .command])
        XCTAssertEqual(shortcuts.copyName.key, "c")
        XCTAssertEqual(shortcuts.copyName.displayText, "⇧⌘C")
        XCTAssertEqual(shortcuts.action(forKey: "C", modifiers: [.shift, .command]), .copyName)
        XCTAssertNil(shortcuts.action(forKey: "c", modifiers: [.command]))
        XCTAssertNil(shortcuts.action(forKey: "c", modifiers: [.shift, .command, .option]))
        shortcuts.copyName.key = "N"
        XCTAssertEqual(shortcuts.action(forKey: "n", modifiers: [.shift, .command]), .copyName)
    }

    func testQuickTrackDefaultsToControlOptionSpace() async {
        let shortcuts = MenuKeyboardShortcuts.defaults
        XCTAssertEqual(shortcuts.openQuickTrack, MenuShortcut(key: " ", modifiers: [.control, .option]))
        XCTAssertEqual(shortcuts.openQuickTrack.displayText, "⌃⌥Space")
        XCTAssertEqual(MenuShortcut(key: " ", modifiers: [.shift, .command]).displayText, "⇧⌘Space")
        XCTAssertEqual(MenuShortcutAction.openQuickTrack.title, "Open Quick Track")
        XCTAssertTrue(shortcuts.validationErrors().isEmpty)
        XCTAssertEqual(shortcuts[.openQuickTrack], shortcuts.openQuickTrack)
        XCTAssertEqual(MenuShortcutAction.allCases.filter(\.isGlobal), [.openMenu, .openQuickTrack])
    }

    func testQuickTrackNeedsACommandControlOrOptionModifier() async {
        for modifiers: MenuShortcut.Modifiers in [[], .shift] {
            var shortcuts = MenuKeyboardShortcuts.defaults
            shortcuts.openQuickTrack = MenuShortcut(key: "x", modifiers: modifiers)
            XCTAssertEqual(
                shortcuts.validationError,
                "Open Quick Track needs Command, Control or Option to avoid capturing normal typing.")
            shortcuts.openQuickTrack = MenuShortcut(key: " ", modifiers: modifiers)
            XCTAssertEqual(
                shortcuts.validationError,
                "Open Quick Track needs Command, Control or Option to avoid capturing normal typing.")
        }
        var shortcuts = MenuKeyboardShortcuts.defaults
        shortcuts.openQuickTrack = MenuShortcut(key: "x", modifiers: .command)
        XCTAssertNil(shortcuts.validationError)
    }

    func testQuickTrackMustDifferFromOpenMenu() async {
        var shortcuts = MenuKeyboardShortcuts.defaults
        shortcuts.openQuickTrack = shortcuts.openMenu
        XCTAssertEqual(
            shortcuts.validationError,
            "Open Quick Track and Open menu use the same shortcut, ⌃⌥T.")
        shortcuts = .defaults
        shortcuts.openMenu = shortcuts.openQuickTrack
        XCTAssertFalse(shortcuts.validationErrors().isEmpty)
    }

    func testOnlyQuickTrackAcceptsSpace() async {
        for action in MenuShortcutAction.allCases where action != .openQuickTrack {
            var shortcuts = MenuKeyboardShortcuts.defaults
            shortcuts[action] = MenuShortcut(key: " ", modifiers: [.command])
            XCTAssertTrue(
                shortcuts.validationErrors().contains {
                    $0
                        == "\(action.title) needs one printable ASCII key. Space, arrow keys, Return and Escape are reserved."
                }, "\(action)")
        }
        var shortcuts = MenuKeyboardShortcuts.defaults
        shortcuts.openQuickTrack = MenuShortcut(key: "é", modifiers: [.command])
        XCTAssertEqual(
            shortcuts.validationError,
            "Open Quick Track needs one printable ASCII key or Space. Arrow keys, Return and Escape are reserved.")
    }

    func testSavedBindingsWithoutQuickTrackGetTheDefault() async throws {
        let data = try JSONEncoder().encode(MenuKeyboardShortcuts.defaults)
        var object = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])
        XCTAssertNotNil(object.removeValue(forKey: "openQuickTrack"))
        object["openMenu"] = ["key": "y", "modifiers": 6]
        let old = try JSONSerialization.data(withJSONObject: object)
        let loaded = try JSONDecoder().decode(MenuKeyboardShortcuts.self, from: old)
        XCTAssertEqual(loaded.openQuickTrack, MenuKeyboardShortcuts.defaults.openQuickTrack)
        XCTAssertEqual(loaded.openMenu, MenuShortcut(key: "y", modifiers: [.control, .option]))
    }

    func testQuickTrackBindingRoundTripsAndInvalidSavedValuesAreRejected() async throws {
        var shortcuts = MenuKeyboardShortcuts.defaults
        shortcuts.openQuickTrack = MenuShortcut(key: " ", modifiers: [.command, .shift])
        let loaded = try JSONDecoder().decode(
            MenuKeyboardShortcuts.self, from: JSONEncoder().encode(shortcuts))
        XCTAssertEqual(loaded, shortcuts)
        shortcuts.openQuickTrack = shortcuts.openMenu
        XCTAssertThrowsError(
            try JSONDecoder().decode(MenuKeyboardShortcuts.self, from: JSONEncoder().encode(shortcuts)))
    }

    func testUnknownModifierBitsFailValidationAndDecoding() async throws {
        let unknown = MenuShortcut.Modifiers(rawValue: 1 << 7)
        var shortcuts = MenuKeyboardShortcuts.defaults
        shortcuts.copyName.modifiers = unknown
        XCTAssertEqual(
            shortcuts.validationError,
            "Copy task name only supports Command, Control, Option and Shift modifiers.")
        XCTAssertNil(shortcuts.action(forKey: "c", modifiers: unknown))
        XCTAssertThrowsError(try JSONDecoder().decode(MenuShortcut.Modifiers.self, from: Data("128".utf8)))
        for modifiers: MenuShortcut.Modifiers in [[], .command, .control, .option, .shift, .allowed] {
            XCTAssertEqual(
                try JSONDecoder().decode(
                    MenuShortcut.Modifiers.self,
                    from: JSONEncoder().encode(modifiers)), modifiers)
        }
        XCTAssertEqual(MenuShortcut(key: "x", modifiers: .allowed).displayText, "⌃⌥⇧⌘X")
    }

    func testIncompleteAndMalformedSavedBindingsAreRejected() async {
        XCTAssertThrowsError(try JSONDecoder().decode(MenuKeyboardShortcuts.self, from: Data("{}".utf8)))
        XCTAssertThrowsError(try JSONDecoder().decode(MenuKeyboardShortcuts.self, from: Data("null".utf8)))
        XCTAssertThrowsError(try JSONDecoder().decode(MenuKeyboardShortcuts.self, from: Data("invalid".utf8)))
    }
}

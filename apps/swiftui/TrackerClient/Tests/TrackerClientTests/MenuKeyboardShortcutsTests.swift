import XCTest
@testable import TrackerClient

final class MenuKeyboardShortcutsTests: XCTestCase {
    func testDefaultsSeparateGlobalAndMenuBindings() throws {
        let shortcuts = MenuKeyboardShortcuts.defaults
        XCTAssertEqual(shortcuts.openMenu, MenuShortcut(key: "t", modifiers: [.control, .option]))
        XCTAssertEqual(shortcuts.moveDown, MenuShortcut(key: "j"))
        XCTAssertEqual(shortcuts.moveUp, MenuShortcut(key: "k"))
        XCTAssertEqual(shortcuts.copyName, MenuShortcut(key: "c"))
        XCTAssertEqual(shortcuts.copyExact, MenuShortcut(key: "t"))
        XCTAssertEqual(shortcuts.copyRounded, MenuShortcut(key: "s"))
        XCTAssertTrue(shortcuts.validationErrors().isEmpty)
        XCTAssertEqual(try shortcuts.validated(), shortcuts)
        XCTAssertEqual(shortcuts.action(forKey: "t", modifiers: []), .copyExact)
        XCTAssertNil(shortcuts.action(forKey: "t", modifiers: [.control, .option]))
        XCTAssertEqual(shortcuts.action(forKey: "t", modifiers: [.control, .option], includingGlobal: true), .openMenu)
    }

    func testCustomizedBindingsRoundTripThroughJSON() throws {
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

    func testRemappingCopyToExactRequiresMovingTheNameBinding() throws {
        var shortcuts = MenuKeyboardShortcuts.defaults
        shortcuts.copyExact = MenuShortcut(key: "c")
        XCTAssertEqual(shortcuts.validationError,
                       "Copy exact duration and Copy task name use the same shortcut, C.")
        XCTAssertThrowsError(try shortcuts.validated())
        XCTAssertThrowsError(try JSONDecoder().decode(MenuKeyboardShortcuts.self,
                                                       from: JSONEncoder().encode(shortcuts)))
        shortcuts.copyName = MenuShortcut(key: "n")
        XCTAssertNil(shortcuts.validationError)
        XCTAssertEqual(shortcuts.action(forKey: "c", modifiers: []), .copyExact)
    }

    func testGlobalAndLocalCanUseTheSameKeyWithDifferentModifiers() {
        var shortcuts = MenuKeyboardShortcuts.defaults
        shortcuts.openMenu = MenuShortcut(key: "c", modifiers: [.command])
        XCTAssertTrue(shortcuts.validationErrors().isEmpty)
        XCTAssertEqual(shortcuts.action(forKey: "c", modifiers: []), .copyName)
        XCTAssertEqual(shortcuts.action(forKey: "c", modifiers: [.command], includingGlobal: true), .openMenu)
        shortcuts.copyExact = shortcuts.openMenu
        XCTAssertTrue(shortcuts.validationErrors().contains {
            $0 == "Copy exact duration and Open menu use the same shortcut, ⌘C."
        })
    }

    func testGlobalShortcutRequiresACommandControlOrOptionModifier() {
        for modifiers: MenuShortcut.Modifiers in [[], .shift] {
            var shortcuts = MenuKeyboardShortcuts.defaults
            shortcuts.openMenu = MenuShortcut(key: "x", modifiers: modifiers)
            XCTAssertEqual(shortcuts.validationError,
                           "Open menu needs Command, Control or Option to avoid capturing normal typing.")
        }
        for modifiers: MenuShortcut.Modifiers in [.command, .control, .option, [.shift, .command]] {
            var shortcuts = MenuKeyboardShortcuts.defaults
            shortcuts.openMenu = MenuShortcut(key: "x", modifiers: modifiers)
            XCTAssertNil(shortcuts.validationError)
        }
    }

    func testReservedAndInvalidKeysCannotBeConfigured() {
        for key in ["", " ", "\r", "\n", "\u{1b}", "up", "down", "return", "escape", "ab", "\u{f700}", "\u{f701}", "★", "é", "K"] {
            var shortcuts = MenuKeyboardShortcuts.defaults
            shortcuts.copyName = MenuShortcut(key: key)
            XCTAssertTrue(shortcuts.validationErrors().contains {
                $0 == "Copy task name needs one printable ASCII key. Space, arrow keys, Return and Escape are reserved."
            }, "Rejected key: \(key.debugDescription)")
            XCTAssertNil(shortcuts.action(forKey: key, modifiers: []))
        }
        var shortcuts = MenuKeyboardShortcuts.defaults
        shortcuts.copyName = MenuShortcut(key: "/")
        XCTAssertNil(shortcuts.validationError)
        XCTAssertEqual(shortcuts.action(forKey: "/", modifiers: []), .copyName)
    }

    func testMatchingNormalizesCaseAndRequiresExactModifiers() throws {
        var shortcuts = MenuKeyboardShortcuts.defaults
        shortcuts.copyName = MenuShortcut(key: "C", modifiers: [.shift, .command])
        XCTAssertEqual(shortcuts.copyName.key, "c")
        XCTAssertEqual(shortcuts.copyName.displayText, "⇧⌘C")
        XCTAssertEqual(shortcuts.action(forKey: "C", modifiers: [.shift, .command]), .copyName)
        XCTAssertNil(shortcuts.action(forKey: "c", modifiers: [.command]))
        XCTAssertNil(shortcuts.action(forKey: "c", modifiers: [.shift, .command, .option]))
        shortcuts.copyName.key = "N"
        XCTAssertEqual(try shortcuts.validated().copyName.key, "n")
        XCTAssertEqual(shortcuts.action(forKey: "n", modifiers: [.shift, .command]), .copyName)
    }

    func testUnknownModifierBitsFailValidationAndDecoding() throws {
        let unknown = MenuShortcut.Modifiers(rawValue: 1 << 7)
        var shortcuts = MenuKeyboardShortcuts.defaults
        shortcuts.copyName.modifiers = unknown
        XCTAssertEqual(shortcuts.validationError,
                       "Copy task name only supports Command, Control, Option and Shift modifiers.")
        XCTAssertNil(shortcuts.action(forKey: "c", modifiers: unknown))
        XCTAssertThrowsError(try JSONDecoder().decode(MenuShortcut.Modifiers.self, from: Data("128".utf8)))
        for modifiers: MenuShortcut.Modifiers in [[], .command, .control, .option, .shift, .allowed] {
            XCTAssertEqual(try JSONDecoder().decode(MenuShortcut.Modifiers.self,
                                                    from: JSONEncoder().encode(modifiers)), modifiers)
        }
        XCTAssertEqual(MenuShortcut(key: "x", modifiers: .allowed).displayText, "⌃⌥⇧⌘X")
    }

    func testIncompleteAndMalformedSavedBindingsAreRejected() {
        XCTAssertThrowsError(try JSONDecoder().decode(MenuKeyboardShortcuts.self, from: Data("{}".utf8)))
        XCTAssertThrowsError(try JSONDecoder().decode(MenuKeyboardShortcuts.self, from: Data("null".utf8)))
        XCTAssertThrowsError(try JSONDecoder().decode(MenuKeyboardShortcuts.self, from: Data("invalid".utf8)))
    }

    func testExactDurationPreservesSecondsAndDropsLeadingZeroUnits() {
        XCTAssertEqual(MenuDurationFormatter.exact(5_025), "1h 23m 45s")
        XCTAssertEqual(MenuDurationFormatter.exact(1_425), "23m 45s")
        XCTAssertEqual(MenuDurationFormatter.exact(45.99), "45s")
        XCTAssertEqual(MenuDurationFormatter.exact(3_600), "1h 0m 0s")
        XCTAssertEqual(MenuDurationFormatter.exact(0), "0s")
        XCTAssertEqual(MenuDurationFormatter.exact(-10), "0s")
    }

    func testRoundedDurationUsesNearestQuarterHourAndRoundsTiesUp() {
        XCTAssertEqual(MenuDurationFormatter.rounded(449), "0m")
        XCTAssertEqual(MenuDurationFormatter.rounded(450), "15m")
        XCTAssertEqual(MenuDurationFormatter.rounded(1_349), "15m")
        XCTAssertEqual(MenuDurationFormatter.rounded(1_350), "30m")
        XCTAssertEqual(MenuDurationFormatter.rounded(5_025), "1h 30m")
        XCTAssertEqual(MenuDurationFormatter.rounded(3_600), "1h 0m")
        XCTAssertEqual(MenuDurationFormatter.rounded(-450), "0m")
    }

    func testUnavailableNonfiniteAndOversizedDurationsHaveNoCopyValue() {
        for duration: TimeInterval? in [nil, .nan, .infinity, -.infinity, .greatestFiniteMagnitude, Double(Int64.max)] {
            XCTAssertNil(MenuDurationFormatter.exact(duration))
            XCTAssertNil(MenuDurationFormatter.rounded(duration))
        }
    }
}

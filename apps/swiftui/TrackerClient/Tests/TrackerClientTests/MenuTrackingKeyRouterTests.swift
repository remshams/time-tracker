import XCTest
@testable import TrackerClient

final class MenuTrackingKeyRouterTests: XCTestCase {
    func testCopyPressPerformsOnceAndConsumesRepeatsAndRelease() {
        for action in [MenuShortcutAction.copyName, .copyExact, .copyRounded] {
            var router = MenuTrackingKeyRouter()
            let binding = MenuKeyboardShortcuts.defaults[action]
            XCTAssertEqual(router.route(keyCode: 8, key: binding.key, modifiers: binding.modifiers,
                                        phase: .down, isRepeat: false, shortcuts: .defaults), .perform(action))
            XCTAssertEqual(router.route(keyCode: 8, key: nil, modifiers: [],
                                        phase: .down, isRepeat: true, shortcuts: .defaults), .consume)
            XCTAssertEqual(router.route(keyCode: 8, key: nil, modifiers: [],
                                        phase: .up, isRepeat: false, shortcuts: .defaults), .consume)
            XCTAssertEqual(router.route(keyCode: 8, key: binding.key, modifiers: binding.modifiers,
                                        phase: .down, isRepeat: false, shortcuts: .defaults), .perform(action))
        }
    }

    func testNavigationPressRepeatAndReleaseKeepTheOriginalDirection() {
        for action in [MenuShortcutAction.moveDown, .moveUp] {
            var router = MenuTrackingKeyRouter()
            let binding = MenuKeyboardShortcuts.defaults[action]
            XCTAssertEqual(router.route(keyCode: 38, key: binding.key, modifiers: [],
                                        phase: .down, isRepeat: false, shortcuts: .defaults), .navigate(action))
            XCTAssertEqual(router.route(keyCode: 38, key: nil, modifiers: [.shift],
                                        phase: .down, isRepeat: true, shortcuts: .defaults), .navigate(action))
            XCTAssertEqual(router.route(keyCode: 38, key: nil, modifiers: [],
                                        phase: .up, isRepeat: false, shortcuts: .defaults), .navigate(action))
            XCTAssertEqual(router.route(keyCode: 38, key: binding.key, modifiers: [],
                                        phase: .up, isRepeat: false, shortcuts: .defaults), .passThrough)
        }
    }

    func testModifiedShortcutReleaseDoesNotNeedTheOriginalModifiers() {
        var shortcuts = MenuKeyboardShortcuts.defaults
        shortcuts.copyName = MenuShortcut(key: "c", modifiers: [.command, .shift])
        var router = MenuTrackingKeyRouter()
        XCTAssertEqual(router.route(keyCode: 8, key: "C", modifiers: [.command, .shift],
                                    phase: .down, isRepeat: false, shortcuts: shortcuts), .perform(.copyName))
        XCTAssertEqual(router.route(keyCode: 8, key: "c", modifiers: [],
                                    phase: .up, isRepeat: false, shortcuts: shortcuts), .consume)
    }

    func testShortcutChangesWhileHeldApplyAfterTheOriginalRelease() {
        var router = MenuTrackingKeyRouter()
        XCTAssertEqual(router.route(keyCode: 38, key: "j", modifiers: [],
                                    phase: .down, isRepeat: false, shortcuts: .defaults), .navigate(.moveDown))
        var changed = MenuKeyboardShortcuts.defaults
        changed.moveDown = MenuShortcut(key: "k")
        changed.moveUp = MenuShortcut(key: "j")
        XCTAssertEqual(router.route(keyCode: 38, key: "j", modifiers: [],
                                    phase: .down, isRepeat: true, shortcuts: changed), .navigate(.moveDown))
        XCTAssertEqual(router.route(keyCode: 38, key: "j", modifiers: [],
                                    phase: .up, isRepeat: false, shortcuts: changed), .navigate(.moveDown))
        XCTAssertEqual(router.route(keyCode: 38, key: "j", modifiers: [],
                                    phase: .down, isRepeat: false, shortcuts: changed), .navigate(.moveUp))
    }

    func testUnknownKeysAndNativeMenuKeysPassThrough() {
        var router = MenuTrackingKeyRouter()
        for phase in [MenuTrackingKeyPhase.down, .up] {
            for code: UInt16 in [36, 49, 53, 76, 123, 124, 125, 126] {
                XCTAssertEqual(router.route(keyCode: code, key: "j", modifiers: [],
                                            phase: phase, isRepeat: false, shortcuts: .defaults), .passThrough)
            }
            for key: String? in [nil, "x", "🕒", "\u{f701}"] {
                XCTAssertEqual(router.route(keyCode: 7, key: key, modifiers: [],
                                            phase: phase, isRepeat: false, shortcuts: .defaults), .passThrough)
            }
        }
        XCTAssertEqual(router.route(keyCode: 38, key: "j", modifiers: [.option],
                                    phase: .down, isRepeat: false, shortcuts: .defaults), .passThrough)
    }

    func testResetReleasesAllHeldBindings() {
        var router = MenuTrackingKeyRouter()
        XCTAssertEqual(router.route(keyCode: 38, key: "j", modifiers: [],
                                    phase: .down, isRepeat: false, shortcuts: .defaults), .navigate(.moveDown))
        XCTAssertEqual(router.route(keyCode: 8, key: "c", modifiers: [],
                                    phase: .down, isRepeat: false, shortcuts: .defaults), .perform(.copyName))
        router.reset()
        for code: UInt16 in [38, 8] {
            XCTAssertEqual(router.route(keyCode: code, key: nil, modifiers: [],
                                        phase: .up, isRepeat: false, shortcuts: .defaults), .passThrough)
        }
    }

    func testGlobalShortcutPerformsOnceAndAnOpeningChordRepeatOnlyConsumes() {
        var router = MenuTrackingKeyRouter()
        XCTAssertEqual(router.route(keyCode: 17, key: "t", modifiers: [.control, .option],
                                    phase: .down, isRepeat: false, shortcuts: .defaults), .perform(.openMenu))
        XCTAssertEqual(router.route(keyCode: 17, key: nil, modifiers: [],
                                    phase: .down, isRepeat: true, shortcuts: .defaults), .consume)
        XCTAssertEqual(router.route(keyCode: 17, key: nil, modifiers: [],
                                    phase: .up, isRepeat: false, shortcuts: .defaults), .consume)
        router.reset()
        XCTAssertEqual(router.route(keyCode: 17, key: "t", modifiers: [.control, .option],
                                    phase: .down, isRepeat: true, shortcuts: .defaults), .consume)
    }

    func testASecondPhysicalKeyKeepsItsOwnReleaseAction() {
        var router = MenuTrackingKeyRouter()
        XCTAssertEqual(router.route(keyCode: 38, key: "j", modifiers: [],
                                    phase: .down, isRepeat: false, shortcuts: .defaults), .navigate(.moveDown))
        XCTAssertEqual(router.route(keyCode: 40, key: "k", modifiers: [],
                                    phase: .down, isRepeat: false, shortcuts: .defaults), .navigate(.moveUp))
        XCTAssertEqual(router.route(keyCode: 38, key: nil, modifiers: [],
                                    phase: .up, isRepeat: false, shortcuts: .defaults), .navigate(.moveDown))
        XCTAssertEqual(router.route(keyCode: 40, key: nil, modifiers: [],
                                    phase: .up, isRepeat: false, shortcuts: .defaults), .navigate(.moveUp))
    }

    func testFreshPressReplacesABindingWhoseReleaseWasLost() {
        var router = MenuTrackingKeyRouter()
        XCTAssertEqual(router.route(keyCode: 38, key: "j", modifiers: [],
                                    phase: .down, isRepeat: false, shortcuts: .defaults), .navigate(.moveDown))
        XCTAssertEqual(router.route(keyCode: 38, key: "x", modifiers: [],
                                    phase: .down, isRepeat: false, shortcuts: .defaults), .passThrough)
        XCTAssertEqual(router.route(keyCode: 38, key: nil, modifiers: [],
                                    phase: .up, isRepeat: false, shortcuts: .defaults), .passThrough)
    }

    func testExcessHeldKeysPassThroughAndReleasingOneAllowsAnotherBinding() {
        var router = MenuTrackingKeyRouter()
        for code: UInt16 in 200..<328 {
            XCTAssertEqual(router.route(keyCode: code, key: "j", modifiers: [],
                                        phase: .down, isRepeat: false, shortcuts: .defaults), .navigate(.moveDown))
        }
        XCTAssertEqual(router.route(keyCode: 328, key: "j", modifiers: [],
                                    phase: .down, isRepeat: false, shortcuts: .defaults), .passThrough)
        XCTAssertEqual(router.route(keyCode: 328, key: nil, modifiers: [],
                                    phase: .up, isRepeat: false, shortcuts: .defaults), .passThrough)
        XCTAssertEqual(router.route(keyCode: 200, key: nil, modifiers: [],
                                    phase: .up, isRepeat: false, shortcuts: .defaults), .navigate(.moveDown))
        XCTAssertEqual(router.route(keyCode: 328, key: "j", modifiers: [],
                                    phase: .down, isRepeat: false, shortcuts: .defaults), .navigate(.moveDown))
    }
}

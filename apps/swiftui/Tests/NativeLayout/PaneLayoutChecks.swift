import AppKit
import SwiftUI

@main
@MainActor
enum PaneLayoutChecks {
    static func main() throws {
        NSApplication.shared.setActivationPolicy(.accessory)
        for insetParent in [false, true] {
            try checkPane(insetParent: insetParent)
        }
        print("Native pane layout checks passed.")
    }

    private static func checkPane(insetParent: Bool) throws {
        let window = TrackerWindowChrome.makeWindow()
        window.setFrameOrigin(NSPoint(x: 100, y: 100))
        window.isReleasedWhenClosed = false
        defer { window.close() }
        let root = NSViewController()
        root.view = NSView()
        window.contentViewController = root
        let pane = TrackerPaneViewController(content: AnyView(
            VStack(alignment: .leading) {
                Text("A task heading that must remain below the toolbar")
                    .font(.title)
                Spacer()
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        ))
        root.addChild(pane)
        pane.view.translatesAutoresizingMaskIntoConstraints = false
        root.view.addSubview(pane.view)
        guard let contentGuide = window.contentLayoutGuide as? NSLayoutGuide else {
            throw LayoutFailure(message: "The window has no content layout guide")
        }
        NSLayoutConstraint.activate([
            pane.view.leadingAnchor.constraint(equalTo: root.view.leadingAnchor),
            pane.view.trailingAnchor.constraint(equalTo: root.view.trailingAnchor),
            pane.view.bottomAnchor.constraint(equalTo: root.view.bottomAnchor),
            pane.view.topAnchor.constraint(equalTo: insetParent ? contentGuide.topAnchor : root.view.topAnchor)
        ])
        let toolbarDelegate = LayoutToolbarDelegate()
        let toolbar = NSToolbar(identifier: NSToolbar.Identifier(UUID().uuidString))
        toolbar.delegate = toolbarDelegate
        toolbar.displayMode = .iconOnly
        window.toolbar = toolbar
        window.orderFront(nil)

        for style in [NSWindow.ToolbarStyle.unifiedCompact, .expanded] {
            window.toolbarStyle = style
            for size in [NSSize(width: 760, height: 480), NSSize(width: 1100, height: 760)] {
                window.setContentSize(size)
                for visible in [true, false, true] {
                    toolbar.isVisible = visible
                    // Native safe areas become available after the window is visible and laid out.
                    RunLoop.main.run(until: Date().addingTimeInterval(0.05))
                    window.layoutIfNeeded()
                    root.view.layoutSubtreeIfNeeded()
                    let hosted = pane.children[0].view
                    let actual = hosted.convert(hosted.bounds, to: nil)
                    let parent = pane.view.convert(pane.view.bounds, to: nil)
                    let expected = parent.intersection(window.contentLayoutRect)
                    let tolerance: CGFloat = 1
                    if visible && !insetParent {
                        guard parent.maxY > expected.maxY + tolerance else {
                            throw LayoutFailure(message: "The fixture did not create content behind a visible toolbar")
                        }
                    }
                    guard !expected.isEmpty,
                          abs(actual.minX - expected.minX) <= tolerance,
                          abs(actual.maxX - expected.maxX) <= tolerance,
                          abs(actual.minY - expected.minY) <= tolerance,
                          abs(actual.maxY - expected.maxY) <= tolerance else {
                        throw LayoutFailure(message: "Hosted frame \(actual) differs from unobscured pane \(expected). Inset parent: \(insetParent), toolbar visible: \(visible), style: \(style.rawValue), size: \(size)")
                    }
                }
            }
        }
        withExtendedLifetime(toolbarDelegate) {}
    }
}

private struct LayoutFailure: Error, CustomStringConvertible {
    let message: String
    var description: String { message }
}

@MainActor
private final class LayoutToolbarDelegate: NSObject, NSToolbarDelegate {
    private let actionID = NSToolbarItem.Identifier("LayoutAction")

    func toolbarDefaultItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        [actionID]
    }

    func toolbarAllowedItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        toolbarDefaultItemIdentifiers(toolbar)
    }

    func toolbar(_ toolbar: NSToolbar, itemForItemIdentifier itemIdentifier: NSToolbarItem.Identifier,
                 willBeInsertedIntoToolbar flag: Bool) -> NSToolbarItem? {
        let item = NSToolbarItem(itemIdentifier: itemIdentifier)
        item.image = NSImage(systemSymbolName: "plus", accessibilityDescription: "New task")
        item.label = "New task"
        return item
    }
}

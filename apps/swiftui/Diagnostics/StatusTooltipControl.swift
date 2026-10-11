import AppKit
import ApplicationServices
import CoreGraphics

@MainActor
final class TooltipControl: NSObject, NSApplicationDelegate {
    private var statusItem: NSStatusItem?
    private var window: NSWindow?
    private let toolbarButton = NSButton(title: "New task", target: nil, action: nil)
    private let statusTooltip: NSString = "Tracking: Standalone tooltip control"
    private let output = CommandLine.arguments.dropFirst().first ?? "/tmp"

    func applicationDidFinishLaunching(_ notification: Notification) {
        let window = NSWindow(contentRect: NSRect(x: 100, y: 100, width: 500, height: 300),
                              styleMask: [.titled, .closable], backing: .buffered, defer: false)
        self.window = window
        window.title = "Native tooltip control"
        toolbarButton.frame = NSRect(x: 30, y: 210, width: 120, height: 40)
        toolbarButton.toolTip = "New task"
        window.contentView?.addSubview(toolbarButton)
        window.makeKeyAndOrderFront(nil)
        NSApplication.shared.activate(ignoringOtherApps: true)
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        statusItem = item
        guard let button = item.button else { fatalError("No status button") }
        button.image = NSImage(systemSymbolName: "circle.fill", accessibilityDescription: nil)
        button.imagePosition = .imageLeading
        button.title = " Tooltip control"
        button.toolTip = "Tracking: Standalone tooltip control"
        print("Post event access: \(CGPreflightPostEventAccess())")
        print("Screen capture access: \(CGPreflightScreenCaptureAccess())")
        print("Status tooltip: \(button.toolTip ?? "")")
        movePointer(to: CGPoint(x: 400, y: 400))
        DispatchQueue.main.asyncAfter(deadline: .now() + 2) {
            button.toolTip = nil
            let tag = button.addToolTip(button.bounds, owner: self.statusTooltip, userData: nil)
            print("Explicit status tooltip region: \(button.bounds), tag: \(tag)")
            self.hover(button, leadingIcon: true)
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 7) {
            self.capture("status-control")
            self.movePointer(to: CGPoint(x: 400, y: 400))
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 9) {
            self.hover(self.toolbarButton, leadingIcon: false)
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 14) {
            self.capture("toolbar-control")
            NSApplication.shared.terminate(nil)
        }
    }

    private func hover(_ button: NSButton, leadingIcon: Bool) {
        guard let window = button.window else { fatalError("Button has no window") }
        let frame = window.convertToScreen(button.convert(button.bounds, to: nil))
        let x = leadingIcon ? frame.minX + min(18, frame.width / 2) : frame.midX
        let point = CGPoint(x: x, y: CGDisplayBounds(CGMainDisplayID()).height - frame.midY)
        print("Hover frame: \(frame), Quartz point: \(point)")
        movePointer(to: point)
    }

    private func movePointer(to point: CGPoint) {
        guard let event = CGEvent(mouseEventSource: nil, mouseType: .mouseMoved,
                                  mouseCursorPosition: point, mouseButton: .left) else {
            fatalError("Cannot create mouse event")
        }
        event.post(tap: .cghidEventTap)
    }

    private func capture(_ name: String) {
        print("\(name) pointer: \(NSEvent.mouseLocation)")
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
        process.arguments = ["-x", "\(output)/\(name).png"]
        do {
            try process.run()
            process.waitUntilExit()
            print("\(name) capture result: \(process.terminationStatus)")
        } catch {
            print("\(name) capture error: \(error)")
        }
    }
}

@main
@MainActor
enum TooltipControlMain {
    static func main() {
        let app = NSApplication.shared
        app.setActivationPolicy(.regular)
        let delegate = TooltipControl()
        app.delegate = delegate
        withExtendedLifetime(delegate) { app.run() }
    }
}

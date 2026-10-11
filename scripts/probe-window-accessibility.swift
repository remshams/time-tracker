import Foundation

#if os(macOS)
    import AppKit
    import ApplicationServices

    private func report(_ event: String, _ fields: [String: Any] = [:]) {
        var record = fields
        record["event"] = event
        do {
            let data = try JSONSerialization.data(withJSONObject: record, options: [.sortedKeys])
            print(String(decoding: data, as: UTF8.self))
        } catch {
            FileHandle.standardError.write(Data("Diagnostic output could not be encoded.\n".utf8))
            exit(2)
        }
    }

    private func status(_ error: AXError) -> [String: Any] {
        ["appleAXError": error.rawValue, "appleAXErrorName": String(describing: error)]
    }

    private func read(_ element: AXUIElement, _ attribute: String, window: Int? = nil) -> CFTypeRef? {
        var value: CFTypeRef?
        let error = AXUIElementCopyAttributeValue(element, attribute as CFString, &value)
        var fields = status(error)
        fields["attribute"] = attribute
        fields["window"] = window
        if let value {
            fields["valueTypeID"] = CFGetTypeID(value)
            if [kAXRoleAttribute, kAXSubroleAttribute].contains(attribute) {
                fields["value"] = value as? String
            } else if [
                kAXFocusedAttribute, kAXMainAttribute, kAXMinimizedAttribute,
                kAXFrontmostAttribute, kAXHiddenAttribute,
            ].contains(attribute) {
                fields["value"] = value as? Bool
            }
        }
        report("read", fields)
        return error == .success ? value : nil
    }

    private func settable(_ element: AXUIElement, _ attribute: String, window: Int? = nil) -> Bool {
        var allowed = DarwinBoolean(false)
        let error = AXUIElementIsAttributeSettable(element, attribute as CFString, &allowed)
        var fields = status(error)
        fields["attribute"] = attribute
        fields["window"] = window
        fields["settable"] = allowed.boolValue
        report("settable", fields)
        return error == .success && allowed.boolValue
    }

    private func setTrue(_ element: AXUIElement, _ attribute: String, window: Int? = nil) -> Bool {
        let error = AXUIElementSetAttributeValue(element, attribute as CFString, kCFBooleanTrue)
        var fields = status(error)
        fields["attribute"] = attribute
        fields["window"] = window
        report("write", fields)
        return error == .success
    }

    private func matchesFocusedWindow(_ application: AXUIElement, _ window: AXUIElement) -> Bool {
        var focused: CFTypeRef?
        let error = AXUIElementCopyAttributeValue(application, kAXFocusedWindowAttribute as CFString, &focused)
        guard error == .success, let focused, CFGetTypeID(focused) == AXUIElementGetTypeID() else {
            return false
        }
        return CFEqual(focused, window)
    }

    @main
    @MainActor
    private struct WindowAccessibilityProbe {
        static func main() {
            let arguments = Array(CommandLine.arguments.dropFirst())
            guard arguments.count == 1 || arguments.count == 3,
                arguments.count == 1 || arguments[1] == "--focus",
                arguments.count == 1 || Int(arguments[2]).map({ $0 >= 0 }) == true
            else {
                report(
                    "prerequisite", ["message": "Usage: probe-window-accessibility BUNDLE_ID [--focus WINDOW_INDEX]"])
                exit(2)
            }
            report("environment", ["macOS": ProcessInfo.processInfo.operatingSystemVersionString])
            guard AXIsProcessTrusted() else {
                report(
                    "prerequisite",
                    ["message": "Grant this probe Accessibility access in System Settings, then rerun it."])
                exit(2)
            }
            let applications = NSRunningApplication.runningApplications(withBundleIdentifier: arguments[0])
            guard applications.count == 1, let running = applications.first else {
                report(
                    "prerequisite",
                    [
                        "message": "Exactly one running application with this bundle ID is required.",
                        "matchingProcesses": applications.count,
                    ])
                exit(2)
            }
            let application = AXUIElementCreateApplication(running.processIdentifier)
            report(
                "application",
                [
                    "bundleID": arguments[0], "pid": running.processIdentifier,
                    "active": running.isActive, "hidden": running.isHidden,
                ])
            let timeoutError = AXUIElementSetMessagingTimeout(application, 2)
            report("messagingTimeout", status(timeoutError))
            guard let windowsValue = read(application, kAXWindowsAttribute),
                let windows = windowsValue as? [AXUIElement], !windows.isEmpty
            else {
                report("failure", ["message": "The app does not expose any AX windows. Open a window before testing."])
                exit(1)
            }
            report("windows", ["count": windows.count])
            _ = read(application, kAXFrontmostAttribute)
            _ = read(application, kAXHiddenAttribute)
            _ = read(application, kAXFocusedWindowAttribute)
            _ = settable(application, kAXFrontmostAttribute)
            for (index, window) in windows.enumerated() {
                var actions: CFArray?
                let actionsError = AXUIElementCopyActionNames(window, &actions)
                var fields = status(actionsError)
                fields["window"] = index
                fields["actions"] = actions as? [String] ?? []
                report("actions", fields)
                for attribute in [
                    kAXRoleAttribute, kAXSubroleAttribute, kAXTitleAttribute,
                    kAXPositionAttribute, kAXSizeAttribute, kAXMainAttribute,
                    kAXFocusedAttribute, kAXMinimizedAttribute,
                ] {
                    _ = read(window, attribute, window: index)
                }
                _ = settable(window, kAXMainAttribute, window: index)
                _ = settable(window, kAXFocusedAttribute, window: index)
            }
            guard arguments.count == 3, let index = Int(arguments[2]) else {
                report(
                    "inspected",
                    ["message": "No focus request made. Attribute values containing user content are omitted."])
                exit(0)
            }
            guard windows.indices.contains(index) else {
                report("prerequisite", ["message": "The requested AX window index does not exist."])
                exit(2)
            }
            guard !running.isActive else {
                report("prerequisite", ["message": "Activate another app before testing a switch to this app."])
                exit(2)
            }
            let window = windows[index]
            guard read(window, kAXRoleAttribute, window: index) as? String == kAXWindowRole else {
                report("failure", ["message": "The selected AX element is not a window."])
                exit(1)
            }
            var succeeded = true
            let raiseError = AXUIElementPerformAction(window, kAXRaiseAction as CFString)
            var raiseFields = status(raiseError)
            raiseFields["window"] = index
            report("raise", raiseFields)
            succeeded = raiseError == .success
            for attribute in [kAXMainAttribute, kAXFocusedAttribute] {
                if settable(window, attribute, window: index) {
                    succeeded = setTrue(window, attribute, window: index) && succeeded
                }
            }
            succeeded = setTrue(application, kAXFrontmostAttribute) && succeeded
            let deadline = Date().addingTimeInterval(3)
            while Date() < deadline {
                if running.isActive && matchesFocusedWindow(application, window) { break }
                RunLoop.current.run(until: Date().addingTimeInterval(0.05))
            }
            let focused = matchesFocusedWindow(application, window)
            let frontmost = running.isActive
            _ = read(application, kAXFocusedWindowAttribute)
            _ = read(window, kAXMainAttribute, window: index)
            _ = read(window, kAXFocusedAttribute, window: index)
            report(
                "focusResult",
                [
                    "window": index, "frontmost": frontmost,
                    "selectedWindowFocused": focused, "AXOperationsSucceeded": succeeded,
                ])
            exit(succeeded && frontmost && focused ? 0 : 1)
        }
    }
#else
    @main
    private struct WindowAccessibilityProbe {
        static func main() {
            print("Native window accessibility diagnostics require macOS and a desktop session.")
            exit(2)
        }
    }
#endif

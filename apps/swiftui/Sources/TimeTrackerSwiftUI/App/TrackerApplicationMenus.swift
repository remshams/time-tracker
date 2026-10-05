import AppKit

@MainActor
enum TrackerApplicationMenus {
    static func install(for delegate: TrackerApplicationDelegate) {
        let application = NSApplication.shared
        let mainMenu = NSMenu(title: "Main menu")
        let appMenu = submenu("Time Tracker", in: mainMenu)
        appMenu.addItem(item("About Time Tracker", action: #selector(NSApplication.orderFrontStandardAboutPanel(_:))))
        appMenu.addItem(.separator())
        appMenu.addItem(item("Settings...", action: #selector(TrackerApplicationDelegate.openSettings(_:)),
                             key: ",", target: delegate))
        appMenu.addItem(.separator())
        let servicesMenu = submenu("Services", in: appMenu)
        application.servicesMenu = servicesMenu
        appMenu.addItem(.separator())
        appMenu.addItem(item("Hide Time Tracker", action: #selector(NSApplication.hide(_:)), key: "h"))
        let hideOthers = item("Hide Others", action: #selector(NSApplication.hideOtherApplications(_:)), key: "h")
        hideOthers.keyEquivalentModifierMask = [.command, .option]
        appMenu.addItem(hideOthers)
        appMenu.addItem(item("Show All", action: #selector(NSApplication.unhideAllApplications(_:))))
        appMenu.addItem(.separator())
        appMenu.addItem(item("Quit Time Tracker", action: #selector(NSApplication.terminate(_:)), key: "q"))

        let fileMenu = submenu("File", in: mainMenu)
        fileMenu.addItem(item("New Task", action: #selector(TrackerApplicationDelegate.createTask(_:)),
                              key: "n", target: delegate))
        let newWindow = item("New Window", action: #selector(TrackerApplicationDelegate.createWindow(_:)),
                             key: "n", target: delegate)
        newWindow.keyEquivalentModifierMask = [.command, .shift]
        fileMenu.addItem(newWindow)
        fileMenu.addItem(.separator())
        fileMenu.addItem(item("Close", action: #selector(NSWindow.performClose(_:)), key: "w"))

        let editMenu = submenu("Edit", in: mainMenu)
        editMenu.addItem(item("Undo", action: Selector("undo:"), key: "z"))
        let redo = item("Redo", action: Selector("redo:"), key: "z")
        redo.keyEquivalentModifierMask = [.command, .shift]
        editMenu.addItem(redo)
        editMenu.addItem(.separator())
        editMenu.addItem(item("Cut", action: #selector(NSText.cut(_:)), key: "x"))
        editMenu.addItem(item("Copy", action: #selector(NSText.copy(_:)), key: "c"))
        editMenu.addItem(item("Paste", action: #selector(NSText.paste(_:)), key: "v"))
        editMenu.addItem(item("Select All", action: #selector(NSText.selectAll(_:)), key: "a"))

        let windowMenu = submenu("Window", in: mainMenu)
        windowMenu.addItem(item("Show Time Tracker", action: #selector(TrackerApplicationDelegate.showMainWindow(_:)),
                                key: "0", target: delegate))
        windowMenu.addItem(.separator())
        windowMenu.addItem(item("Minimize", action: #selector(NSWindow.performMiniaturize(_:)), key: "m"))
        windowMenu.addItem(item("Zoom", action: #selector(NSWindow.performZoom(_:))))
        windowMenu.addItem(.separator())
        windowMenu.addItem(item("Bring All to Front", action: #selector(NSApplication.arrangeInFront(_:))))
        application.windowsMenu = windowMenu
        application.mainMenu = mainMenu
    }

    private static func submenu(_ title: String, in parent: NSMenu) -> NSMenu {
        let entry = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        let menu = NSMenu(title: title)
        entry.submenu = menu
        parent.addItem(entry)
        return menu
    }

    private static func item(_ title: String, action: Selector, key: String = "",
                             target: AnyObject? = nil) -> NSMenuItem {
        let entry = NSMenuItem(title: title, action: action, keyEquivalent: key)
        entry.target = target
        entry.keyEquivalentModifierMask = [.command]
        return entry
    }
}

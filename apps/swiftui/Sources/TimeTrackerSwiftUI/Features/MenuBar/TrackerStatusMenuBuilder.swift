import AppKit
import TrackerClient

/// A command a menu item runs, with the connection the menu showed when it opened.
struct TrackerMenuCommand {
    enum Action {
        case start(String), stop(String), copy(MenuShortcutAction, String), openWindow, quit
    }

    let action: Action
    let connection: ConnectionSettings
}

/// The status item's open menu and the live value fields and task rows the controller updates while it is open.
struct TrackerStatusMenu {
    let menu: NSMenu
    let elapsedField: MenuValueField?
    let totalField: MenuValueField?
    let taskRows: [String: MenuTaskRow]
}

/// Lays out the status item's `NSMenu` from the menu content. Every command item calls `itemAction` on `target`
/// with a `TrackerMenuCommand` as its represented object.
struct TrackerStatusMenuBuilder {
    let menuBar: MenuBarStore
    let connection: ConnectionSettings
    let target: AnyObject
    let itemAction: Selector

    func makeMenu(_ content: TrackerMenuContent, appearance: NSAppearance) -> TrackerStatusMenu {
        let menu = NSMenu()
        menu.appearance = appearance
        menu.autoenablesItems = false
        let serverURL =
            connection.mode == .server
            ? connection.serverURL.trimmingCharacters(in: .whitespacesAndNewlines) : nil
        addText(content.connectionStatusText, value: serverURL, to: menu)
        if content.isStale { addText("Showing last confirmed state", to: menu) }
        let elapsedField = addText(content.runningTaskName, value: content.elapsedText, to: menu, liveValue: true)
        if let status = content.autoPauseStatusText { addText(status, to: menu) }
        if let error = content.trackingError { addText(error, to: menu) }
        menu.addItem(.separator())
        addText(content.dailyTotalsStatus == .cached ? "Today, cached" : "Today", to: menu)
        let totalField = addText(
            "Total", value: content.totalText, to: menu,
            help: content.totalsExplanation, liveValue: true)
        menu.addItem(.separator())
        var taskRows: [String: MenuTaskRow] = [:]
        for entry in content.todayTasks {
            let item = addTask(
                entry, title: MenuTaskRow.title(entry, duration: entry.durationText),
                help: content.totalsExplanation, content: content,
                appearance: appearance, to: menu)
            let row = MenuTaskRow(item: item, entry: entry, isStale: content.isStale)
            taskRows[entry.id] = row
            row.setDurationFont(entry.durationText)
            menu.minimumWidth = max(menu.minimumWidth, row.reservedWidth)
        }
        if content.todayTasks.isEmpty {
            let empty: String
            switch content.dailyTotalsStatus {
            case .current: empty = "No time logged today"
            case .cached: empty = "Last confirmed totals are empty"
            case .loading: empty = "Loading today's totals"
            case .unavailable: empty = "Today's totals are unavailable"
            }
            addText(empty, to: menu)
        }
        if !content.otherTasks.isEmpty {
            let submenu = NSMenu(title: "Start tracking")
            submenu.appearance = appearance
            submenu.autoenablesItems = false
            for entry in content.otherTasks {
                addTask(
                    entry, title: entry.task.name, help: nil, content: content,
                    appearance: appearance, to: submenu)
            }
            let parent = NSMenuItem(title: "Start tracking", action: nil, keyEquivalent: "")
            parent.submenu = submenu
            parent.isEnabled = true
            menu.addItem(parent)
        }
        menu.addItem(.separator())
        addAction("Open Time Tracker", action: .openWindow, to: menu)
        addAction("Quit Time Tracker", action: .quit, to: menu)
        return TrackerStatusMenu(menu: menu, elapsedField: elapsedField, totalField: totalField, taskRows: taskRows)
    }

    @discardableResult
    private func addTask(
        _ entry: TrackerMenuTask, title: String, help: String?,
        content: TrackerMenuContent, appearance: NSAppearance, to menu: NSMenu
    ) -> NSMenuItem {
        let isRunning = entry.isRunning && !content.isStale
        let trackingAction = entry.trackingAction(in: content).map { action -> TrackerMenuCommand.Action in
            switch action {
            case .start(let taskID): return .start(taskID)
            case .stop(let worklogID): return .stop(worklogID)
            }
        }
        let submenu = NSMenu(title: entry.task.name)
        submenu.appearance = appearance
        submenu.autoenablesItems = false
        addAction(
            entry.isRunning ? "Stop tracking" : "Start tracking",
            action: trackingAction, enabled: trackingAction != nil, to: submenu)
        submenu.addItem(.separator())
        for action in [MenuShortcutAction.copyName, .copyExact, .copyRounded] {
            let available =
                menuBar.menuCopyValue(
                    action, taskID: entry.id,
                    connection: connection) != nil
            let item = addAction(action.title, action: .copy(action, entry.id), enabled: available, to: submenu)
            item.setAccessibilityIdentifier("menu.copy.\(entry.id).\(action.rawValue)")
        }
        let item = addAction(title, action: trackingAction, to: menu)
        item.setAccessibilityIdentifier("menu.task.\(entry.id)")
        item.setAccessibilityValue("Task color: \(TaskColor.forTaskID(entry.id).rawValue)")
        item.submenu = submenu
        if trackingAction != nil {
            // Setting a submenu assigns submenuAction; restore the row's tracking action afterward.
            item.target = target
            item.action = itemAction
        }
        if isRunning {
            item.attributedTitle = NSAttributedString(
                string: title,
                attributes: [
                    .font: NSFont.boldSystemFont(ofSize: NSFont.menuFont(ofSize: 0).pointSize)
                ])
        }
        item.image = TaskDotImage.make(
            color: TaskColor.forTaskID(entry.id),
            isRunning: isRunning, appearance: appearance)
        item.preferredImageVisibility = .visible
        item.toolTip =
            trackingAction != nil ? help : "Open the submenu to copy this task's name or time. Tracking is unavailable."
        let status = MenuTaskRow.accessibilityStatus(isRunning: entry.isRunning, isStale: content.isStale)
        item.setAccessibilityLabel("\(title), \(status)")
        return item
    }

    @discardableResult
    private func addText(
        _ title: String, value: String? = nil, to menu: NSMenu,
        help: String? = nil, liveValue: Bool = false
    ) -> MenuValueField? {
        let text = value.map { "\(title), \($0)" } ?? title
        let item = NSMenuItem(title: text, action: nil, keyEquivalent: "")
        item.isEnabled = false
        let label = NSTextField(labelWithString: title)
        label.font = .menuFont(ofSize: 0)
        label.textColor = .labelColor
        label.lineBreakMode = .byTruncatingMiddle
        label.toolTip = help ?? title
        label.setAccessibilityLabel(title)
        let valueLabel = value.map { NSTextField(labelWithString: $0) }
        valueLabel?.font =
            liveValue
            ? NSFont.monospacedDigitSystemFont(
                ofSize: NSFont.menuFont(ofSize: 0).pointSize,
                weight: .regular) : label.font
        valueLabel?.textColor = .labelColor
        valueLabel?.alignment = .right
        valueLabel?.lineBreakMode = .byTruncatingMiddle
        valueLabel?.toolTip = help ?? value
        valueLabel?.setAccessibilityLabel(value ?? "")
        let valueSize = valueLabel?.fittingSize ?? .zero
        let valueWidth =
            liveValue && valueLabel != nil
            ? max(
                valueSize.width,
                NSAttributedString(
                    string: "~000000:00:00",
                    attributes: [
                        .font: valueLabel?.font ?? NSFont.menuFont(ofSize: 0)
                    ]
                ).size().width) : valueSize.width
        let width = min(480, label.fittingSize.width + valueWidth + (value == nil ? 40 : 56))
        let height = max(label.fittingSize.height, valueSize.height) + 8
        let view = NSView(frame: NSRect(x: 0, y: 0, width: width, height: height))
        view.appearance = menu.appearance
        view.autoresizingMask = [.width]
        label.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(label)
        NSLayoutConstraint.activate([
            label.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 20),
            label.centerYAnchor.constraint(equalTo: view.centerYAnchor),
        ])
        if let valueLabel {
            valueLabel.translatesAutoresizingMaskIntoConstraints = false
            view.addSubview(valueLabel)
            label.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
            NSLayoutConstraint.activate([
                valueLabel.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -20),
                valueLabel.centerYAnchor.constraint(equalTo: view.centerYAnchor),
                label.trailingAnchor.constraint(lessThanOrEqualTo: valueLabel.leadingAnchor, constant: -16),
            ])
            if liveValue {
                valueLabel.widthAnchor.constraint(equalToConstant: valueWidth).isActive = true
            } else {
                valueLabel.widthAnchor.constraint(lessThanOrEqualTo: view.widthAnchor, multiplier: 0.65).isActive = true
            }
        } else {
            label.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -20).isActive = true
        }
        item.view = view
        menu.addItem(item)
        return valueLabel.map { MenuValueField(item: item, label: $0, titleLabel: label, title: title) }
    }

    @discardableResult
    private func addAction(
        _ title: String, action: TrackerMenuCommand.Action?, enabled: Bool = true,
        to menu: NSMenu
    ) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: itemAction, keyEquivalent: "")
        item.target = target
        item.representedObject = action.map { TrackerMenuCommand(action: $0, connection: connection) }
        item.isEnabled = enabled
        menu.addItem(item)
        return item
    }
}

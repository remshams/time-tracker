import AppKit
import TrackerClient

struct MenuValueField {
    let item: NSMenuItem
    let label: NSTextField
    let titleLabel: NSTextField
    let title: String

    func update(_ value: String, help: String? = nil) {
        if label.stringValue != value {
            label.stringValue = value
            item.title = "\(title), \(value)"
            label.setAccessibilityLabel(value)
            item.setAccessibilityLabel("\(title), \(value)")
        }
        if let help, label.toolTip != help { label.toolTip = help }
        if let help, titleLabel.toolTip != help { titleLabel.toolTip = help }
    }
}

struct MenuTaskRow {
    let item: NSMenuItem
    let entry: TrackerMenuTask
    let isStale: Bool

    var reservedWidth: CGFloat {
        styledTitle(Self.title(entry, duration: "000000:00:00"), duration: "000000:00:00").size().width + 70
    }

    func update(duration: String, help: String) {
        let nextTitle = Self.title(entry, duration: duration)
        if item.title != nextTitle {
            item.title = nextTitle
            setDurationFont(duration)
            let status = Self.accessibilityStatus(isRunning: entry.isRunning, isStale: isStale)
            item.setAccessibilityLabel("\(nextTitle), \(status)")
        }
        if item.representedObject != nil, item.toolTip != help { item.toolTip = help }
    }

    func setDurationFont(_ duration: String) {
        item.attributedTitle = styledTitle(item.title, duration: duration)
    }

    static func title(_ entry: TrackerMenuTask, duration: String) -> String {
        let archived = entry.task.archived ? "  Archived" : ""
        return "\(entry.task.name)  \(duration)\(archived)"
    }

    static func accessibilityStatus(isRunning: Bool, isStale: Bool) -> String {
        switch (isStale, isRunning) {
        case (true, true): return "last confirmed tracking, current status unavailable"
        case (true, false): return "current tracking status unavailable"
        case (false, true): return "tracking"
        case (false, false): return "not tracking"
        }
    }

    private func styledTitle(_ title: String, duration: String) -> NSAttributedString {
        let running = entry.isRunning && !isStale
        let menuFont = NSFont.menuFont(ofSize: 0)
        let font = running ? NSFont.boldSystemFont(ofSize: menuFont.pointSize) : menuFont
        let text = NSMutableAttributedString(string: title, attributes: [.font: font])
        let range = (title as NSString).range(of: duration, options: .backwards)
        text.addAttribute(
            .font,
            value: NSFont.monospacedDigitSystemFont(
                ofSize: font.pointSize, weight: running ? .bold : .regular), range: range)
        return text
    }
}

import AppKit
import TrackerClient

struct MenuValueField {
    let item: NSMenuItem
    let label: NSTextField
    let titleLabel: NSTextField
    let title: String

    func update(_ value: MenuDurationText, help: String? = nil) {
        if label.stringValue != value.text { show(value) }
        if let help, label.toolTip != help { label.toolTip = help }
        if let help, titleLabel.toolTip != help { titleLabel.toolTip = help }
    }

    /// Shows the duration with its seconds dimmed. VoiceOver reads its spoken form.
    func show(_ value: MenuDurationText) {
        let paragraph = NSMutableParagraphStyle()
        paragraph.alignment = label.alignment
        paragraph.lineBreakMode = label.lineBreakMode
        let text = NSMutableAttributedString(
            string: value.text,
            attributes: [
                .font: label.font ?? NSFont.menuFont(ofSize: 0), .foregroundColor: NSColor.labelColor,
                .paragraphStyle: paragraph,
            ])
        text.dimSeconds(of: value, endingAt: text.length)
        label.attributedStringValue = text
        item.title = "\(title), \(value.text)"
        label.setAccessibilityLabel(value.spoken)
        item.setAccessibilityLabel("\(title), \(value.spoken)")
    }
}

struct MenuTaskRow {
    let item: NSMenuItem
    let entry: TrackerMenuTask
    let isStale: Bool

    var reservedWidth: CGFloat {
        let widest = MenuDurationText(placeholder: "000000:00:00")
        return styledTitle(Self.title(entry, duration: widest.text), duration: widest).size().width + 70
    }

    func update(duration: MenuDurationText, help: String) {
        let nextTitle = Self.title(entry, duration: duration.text)
        if item.title != nextTitle {
            item.title = nextTitle
            setDurationFont(duration)
            let status = Self.accessibilityStatus(isRunning: entry.isRunning, isStale: isStale)
            item.setAccessibilityLabel("\(Self.title(entry, duration: duration.spoken)), \(status)")
        }
        if item.representedObject != nil, item.toolTip != help { item.toolTip = help }
    }

    func setDurationFont(_ duration: MenuDurationText) {
        item.attributedTitle = styledTitle(item.title, duration: duration)
    }

    /// The row's title with `duration`, which is the shown or the spoken duration.
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

    private func styledTitle(_ title: String, duration: MenuDurationText) -> NSAttributedString {
        let running = entry.isRunning && !isStale
        let menuFont = NSFont.menuFont(ofSize: 0)
        let font = running ? NSFont.boldSystemFont(ofSize: menuFont.pointSize) : menuFont
        let text = NSMutableAttributedString(string: title, attributes: [.font: font])
        let range = (title as NSString).range(of: duration.text, options: .backwards)
        text.addAttribute(
            .font,
            value: NSFont.monospacedDigitSystemFont(
                ofSize: font.pointSize, weight: running ? .bold : .regular), range: range)
        text.dimSeconds(of: duration, endingAt: NSMaxRange(range))
        return text
    }
}

extension NSMutableAttributedString {
    /// Dims the seconds of `duration`, whose text ends at `end`, as the main window does.
    func dimSeconds(of duration: MenuDurationText, endingAt end: Int) {
        let length = (duration.seconds as NSString).length
        guard length > 0 else { return }
        addAttribute(
            .foregroundColor, value: NSColor.secondaryLabelColor, range: NSRange(location: end - length, length: length)
        )
    }
}

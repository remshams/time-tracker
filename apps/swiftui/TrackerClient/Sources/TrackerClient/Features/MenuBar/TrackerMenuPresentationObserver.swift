import Foundation

@MainActor
public final class TrackerMenuPresentationObserver {
    public var onContentChange: (() -> Void)?
    public var onLabelChange: (() -> Void)?
    public private(set) var content: TrackerMenuContent
    public private(set) var label: TrackerMenuLabelContent

    public init(session: TrackerSession, showDailyTotal: Bool) {
        content = TrackerMenuContent(session)
        label = TrackerMenuLabelContent(session, showDailyTotal: showDailyTotal)
    }

    public func update(from session: TrackerSession, showDailyTotal: Bool) {
        let nextContent = TrackerMenuContent(session)
        let nextLabel = TrackerMenuLabelContent(session, showDailyTotal: showDailyTotal)
        if content != nextContent {
            content = nextContent
            onContentChange?()
        }
        if label != nextLabel {
            label = nextLabel
            onLabelChange?()
        }
    }
}

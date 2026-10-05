import Foundation

@MainActor
public final class TrackerMenuPresentationObserver {
    public var onContentChange: (() -> Void)?
    public var onLabelChange: (() -> Void)?
    public private(set) var content: TrackerMenuContent
    public private(set) var label: TrackerMenuLabelContent
    private var trackingDepth = 0

    public init(session: TrackerSession, showDailyTotal: Bool) {
        content = TrackerMenuContent(session)
        label = TrackerMenuLabelContent(session, showDailyTotal: showDailyTotal)
    }

    public func update(from session: TrackerSession, showDailyTotal: Bool) {
        // Keep the presentation stable while the user navigates an open popup.
        guard trackingDepth == 0 else { return }
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

    public func menuOpened(from session: TrackerSession, showDailyTotal: Bool) {
        update(from: session, showDailyTotal: showDailyTotal)
        trackingDepth += 1
    }

    public func menuClosed(from session: TrackerSession, showDailyTotal: Bool) {
        trackingDepth = max(0, trackingDepth - 1)
        update(from: session, showDailyTotal: showDailyTotal)
    }
}

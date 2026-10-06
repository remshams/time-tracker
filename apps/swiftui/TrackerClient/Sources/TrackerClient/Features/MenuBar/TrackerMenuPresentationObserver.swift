import Foundation

@MainActor
public final class TrackerMenuPresentationObserver {
    public var onContentChange: (() -> Void)?
    public var onLabelChange: (() -> Void)?
    public var onValuesChange: (() -> Void)?
    public private(set) var content: TrackerMenuContent
    public private(set) var label: TrackerMenuLabelContent
    public private(set) var values: TrackerMenuValues
    private var trackingDepth = 0

    public init(session: TrackerSession, showDailyTotal: Bool) {
        content = TrackerMenuContent(session)
        label = TrackerMenuLabelContent(session, showDailyTotal: showDailyTotal)
        values = TrackerMenuValues(session, content: content)
    }

    public func update(from session: TrackerSession, showDailyTotal: Bool) {
        // Keep navigation stable while time values continue to update.
        let nextContent = trackingDepth == 0 ? TrackerMenuContent(session) : content
        let nextLabel = trackingDepth == 0
            ? TrackerMenuLabelContent(session, showDailyTotal: showDailyTotal) : label
        let nextValues = TrackerMenuValues(session, content: nextContent)
        let contentChanged = content != nextContent
        let labelChanged = label != nextLabel
        let valuesChanged = values != nextValues
        content = nextContent
        label = nextLabel
        values = nextValues
        if contentChanged { onContentChange?() }
        if labelChanged { onLabelChange?() }
        if valuesChanged { onValuesChange?() }
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

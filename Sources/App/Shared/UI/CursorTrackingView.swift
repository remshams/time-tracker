import AppKit
import SwiftUI

/// A transparent view that changes the cursor when hovered.
/// Use this as an overlay on the trailing edge of a sidebar to hint that the panel is resizable.
struct CursorTrackingView: NSViewRepresentable {
    let cursor: NSCursor

    func makeNSView(context: Context) -> TrackingNSView {
        TrackingNSView(cursor: cursor)
    }

    func updateNSView(_ nsView: TrackingNSView, context: Context) {}
}

final class TrackingNSView: NSView {
    private let cursor: NSCursor

    init(cursor: NSCursor) {
        self.cursor = cursor
        super.init(frame: .zero)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        for area in trackingAreas {
            removeTrackingArea(area)
        }
        let options: NSTrackingArea.Options = [.mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect]
        addTrackingArea(NSTrackingArea(rect: bounds, options: options, owner: self))
    }

    override func mouseEntered(with event: NSEvent) {
        cursor.push()
    }

    override func mouseExited(with event: NSEvent) {
        NSCursor.pop()
    }
}

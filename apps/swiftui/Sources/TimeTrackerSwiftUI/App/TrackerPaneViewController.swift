import AppKit
import SwiftUI

@MainActor
final class TrackerPaneViewController: NSViewController {
    private let hosting: NSHostingController<AnyView>

    init(content: AnyView) {
        hosting = NSHostingController(rootView: content)
        super.init(nibName: nil, bundle: nil)
        addChild(hosting)
    }

    required init?(coder: NSCoder) {
        fatalError("TrackerPaneViewController requires SwiftUI content")
    }

    override func loadView() {
        view = NSView()
        let content = hosting.view
        content.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(content)
        // The split view extends behind the toolbar; pane content stays in the unobscured area.
        let guide = view.safeAreaLayoutGuide
        NSLayoutConstraint.activate([
            content.leadingAnchor.constraint(equalTo: guide.leadingAnchor),
            content.trailingAnchor.constraint(equalTo: guide.trailingAnchor),
            content.topAnchor.constraint(equalTo: guide.topAnchor),
            content.bottomAnchor.constraint(equalTo: guide.bottomAnchor)
        ])
    }
}

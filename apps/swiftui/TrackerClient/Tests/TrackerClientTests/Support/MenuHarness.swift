import XCTest
@testable import TrackerClient

/// Builds the menu bar observer over a fixture's stores, as the app does.
///
/// After each session change it updates the presentation, which supplies the values that combine features. The
/// observer then rebuilds the menu from its `Observations` loop; `settled` waits for that rebuild.
@MainActor
final class MenuHarness {
    let presentation: TrackerPresentation
    let sources: TrackerMenuSources
    let observer: TrackerMenuPresentationObserver
    private let fixture: Fixture

    init(_ fixture: Fixture, showDailyTotal: Bool, taskOrder: MenuTaskOrder = .newestFirst) {
        self.fixture = fixture
        let presentation = TrackerPresentation(session: fixture.session)
        self.presentation = presentation
        sources = TrackerMenuSources(session: fixture.session, presentation: presentation)
        observer = TrackerMenuPresentationObserver(
            sources: sources, showDailyTotal: showDailyTotal, taskOrder: taskOrder)
        attach()
    }

    /// Restores the session change handler after a fixture helper replaced it.
    func attach() {
        fixture.sync.onChange = { [weak self] in self?.update() }
    }

    func update() { presentation.update(from: fixture.session) }

    /// Opens the menu as the app does: the sync publishes first, then the menu captures its content.
    func menuOpened() {
        fixture.sync.setMenuVisible(true)
        observer.menuOpened()
    }

    func menuClosed() {
        fixture.sync.setMenuVisible(false)
        observer.menuClosed()
    }

    /// Waits until the observer has rebuilt the menu from the current store values.
    func settled(file: StaticString = #filePath, line: UInt = #line) async throws {
        let observer = observer
        let sources = sources
        let finished = Task { @MainActor in
            while observer.lastInputs != sources.read() && !Task.isCancelled { await Task.yield() }
        }
        try await fixture.taskValue(finished, file: file, line: line)
    }

    /// Advances the clock and fires the menu bar's minute clock, which republishes the stores.
    func tick(_ seconds: TimeInterval, file: StaticString = #filePath, line: UInt = #line) async throws {
        fixture.clock.now.addTimeInterval(seconds)
        fixture.clock.uptime += seconds
        try XCTUnwrap(fixture.scheduler.menuBarClock, file: file, line: line).fire()
        try await settled(file: file, line: line)
    }
}

extension TrackerMenuInputs {
    /// Reads the menu's inputs from the session's stores and a presentation updated from the session.
    @MainActor static func current(_ session: TrackerSession) -> TrackerMenuInputs {
        TrackerMenuSources(session: session, presentation: TrackerPresentation(session: session)).read()
    }
}

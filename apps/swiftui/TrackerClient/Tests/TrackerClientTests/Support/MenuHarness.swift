import XCTest
@testable import TrackerClient

/// Builds the menu bar observer over a fixture's stores, as the app does.
///
/// The observer rebuilds the menu from its `Observations` loop; `settled` waits for that rebuild.
@MainActor
final class MenuHarness {
    let sources: TrackerMenuSources
    let observer: TrackerMenuPresentationObserver
    private let fixture: Fixture

    init(_ fixture: Fixture, showDailyTotal: Bool, taskOrder: MenuTaskOrder = .newestFirst) {
        self.fixture = fixture
        sources = fixture.features.menuSources
        observer = TrackerMenuPresentationObserver(
            sources: sources, showDailyTotal: showDailyTotal, taskOrder: taskOrder)
    }

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
    /// Reads the menu's inputs from the features' stores.
    @MainActor static func current(_ features: TrackerFeatures) -> TrackerMenuInputs {
        features.menuSources.read()
    }
}

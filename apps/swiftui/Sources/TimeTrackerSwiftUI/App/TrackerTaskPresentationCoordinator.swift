import Observation
import TrackerClient

/// Brings up the tracker window when a dialog opens, so its sheet has a window. A tracking error alone doesn't open
/// the window; its alert appears when the window opens.
final class TrackerTaskPresentationCoordinator {
    private let gate: PresentationGate
    private let revealWindow: () -> Void

    init(gate: PresentationGate, revealWindow: @escaping () -> Void) {
        self.gate = gate
        self.revealWindow = revealWindow
    }

    func start() {
        // The closure reads the gate without the coordinator. The loop ends at the first change after the
        // coordinator is released.
        Task { [weak self, gate] in
            var showedSheet = false
            for await showsSheet in Observations({ gate.showsSheet }) {
                guard let self else { return }
                if showsSheet && !showedSheet { revealWindow() }
                showedSheet = showsSheet
            }
        }
    }
}

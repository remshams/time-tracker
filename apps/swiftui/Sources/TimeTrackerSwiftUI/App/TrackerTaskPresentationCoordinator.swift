import Foundation
import Observation
import SwiftUI

@MainActor
@Observable
final class TrackerTaskPresentationCoordinator {
    enum Feature: CaseIterable {
        case creation, rename, correction, move, deletion, archiving, bulkArchiving, trackingError
    }

    private var owners: [Feature: UUID] = [:]
    private let store: TrackerStore
    private let presentingWindow: (Bool, Set<UUID>) -> UUID?
    @ObservationIgnored private var observationGeneration = 0
    @ObservationIgnored private var isRunning = false
    @ObservationIgnored private var isChoosingOwner = false

    init(store: TrackerStore, presentingWindow: @escaping (Bool, Set<UUID>) -> UUID?) {
        self.store = store
        self.presentingWindow = presentingWindow
    }

    func start() {
        guard !isRunning else { return }
        isRunning = true
        observationGeneration += 1
        observePresentations(generation: observationGeneration)
    }

    func stop() {
        isRunning = false
        observationGeneration += 1
        owners.removeAll()
    }

    func windowAvailable() { update() }

    func owns(_ feature: Feature, windowID: UUID) -> Bool {
        owners[feature] == windowID
    }

    func canClose(_ windowID: UUID) -> Bool {
        !owners.values.contains(windowID)
    }

    func windowClosed(_ windowID: UUID) {
        let remaining = owners.filter { $0.value != windowID }
        if owners != remaining { owners = remaining }
        update()
    }

    private func isVisible(_ feature: Feature) -> Bool {
        switch feature {
        case .creation: store.creation.state.isPresented
        case .rename: store.rename.state.isPresented
        case .correction: store.correction.state.isPresented
        case .move: store.move.state.isPresented
        case .deletion: store.deletion.state.isPresented
        case .archiving: store.archiving.state.isPresented
        case .bulkArchiving: store.bulkArchiving.state.isPresented
        case .trackingError: store.trackingError != nil
        }
    }

    /// Chooses an owner whenever a value read by `update()` changes.
    private func observePresentations(generation: Int) {
        guard isRunning, generation == observationGeneration else { return }
        withObservationTracking {
            update()
        } onChange: { [weak self] in
            // Observation reports changes before the new value is stored; read it on the next main-actor turn.
            Task { @MainActor in self?.observePresentations(generation: generation) }
        }
    }

    private func update() {
        guard isRunning, !isChoosingOwner else { return }
        isChoosingOwner = true
        defer { isChoosingOwner = false }
        let visible = Feature.allCases.filter(isVisible)
        let remaining = owners.filter { visible.contains($0.key) }
        if owners != remaining { owners = remaining }
        for feature in visible where owners[feature] == nil {
            if let owner = presentingWindow(feature != .trackingError, Set(owners.values)) {
                owners[feature] = owner
            }
        }
    }
}

@MainActor
struct TrackerDetailPresentation: View {
    let store: TrackerStore
    let presentation: TrackerTaskPresentationCoordinator
    let windowID: UUID

    private var trackingFailurePresented: Binding<Bool> {
        Binding(
            get: { owns(.trackingError) && store.trackingError != nil },
            set: {
                if owns(.trackingError) && !$0 { store.dismissTrackingError() }
            })
    }

    var body: some View {
        TaskDetails(store: store)
            .background {
                TaskCreationDialog(creation: store.creation, isPresentationOwner: owns(.creation))
                TaskRenameDialog(rename: store.rename, isPresentationOwner: owns(.rename))
                WorklogCorrectionDialog(correction: store.correction, isPresentationOwner: owns(.correction))
                WorklogMoveDialog(move: store.move, isPresentationOwner: owns(.move))
                WorklogDeletionDialog(deletion: store.deletion, isPresentationOwner: owns(.deletion))
                BulkTaskArchivingDialog(archiving: store.bulkArchiving, isPresentationOwner: owns(.bulkArchiving))
                TaskArchivingDialog(archiving: store.archiving, isPresentationOwner: owns(.archiving))
            }
            .alert("Could not change tracking", isPresented: trackingFailurePresented) {
                Button("OK", role: .cancel) { store.dismissTrackingError() }
            } message: {
                Text(store.trackingError ?? "Please try again.")
            }
    }

    private func owns(_ feature: TrackerTaskPresentationCoordinator.Feature) -> Bool {
        presentation.owns(feature, windowID: windowID)
    }
}

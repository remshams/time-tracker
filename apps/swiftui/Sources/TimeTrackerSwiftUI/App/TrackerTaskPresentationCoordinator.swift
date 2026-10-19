import Foundation
import Observation
import SwiftUI

@MainActor
@Observable
final class TrackerTaskPresentationCoordinator {
    private(set) var ownerID: UUID?
    // Windows read this flag instead of the feature states, so draft edits do not redraw them.
    private(set) var hasPresentation = false
    private let store: TrackerStore
    private let presentingWindow: (Bool) -> UUID?
    @ObservationIgnored private var observationGeneration = 0
    @ObservationIgnored private var isRunning = false
    @ObservationIgnored private var isChoosingOwner = false

    init(store: TrackerStore, presentingWindow: @escaping (Bool) -> UUID?) {
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
        hasPresentation = false
        setOwner(nil)
    }

    func windowAvailable() { update() }

    func owns(_ windowID: UUID) -> Bool {
        ownerID == windowID
    }

    func canClose(_ windowID: UUID) -> Bool {
        !owns(windowID) || !hasPresentation
    }

    func windowClosed(_ windowID: UUID) {
        guard ownerID == windowID else { return }
        setOwner(nil)
        update()
    }

    private var editorIsPresented: Bool {
        store.creation.state.isPresented || store.rename.state.isPresented || store.correction.state.isPresented
            || store.move.state.isPresented || store.deletion.state.isPresented || store.archiving.state.isPresented
            || store.bulkArchiving.state.isPresented
    }

    private var presentationIsVisible: Bool {
        editorIsPresented || store.trackingError != nil
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
        let isVisible = presentationIsVisible
        if hasPresentation != isVisible { hasPresentation = isVisible }
        guard isVisible else {
            setOwner(nil)
            return
        }
        guard ownerID == nil else { return }
        isChoosingOwner = true
        defer { isChoosingOwner = false }
        setOwner(presentingWindow(editorIsPresented))
    }

    private func setOwner(_ windowID: UUID?) {
        guard ownerID != windowID else { return }
        ownerID = windowID
    }
}

@MainActor
struct TrackerDetailPresentation: View {
    let store: TrackerStore
    let presentation: TrackerTaskPresentationCoordinator
    let windowID: UUID

    private var isOwner: Bool { presentation.owns(windowID) }
    private var trackingFailurePresented: Binding<Bool> {
        Binding(
            get: { isOwner && store.trackingError != nil },
            set: {
                if isOwner && !$0 { store.dismissTrackingError() }
            })
    }

    var body: some View {
        TaskDetails(store: store)
            .background {
                TaskCreationDialog(creation: store.creation, isPresentationOwner: isOwner)
                TaskRenameDialog(rename: store.rename, isPresentationOwner: isOwner)
                WorklogCorrectionDialog(correction: store.correction, isPresentationOwner: isOwner)
                WorklogMoveDialog(move: store.move, isPresentationOwner: isOwner)
                WorklogDeletionDialog(deletion: store.deletion, isPresentationOwner: isOwner)
                BulkTaskArchivingDialog(archiving: store.bulkArchiving, isPresentationOwner: isOwner)
                TaskArchivingDialog(archiving: store.archiving, isPresentationOwner: isOwner)
            }
            .alert("Could not change tracking", isPresented: trackingFailurePresented) {
                Button("OK", role: .cancel) { store.dismissTrackingError() }
            } message: {
                Text(store.trackingError ?? "Please try again.")
            }
    }
}

import Foundation
import Observation
import SwiftUI

@Observable
final class TrackerTaskPresentationCoordinator {
    private(set) var ownerID: UUID?
    // Windows read this flag instead of the feature states, so draft edits do not redraw them.
    // macOS shows one sheet per window, so another editor waits until the owner window's sheet closes.
    // Menus and buttons that open an editor stay disabled while this is true.
    private(set) var hasPresentation = false
    private let store: TrackerStore
    private let presentingWindow: (Bool) -> UUID?
    @ObservationIgnored private var isRunning = false
    @ObservationIgnored private var isChoosingOwner = false

    init(store: TrackerStore, presentingWindow: @escaping (Bool) -> UUID?) {
        self.store = store
        self.presentingWindow = presentingWindow
    }

    func start() {
        guard !isRunning else { return }
        isRunning = true
        observePresentations()
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
    private func observePresentations() {
        withObservationTracking {
            update()
        } onChange: { [weak self] in
            // Observation reports changes before the new value is stored; read it on the next main-actor turn.
            Task { @MainActor in self?.observePresentations() }
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
                OwnedSheet(
                    isPresentationOwner: isOwner, isPresented: { store.creation.state.isPresented },
                    cancel: store.creation.cancel
                ) {
                    TaskCreationSheet(creation: store.creation)
                }
                OwnedSheet(
                    isPresentationOwner: isOwner, isPresented: { store.rename.state.isPresented },
                    cancel: store.rename.cancel
                ) {
                    TaskRenameSheet(rename: store.rename)
                }
                OwnedSheet(
                    isPresentationOwner: isOwner, isPresented: { store.correction.state.isPresented },
                    cancel: store.correction.cancel
                ) {
                    WorklogCorrectionSheet(correction: store.correction)
                }
                OwnedSheet(
                    isPresentationOwner: isOwner, isPresented: { store.move.state.isPresented },
                    cancel: store.move.cancel
                ) {
                    WorklogMoveSheet(move: store.move)
                }
                OwnedSheet(
                    isPresentationOwner: isOwner, isPresented: { store.deletion.state.isPresented },
                    cancel: store.deletion.cancel
                ) {
                    WorklogDeletionSheet(deletion: store.deletion)
                }
                OwnedSheet(
                    isPresentationOwner: isOwner, isPresented: { store.bulkArchiving.state.isPresented },
                    cancel: store.bulkArchiving.cancel
                ) {
                    BulkTaskArchivingSheet(archiving: store.bulkArchiving)
                }
                OwnedSheet(
                    isPresentationOwner: isOwner, isPresented: { store.archiving.state.isPresented },
                    cancel: store.archiving.cancel
                ) {
                    TaskArchivingSheet(archiving: store.archiving)
                }
            }
            .alert("Could not change tracking", isPresented: trackingFailurePresented) {
                Button("OK", role: .cancel) { store.dismissTrackingError() }
            } message: {
                Text(store.trackingError ?? "Please try again.")
            }
    }
}

/// Presents an editor sheet only in the window that owns task presentations.
/// `isPresented` is a closure, so the sheet binding reads the editor state and the window body doesn't.
private struct OwnedSheet<Content: View>: View {
    let isPresentationOwner: Bool
    let isPresented: () -> Bool
    let cancel: () -> Void
    let content: () -> Content

    var body: some View {
        Color.clear
            .frame(width: 0, height: 0)
            .sheet(
                isPresented: Binding(
                    get: { isPresentationOwner && isPresented() },
                    set: { if isPresentationOwner && !$0 { cancel() } }
                )
            ) {
                content()
            }
    }
}

import Foundation
import Observation
import SwiftUI

@Observable
final class TrackerTaskPresentationCoordinator {
    // Views read this flag instead of the feature states, so draft edits do not redraw them.
    // macOS shows one sheet per window, so another editor waits until the open sheet or alert closes.
    // Menus and buttons that open an editor stay disabled while this is true.
    private(set) var hasPresentation = false
    private let store: TrackerStore
    private let revealWindow: () -> Void
    @ObservationIgnored private var isRunning = false
    @ObservationIgnored private var editorWasPresented = false

    init(store: TrackerStore, revealWindow: @escaping () -> Void) {
        self.store = store
        self.revealWindow = revealWindow
    }

    func start() {
        guard !isRunning else { return }
        isRunning = true
        observePresentations()
    }

    /// The window stays open while it shows a sheet or alert, so the presentation isn't lost.
    var canCloseWindow: Bool { !hasPresentation }

    private var editorIsPresented: Bool {
        store.creation.state.isPresented || store.rename.state.isPresented || store.correction.state.isPresented
            || store.move.state.isPresented || store.deletion.state.isPresented || store.archiving.state.isPresented
            || store.bulkArchiving.state.isPresented
    }

    /// Updates the flag whenever a value read by `update()` changes.
    private func observePresentations() {
        withObservationTracking {
            update()
        } onChange: { [weak self] in
            // Observation reports changes before the new value is stored; read it on the next main-actor turn.
            Task { @MainActor in self?.observePresentations() }
        }
    }

    private func update() {
        guard isRunning else { return }
        let editorIsPresented = editorIsPresented
        let isVisible = editorIsPresented || store.trackingError != nil
        if hasPresentation != isVisible { hasPresentation = isVisible }
        let editorOpened = editorIsPresented && !editorWasPresented
        editorWasPresented = editorIsPresented
        // An editor's sheet needs the tracker window, so bring it up when an editor opens.
        if editorOpened { revealWindow() }
    }
}

struct TrackerDetailPresentation: View {
    let store: TrackerStore

    private var trackingFailurePresented: Binding<Bool> {
        Binding(
            get: { store.trackingError != nil },
            set: {
                if !$0 { store.dismissTrackingError() }
            })
    }

    var body: some View {
        TaskDetails(store: store)
            .background {
                EditorSheet(
                    isPresented: { store.creation.state.isPresented },
                    cancel: store.creation.cancel
                ) {
                    TaskCreationSheet(creation: store.creation)
                }
                EditorSheet(
                    isPresented: { store.rename.state.isPresented },
                    cancel: store.rename.cancel
                ) {
                    TaskRenameSheet(rename: store.rename)
                }
                EditorSheet(
                    isPresented: { store.correction.state.isPresented },
                    cancel: store.correction.cancel
                ) {
                    WorklogCorrectionSheet(correction: store.correction)
                }
                EditorSheet(
                    isPresented: { store.move.state.isPresented },
                    cancel: store.move.cancel
                ) {
                    WorklogMoveSheet(move: store.move)
                }
                EditorSheet(
                    isPresented: { store.deletion.state.isPresented },
                    cancel: store.deletion.cancel
                ) {
                    WorklogDeletionSheet(deletion: store.deletion)
                }
                EditorSheet(
                    isPresented: { store.bulkArchiving.state.isPresented },
                    cancel: store.bulkArchiving.cancel
                ) {
                    BulkTaskArchivingSheet(archiving: store.bulkArchiving)
                }
                EditorSheet(
                    isPresented: { store.archiving.state.isPresented },
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

/// Presents one editor's sheet.
/// `isPresented` is a closure, so the sheet binding reads the editor state and the window body doesn't.
private struct EditorSheet<Content: View>: View {
    let isPresented: () -> Bool
    let cancel: () -> Void
    let content: () -> Content

    var body: some View {
        Color.clear
            .frame(width: 0, height: 0)
            .sheet(
                isPresented: Binding(
                    get: { isPresented() },
                    set: { if !$0 { cancel() } }
                )
            ) {
                content()
            }
    }
}

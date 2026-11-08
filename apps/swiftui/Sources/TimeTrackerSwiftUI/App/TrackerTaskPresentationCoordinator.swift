import Foundation
import Observation
import SwiftUI
import TrackerClient

@Observable
final class TrackerTaskPresentationCoordinator {
    // Views read this flag instead of the feature states, so draft edits do not redraw them.
    // macOS shows one sheet per window, so another editor waits until the open sheet or alert closes.
    // Menus and buttons that open an editor stay disabled while this is true.
    private(set) var hasPresentation = false
    private let store: TrackerStore
    private let revealWindow: () -> Void
    @ObservationIgnored private var editorWasPresented = false

    init(store: TrackerStore, revealWindow: @escaping () -> Void) {
        self.store = store
        self.revealWindow = revealWindow
    }

    /// The first value arrives after this returns, so `hasPresentation` stays false until then. Views observe the
    /// flag, and a presentation that opens in between is part of that first value.
    func start() {
        // The closure reads the store without the coordinator. The loop ends at the first change after the
        // coordinator is released.
        Task { [weak self, store] in
            for await flags in Observations({ store.presentationFlags }) {
                guard let self else { return }
                update(flags)
            }
        }
    }

    /// The window stays open while it shows a sheet or alert, so the presentation isn't lost.
    var canCloseWindow: Bool { !hasPresentation }

    private func update(_ flags: (editorIsPresented: Bool, hasTrackingError: Bool)) {
        let isVisible = flags.editorIsPresented || flags.hasTrackingError
        if hasPresentation != isVisible { hasPresentation = isVisible }
        let editorOpened = flags.editorIsPresented && !editorWasPresented
        editorWasPresented = flags.editorIsPresented
        // An editor's sheet needs the tracker window, so bring it up when an editor opens.
        if editorOpened { revealWindow() }
    }
}

extension TrackerStore {
    /// The values the presentation coordinator reacts to. A closure given to `Observations` re-reads them after
    /// each change, so a flag that `||` skips is read as soon as it can change the result.
    fileprivate var presentationFlags: (editorIsPresented: Bool, hasTrackingError: Bool) {
        let editorIsPresented =
            taskCreation.isPresented || taskRename.isPresented || correction.state.isPresented
            || worklogMove.isPresented || worklogDeletion.isPresented || taskArchiving.isPresented
            || bulkArchiving.state.isPresented
        return (editorIsPresented, tracking.error != nil)
    }
}

struct TrackerDetailPresentation: View {
    let store: TrackerStore
    @Environment(TrackingStore.self) private var tracking

    private var trackingFailurePresented: Binding<Bool> {
        Binding(
            get: { tracking.error != nil },
            set: {
                if !$0 { store.dismissTrackingError() }
            })
    }

    var body: some View {
        TaskDetails(store: store)
            .background {
                EditorSheet(
                    isPresented: { store.taskCreation.isPresented },
                    cancel: store.taskCreation.cancel
                ) {
                    TaskCreationSheet()
                }
                EditorSheet(
                    isPresented: { store.taskRename.isPresented },
                    cancel: store.taskRename.cancel
                ) {
                    TaskRenameSheet()
                }
                EditorSheet(
                    isPresented: { store.correction.state.isPresented },
                    cancel: store.correction.cancel
                ) {
                    WorklogCorrectionSheet(correction: store.correction)
                }
                EditorSheet(
                    isPresented: { store.worklogMove.isPresented },
                    cancel: store.worklogMove.cancel
                ) {
                    WorklogMoveSheet()
                }
                EditorSheet(
                    isPresented: { store.worklogDeletion.isPresented },
                    cancel: store.worklogDeletion.cancel
                ) {
                    WorklogDeletionSheet()
                }
                EditorSheet(
                    isPresented: { store.bulkArchiving.state.isPresented },
                    cancel: store.bulkArchiving.cancel
                ) {
                    BulkTaskArchivingSheet(archiving: store.bulkArchiving)
                }
                EditorSheet(
                    isPresented: { store.taskArchiving.isPresented },
                    cancel: store.taskArchiving.cancel
                ) {
                    TaskArchivingSheet()
                }
            }
            .alert("Could not change tracking", isPresented: trackingFailurePresented) {
                Button("OK", role: .cancel) { store.dismissTrackingError() }
            } message: {
                Text(tracking.error ?? "Please try again.")
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

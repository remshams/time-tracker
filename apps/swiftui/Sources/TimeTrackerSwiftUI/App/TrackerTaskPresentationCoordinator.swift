import Foundation
import Observation
import SwiftUI
import TrackerClient

@Observable
final class TrackerTaskPresentationCoordinator {
    // Views read this flag instead of the feature states, so draft edits do not redraw them.
    // macOS shows one sheet per window, so another editor waits until the open sheet or alert closes.
    // The toolbar buttons and the New Task command stay disabled while this is true. An attached sheet blocks the
    // menus inside the window, as OpenDialogUITests shows.
    private(set) var hasPresentation = false
    private let sources: PresentationSources
    private let revealWindow: () -> Void
    @ObservationIgnored private var editorWasPresented = false

    init(
        taskCreation: TaskCreationStore, taskRename: TaskRenameStore, worklogCorrection: WorklogCorrectionStore,
        worklogMove: WorklogMoveStore, worklogDeletion: WorklogDeletionStore, taskArchiving: TaskArchivingStore,
        bulkTaskArchiving: BulkTaskArchivingStore, tracking: TrackingStore, revealWindow: @escaping () -> Void
    ) {
        sources = PresentationSources(
            taskCreation: taskCreation, taskRename: taskRename, worklogCorrection: worklogCorrection,
            worklogMove: worklogMove, worklogDeletion: worklogDeletion, taskArchiving: taskArchiving,
            bulkTaskArchiving: bulkTaskArchiving, tracking: tracking)
        self.revealWindow = revealWindow
    }

    /// The first value arrives after this returns, so `hasPresentation` stays false until then. Views observe the
    /// flag, and a presentation that opens in between is part of that first value.
    func start() {
        // The closure reads the stores without the coordinator. The loop ends at the first change after the
        // coordinator is released.
        Task { [weak self, sources] in
            for await flags in Observations({ sources.flags }) {
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

/// The stores whose dialogs and tracking error the coordinator watches.
private struct PresentationSources {
    let taskCreation: TaskCreationStore
    let taskRename: TaskRenameStore
    let worklogCorrection: WorklogCorrectionStore
    let worklogMove: WorklogMoveStore
    let worklogDeletion: WorklogDeletionStore
    let taskArchiving: TaskArchivingStore
    let bulkTaskArchiving: BulkTaskArchivingStore
    let tracking: TrackingStore

    /// The values the presentation coordinator reacts to. A closure given to `Observations` re-reads them after
    /// each change, so a flag that `||` skips is read as soon as it can change the result.
    var flags: (editorIsPresented: Bool, hasTrackingError: Bool) {
        let editorIsPresented =
            taskCreation.isPresented || taskRename.isPresented || worklogCorrection.isPresented
            || worklogMove.isPresented || worklogDeletion.isPresented || taskArchiving.isPresented
            || bulkTaskArchiving.isPresented
        return (editorIsPresented, tracking.error != nil)
    }
}

struct TrackerDetailPresentation: View {
    @Environment(TrackingStore.self) private var tracking
    @Environment(TrackingActions.self) private var trackingActions
    @Environment(TaskCreationStore.self) private var taskCreation
    @Environment(TaskRenameStore.self) private var taskRename
    @Environment(WorklogCorrectionStore.self) private var worklogCorrection
    @Environment(WorklogMoveStore.self) private var worklogMove
    @Environment(WorklogDeletionStore.self) private var worklogDeletion
    @Environment(BulkTaskArchivingStore.self) private var bulkTaskArchiving
    @Environment(TaskArchivingStore.self) private var taskArchiving

    private var trackingFailurePresented: Binding<Bool> {
        Binding(
            get: { tracking.error != nil },
            set: {
                if !$0 { trackingActions.dismissTrackingError() }
            })
    }

    var body: some View {
        TaskDetails()
            .background {
                EditorSheet(
                    isPresented: { taskCreation.isPresented },
                    cancel: taskCreation.cancel
                ) {
                    TaskCreationSheet()
                }
                EditorSheet(
                    isPresented: { taskRename.isPresented },
                    cancel: taskRename.cancel
                ) {
                    TaskRenameSheet()
                }
                EditorSheet(
                    isPresented: { worklogCorrection.isPresented },
                    cancel: worklogCorrection.cancel
                ) {
                    WorklogCorrectionSheet()
                }
                EditorSheet(
                    isPresented: { worklogMove.isPresented },
                    cancel: worklogMove.cancel
                ) {
                    WorklogMoveSheet()
                }
                EditorSheet(
                    isPresented: { worklogDeletion.isPresented },
                    cancel: worklogDeletion.cancel
                ) {
                    WorklogDeletionSheet()
                }
                EditorSheet(
                    isPresented: { bulkTaskArchiving.isPresented },
                    cancel: bulkTaskArchiving.cancel
                ) {
                    BulkTaskArchivingSheet()
                }
                EditorSheet(
                    isPresented: { taskArchiving.isPresented },
                    cancel: taskArchiving.cancel
                ) {
                    TaskArchivingSheet()
                }
            }
            .alert("Could not change tracking", isPresented: trackingFailurePresented) {
                Button("OK", role: .cancel) { trackingActions.dismissTrackingError() }
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

import Observation
import SwiftUI
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

struct TrackerDetailPresentation: View {
    @Environment(TrackingStore.self) private var tracking
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
                if !$0 { tracking.dismissError() }
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
                Button("OK", role: .cancel) { tracking.dismissError() }
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

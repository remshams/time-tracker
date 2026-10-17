import Foundation
import Combine
import SwiftUI

@MainActor
final class TrackerTaskPresentationCoordinator {
    let objectWillChange = ObservableObjectPublisher()
    private(set) var ownerID: UUID?
    private let store: TrackerStore
    private let presentingWindow: (Bool) -> UUID?
    private var subscriptions = Set<AnyCancellable>()
    private var isRunning = false
    private var isChoosingOwner = false

    init(store: TrackerStore, presentingWindow: @escaping (Bool) -> UUID?) {
        self.store = store
        self.presentingWindow = presentingWindow
    }

    func start() {
        guard !isRunning else { return }
        isRunning = true
        for publisher in [
            store.objectWillChange, store.creation.objectWillChange,
            store.rename.objectWillChange, store.correction.objectWillChange,
            store.move.objectWillChange, store.deletion.objectWillChange,
            store.archiving.objectWillChange, store.bulkArchiving.objectWillChange,
        ] {
            publisher.sink { [weak self] _ in self?.update() }.store(in: &subscriptions)
        }
        update()
    }

    func stop() {
        isRunning = false
        subscriptions.removeAll()
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

    // Publishers emit before model snapshots change; routing reads the authoritative session.
    private var hasPresentation: Bool {
        store.session.taskCreation.isPresented || store.session.taskRename.isPresented
            || store.session.worklogCorrection.isPresented
            || store.session.worklogMove.isPresented || store.session.worklogDeletion.isPresented
            || store.session.taskArchiving.isPresented
            || store.session.bulkTaskArchiving.isPresented || store.trackingError != nil
    }

    private func update() {
        guard isRunning, !isChoosingOwner else { return }
        guard hasPresentation else {
            setOwner(nil)
            return
        }
        guard ownerID == nil else { return }
        isChoosingOwner = true
        defer { isChoosingOwner = false }
        let needsEditor =
            store.session.taskCreation.isPresented || store.session.taskRename.isPresented
            || store.session.worklogCorrection.isPresented
            || store.session.worklogMove.isPresented || store.session.worklogDeletion.isPresented
            || store.session.taskArchiving.isPresented
            || store.session.bulkTaskArchiving.isPresented
        setOwner(presentingWindow(needsEditor))
    }

    private func setOwner(_ windowID: UUID?) {
        guard ownerID != windowID else { return }
        objectWillChange.send()
        ownerID = windowID
    }
}

#if compiler(>=6.2)
    extension TrackerTaskPresentationCoordinator: @MainActor ObservableObject {}
#else
    extension TrackerTaskPresentationCoordinator: ObservableObject {}
#endif

@MainActor
struct TrackerDetailPresentation: View {
    @ObservedObject var store: TrackerStore
    @ObservedObject var presentation: TrackerTaskPresentationCoordinator
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
                TaskCreationDialog(session: store.session, creation: store.creation, isPresentationOwner: isOwner)
                TaskRenameDialog(session: store.session, rename: store.rename, isPresentationOwner: isOwner)
                WorklogCorrectionDialog(
                    session: store.session, correction: store.correction, isPresentationOwner: isOwner)
                WorklogMoveDialog(session: store.session, move: store.move, isPresentationOwner: isOwner)
                WorklogDeletionDialog(session: store.session, deletion: store.deletion, isPresentationOwner: isOwner)
                BulkTaskArchivingDialog(
                    session: store.session, archiving: store.bulkArchiving, isPresentationOwner: isOwner)
                TaskArchivingDialog(session: store.session, archiving: store.archiving, isPresentationOwner: isOwner)
            }
            .alert("Could not change tracking", isPresented: trackingFailurePresented) {
                Button("OK", role: .cancel) { store.dismissTrackingError() }
            } message: {
                Text(store.trackingError ?? "Please try again.")
            }
    }
}

import AppKit
import Combine
import SwiftUI

@MainActor
final class TrackerTaskPresentationCoordinator {
    let objectWillChange = ObservableObjectPublisher()
    private(set) var ownerID: ObjectIdentifier?
    private weak var ownerWindow: NSWindow?
    private let store: TrackerStore
    private let presentingWindow: (Bool) -> NSWindow?
    private var subscriptions = Set<AnyCancellable>()
    private var isRunning = false
    private var isChoosingOwner = false

    init(store: TrackerStore, presentingWindow: @escaping (Bool) -> NSWindow?) {
        self.store = store
        self.presentingWindow = presentingWindow
    }

    func start() {
        guard !isRunning else { return }
        isRunning = true
        for publisher in [store.objectWillChange, store.creation.objectWillChange, store.rename.objectWillChange] {
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

    func owns(_ windowID: ObjectIdentifier) -> Bool {
        ownerWindow != nil && ownerID == windowID
    }

    func canClose(_ window: NSWindow) -> Bool {
        !owns(ObjectIdentifier(window)) || !hasPresentation
    }

    func windowClosed(_ window: NSWindow) {
        guard ownerID == ObjectIdentifier(window) else { return }
        setOwner(nil)
        update()
    }

    private var hasPresentation: Bool {
        store.creation.state.isPresented || store.rename.state.isPresented || store.trackingError != nil
    }

    private func update() {
        guard isRunning, !isChoosingOwner else { return }
        guard hasPresentation else {
            setOwner(nil)
            return
        }
        guard ownerWindow == nil else { return }
        isChoosingOwner = true
        defer { isChoosingOwner = false }
        let needsEditor = store.creation.state.isPresented || store.rename.state.isPresented
        setOwner(presentingWindow(needsEditor))
    }

    private func setOwner(_ window: NSWindow?) {
        let next = window.map(ObjectIdentifier.init)
        guard ownerID != next else { return }
        objectWillChange.send()
        ownerWindow = window
        ownerID = next
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
    let windowID: ObjectIdentifier

    private var isOwner: Bool { presentation.owns(windowID) }
    private var trackingFailurePresented: Binding<Bool> {
        Binding(get: { isOwner && store.trackingError != nil }, set: {
            if isOwner && !$0 { store.dismissTrackingError() }
        })
    }

    var body: some View {
        TaskDetails(store: store)
            .background {
                TaskCreationDialog(creation: store.creation, isPresentationOwner: isOwner)
                TaskRenameDialog(rename: store.rename, isPresentationOwner: isOwner)
            }
            .alert("Could not change tracking", isPresented: trackingFailurePresented) {
                Button("OK", role: .cancel) { store.dismissTrackingError() }
            } message: {
                Text(store.trackingError ?? "Please try again.")
            }
    }
}

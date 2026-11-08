import Foundation
import Observation

/// Holds the task creation dialog and sends its request.
///
/// The sheet, the context menus and the sidebar toolbar read this store from the SwiftUI environment. Closing leaves
/// the content in place, because SwiftUI renders the sheet during its dismissal animation. `open` replaces it.
@Observable
public final class TaskCreationStore {
    public private(set) var isPresented = false
    public private(set) var name = ""
    public private(set) var isSubmitting = false
    public private(set) var error: String?

    private let client: any TrackerClient
    private let clock: any TrackerClock
    private let connection: ConnectionStore
    private let showTask: @MainActor (TaskItem) -> Void
    private let reloadAfterWrite: @MainActor (_ then: () -> Void) async -> Void

    init(
        client: any TrackerClient, clock: any TrackerClock, connection: ConnectionStore,
        showTask: @escaping @MainActor (TaskItem) -> Void,
        reloadAfterWrite: @escaping @MainActor (_ then: () -> Void) async -> Void
    ) {
        self.client = client
        self.clock = clock
        self.connection = connection
        self.showTask = showTask
        self.reloadAfterWrite = reloadAfterWrite
    }

    public var canOpen: Bool { connection.confirmed }

    public var canSubmit: Bool {
        isPresented && !isSubmitting && !TaskNameEditingPolicy.normalized(name).isEmpty
    }

    public func open() {
        guard canOpen, !isPresented else { return }
        name = ""
        error = nil
        isPresented = true
    }

    public func setName(_ value: String) {
        guard isPresented, !isSubmitting, value != name else { return }
        name = value
        error = nil
    }

    public func cancel() {
        guard !isSubmitting else { return }
        isPresented = false
    }

    public func submit() {
        guard canSubmit else { return }
        if let message = TaskNameEditingPolicy.transportError(name) {
            error = message
            return
        }
        isSubmitting = true
        error = nil
        let occurredAt = formatTimestamp(clock.now)
        Task { [weak self] in
            guard let self else { return }
            do {
                let created = try await client.createTask(name: name, occurredAt: occurredAt).task
                isPresented = false
                isSubmitting = false
                await reloadAfterWrite { self.showTask(created) }
            } catch {
                self.error = writeFailure(error).localizedDescription
                isSubmitting = false
                await reloadAfterWrite({})
            }
        }
    }
}

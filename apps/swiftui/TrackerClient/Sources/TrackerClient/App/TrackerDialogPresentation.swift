import Foundation
#if canImport(Combine)
    import Combine
#endif

public protocol TrackerDialogState: Equatable, Sendable {
    var isPresented: Bool { get }
}

/// A session-owned dialog snapshot with content retained while its sheet dismisses.
@MainActor
public final class TrackerDialogPresentation<State: TrackerDialogState, Availability: Equatable> {
    public private(set) var state: State
    public private(set) var sheetContent: State
    public private(set) var availability: Availability
    #if canImport(Combine)
        public let objectWillChange = ObservableObjectPublisher()
    #endif

    init(state: State, availability: Availability) {
        self.state = state
        sheetContent = state
        self.availability = availability
    }

    func update(state next: State, availability nextAvailability: Availability) {
        guard state != next || availability != nextAvailability else { return }
        #if canImport(Combine)
            objectWillChange.send()
        #endif
        state = next
        availability = nextAvailability
        if next.isPresented { sheetContent = next }
    }
}

public extension TrackerDialogPresentation where Availability == Bool {
    var canOpen: Bool { availability }
}

public typealias TaskCreationModel = TrackerDialogPresentation<TaskCreationPresentation, Bool>
public typealias TaskRenameModel = TrackerDialogPresentation<TaskRenamePresentation, Bool>
public typealias TaskArchivingModel = TrackerDialogPresentation<TaskArchivingPresentation, [String: Bool]>
public typealias BulkTaskArchivingModel = TrackerDialogPresentation<BulkTaskArchivingPresentation, Bool>
public typealias WorklogCorrectionModel = TrackerDialogPresentation<WorklogCorrectionPresentation, Bool>
public typealias WorklogMoveModel = TrackerDialogPresentation<WorklogMovePresentation, Bool>

#if canImport(Combine)
    // Keep publisher access on the main actor when isolated conformances are supported.
    #if compiler(>=6.2)
        extension TrackerDialogPresentation: @MainActor ObservableObject {}
    #else
        extension TrackerDialogPresentation: ObservableObject {}
    #endif
#endif

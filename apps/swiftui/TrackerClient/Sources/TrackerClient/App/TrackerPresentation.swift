import Foundation
import Observation

/// Copies the session values that views read into observable properties.
///
/// Observation notifies on every assignment, including an equal value. `update(from:)` assigns a
/// property only when its value changes, so draft edits, clock ticks, and unchanged polls notify
/// only the views that read the changed values.
@Observable
public final class TrackerPresentation: ChangeGuardedStore {
    public private(set) var bulkTaskArchiving: BulkTaskArchivingPresentation
    public private(set) var bulkTaskArchivingSheetContent: BulkTaskArchivingPresentation
    public private(set) var canOpenBulkTaskArchiving: Bool
    public private(set) var worklogCorrection: WorklogCorrectionPresentation
    public private(set) var worklogCorrectionSheetContent: WorklogCorrectionPresentation
    public private(set) var canOpenWorklogCorrection: Bool
    public private(set) var worklogMove: WorklogMovePresentation
    public private(set) var worklogMoveSheetContent: WorklogMovePresentation
    public private(set) var canOpenWorklogMove: Bool
    public private(set) var worklogDeletion: WorklogDeletionPresentation
    public private(set) var worklogDeletionSheetContent: WorklogDeletionPresentation
    public private(set) var canOpenWorklogDeletion: Bool

    public init(session: TrackerSession) {
        bulkTaskArchiving = session.bulkTaskArchiving
        bulkTaskArchivingSheetContent = session.bulkTaskArchiving
        canOpenBulkTaskArchiving = session.canOpenBulkTaskArchiving
        worklogCorrection = session.worklogCorrection
        worklogCorrectionSheetContent = session.worklogCorrection
        canOpenWorklogCorrection = session.canOpenWorklogCorrection
        worklogMove = session.worklogMove
        worklogMoveSheetContent = session.worklogMove
        canOpenWorklogMove = session.canOpenWorklogMove
        worklogDeletion = session.worklogDeletion
        worklogDeletionSheetContent = session.worklogDeletion
        canOpenWorklogDeletion = session.canOpenWorklogDeletion
    }

    public func update(from session: TrackerSession) {
        set(\.bulkTaskArchiving, session.bulkTaskArchiving)
        set(\.canOpenBulkTaskArchiving, session.canOpenBulkTaskArchiving)
        set(\.worklogCorrection, session.worklogCorrection)
        set(\.canOpenWorklogCorrection, session.canOpenWorklogCorrection)
        set(\.worklogMove, session.worklogMove)
        set(\.canOpenWorklogMove, session.canOpenWorklogMove)
        set(\.worklogDeletion, session.worklogDeletion)
        set(\.canOpenWorklogDeletion, session.canOpenWorklogDeletion)
        // SwiftUI continues rendering a sheet while its dismissal animates, so keep the last presented content.
        if bulkTaskArchiving.isPresented { set(\.bulkTaskArchivingSheetContent, bulkTaskArchiving) }
        if worklogCorrection.isPresented { set(\.worklogCorrectionSheetContent, worklogCorrection) }
        if worklogMove.isPresented { set(\.worklogMoveSheetContent, worklogMove) }
        if worklogDeletion.isPresented { set(\.worklogDeletionSheetContent, worklogDeletion) }
    }
}

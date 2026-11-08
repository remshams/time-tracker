import Foundation
import Observation

/// Copies the session values that views read into observable properties.
///
/// Observation notifies on every assignment, including an equal value. `update(from:)` assigns a
/// property only when its value changes, so draft edits, clock ticks, and unchanged polls notify
/// only the views that read the changed values.
@Observable
public final class TrackerPresentation: ChangeGuardedStore {
    public private(set) var worklogCorrection: WorklogCorrectionPresentation
    public private(set) var worklogCorrectionSheetContent: WorklogCorrectionPresentation
    public private(set) var canOpenWorklogCorrection: Bool

    public init(session: TrackerSession) {
        worklogCorrection = session.worklogCorrection
        worklogCorrectionSheetContent = session.worklogCorrection
        canOpenWorklogCorrection = session.canOpenWorklogCorrection
    }

    public func update(from session: TrackerSession) {
        set(\.worklogCorrection, session.worklogCorrection)
        set(\.canOpenWorklogCorrection, session.canOpenWorklogCorrection)
        // SwiftUI continues rendering a sheet while its dismissal animates, so keep the last presented content.
        if worklogCorrection.isPresented { set(\.worklogCorrectionSheetContent, worklogCorrection) }
    }
}

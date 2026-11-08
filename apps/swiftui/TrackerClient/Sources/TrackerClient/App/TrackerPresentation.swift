import Foundation
import Observation

/// Holds no values, because every dialog now reads its own store. The window still creates and updates it.
@Observable
public final class TrackerPresentation: ChangeGuardedStore {
    public init(session: TrackerSession) {}

    public func update(from session: TrackerSession) {}
}

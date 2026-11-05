import Foundation

/// Explains an uncertain write failure: the request may have reached the tracker, so the user should check the
/// reloaded data before trying again. Other failures are returned unchanged.
func writeFailure(_ error: Error) -> Error {
    guard let failure = error as? BridgeFailure, failure.uncertain else { return error }
    return BridgeFailure(
        message: "Could not confirm the change. Check the reloaded data before trying again. \(failure.message)",
        kind: failure.kind, uncertain: true, requiresRefresh: failure.requiresRefresh)
}

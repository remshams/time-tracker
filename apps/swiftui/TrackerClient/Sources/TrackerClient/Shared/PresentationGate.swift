import Observation

/// The tracker window shows one sheet or alert at a time.
///
/// A dialog store claims the gate while its sheet shows, and `TrackingStore` claims it while its error alert shows.
/// Each dialog store's `canOpen` checks `isClaimed`, so a dialog doesn't open behind another one. The main menu isn't
/// blocked by a sheet, so the New Task command relies on this check. The app reveals the tracker window when a sheet
/// opens.
@Observable
public final class PresentationGate {
    enum Kind {
        case sheet
        case alert
    }

    private var claims: [ObjectIdentifier: Kind] = [:]

    /// True while a sheet or alert shows.
    var isClaimed: Bool { !claims.isEmpty }

    /// True while a sheet shows. A tracking error alert alone doesn't count.
    public var showsSheet: Bool { claims.values.contains(.sheet) }

    /// Claims the gate for `holder` while `isShown` is true and releases it otherwise. An unchanged claim leaves the
    /// state alone, so observers aren't notified.
    func update(_ holder: AnyObject, kind: Kind = .sheet, isShown: Bool) {
        let id = ObjectIdentifier(holder)
        let claim = isShown ? kind : nil
        guard claims[id] != claim else { return }
        claims[id] = claim
    }
}

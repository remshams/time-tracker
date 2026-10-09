import Foundation

public struct TrackerWindowOpenRequest: Equatable, Sendable {
    public let id: UUID
    public let shouldOpen: Bool
}

public struct TrackerWindowRoutingState: Sendable {
    private var windowIDs: [UUID] = []
    private var activeWindowIDs = Set<UUID>()
    private var preferredWindowID: UUID?
    private var pendingWindowID: UUID?

    public init() {}

    public mutating func appeared(_ id: UUID, isActive: Bool) {
        if !windowIDs.contains(id) { windowIDs.append(id) }
        if pendingWindowID == id { pendingWindowID = nil }
        setActive(id, isActive: isActive)
    }

    public mutating func disappeared(_ id: UUID) {
        windowIDs.removeAll { Self.matchesWindowID($0, id) }
        activeWindowIDs.remove(id)
        if preferredWindowID == id { preferredWindowID = nil }
    }

    private static func matchesWindowID(_ windowID: UUID, _ removedID: UUID) -> Bool {
        windowID == removedID
    }

    public mutating func setActive(_ id: UUID, isActive: Bool) {
        guard windowIDs.contains(id) else { return }
        if isActive {
            activeWindowIDs.insert(id)
            preferredWindowID = id
        } else {
            activeWindowIDs.remove(id)
        }
    }

    public mutating func prefer(_ id: UUID) {
        guard windowIDs.contains(id) || pendingWindowID == id else { return }
        preferredWindowID = id
    }

    public var activePresentationWindowID: UUID? {
        if let preferredWindowID, activeWindowIDs.contains(preferredWindowID) {
            return preferredWindowID
        }
        return windowIDs.last { activeWindowIDs.contains($0) }
    }

    public mutating func showTracker(makeID: () -> UUID = UUID.init) -> TrackerWindowOpenRequest {
        if let preferredWindowID, windowIDs.contains(preferredWindowID) {
            return TrackerWindowOpenRequest(id: preferredWindowID, shouldOpen: true)
        }
        if let id = windowIDs.last {
            preferredWindowID = id
            return TrackerWindowOpenRequest(id: id, shouldOpen: true)
        }
        if let pendingWindowID {
            return TrackerWindowOpenRequest(id: pendingWindowID, shouldOpen: false)
        }
        let id = makeID()
        pendingWindowID = id
        preferredWindowID = id
        return TrackerWindowOpenRequest(id: id, shouldOpen: true)
    }
}

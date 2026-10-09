import Foundation

enum TaskNameEditingPolicy {
    static func normalized(_ name: String) -> String {
        let scalars = name.unicodeScalars
        var start = scalars.startIndex
        var end = scalars.endIndex
        while start < end && scalars[start].properties.isWhitespace {
            start = scalars.index(after: start)
        }
        while start < end {
            let previous = scalars.index(before: end)
            guard scalars[previous].properties.isWhitespace else { break }
            end = previous
        }
        return String(scalars[start..<end])
    }

    static func transportError(_ name: String) -> String? {
        name.unicodeScalars.contains(where: isNullScalar)
            ? "Task names must not contain control characters." : nil
    }

    private static func isNullScalar(_ scalar: Unicode.Scalar) -> Bool {
        scalar.value == 0
    }

    static func requiresRecovery(_ error: Error) -> Bool {
        guard let failure = error as? BridgeFailure else { return true }
        return failure.uncertain || failure.requiresRefresh || failure.kind == "unavailable"
            || failure.kind == "protocol"
    }
}

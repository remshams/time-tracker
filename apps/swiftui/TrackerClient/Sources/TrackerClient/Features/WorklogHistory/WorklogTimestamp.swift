import Foundation

func sameWorklogTimestamp(_ lhs: String?, _ rhs: String?) -> Bool {
    if lhs == rhs { return true }
    guard let lhs, let rhs else { return false }
    guard let left = worklogTimestampParts(lhs), let right = worklogTimestampParts(rhs) else { return false }
    return left == right
}

private func worklogTimestampParts(_ value: String) -> (Int64, String)? {
    guard let separator = value.firstIndex(of: "T") else { return nil }
    let suffix = value[value.index(after: separator)...]
    let point = suffix.firstIndex(of: ".")
    let fractionStart = point.map { value.index(after: $0) }
    let digits = fractionStart.map { value[$0...].prefix(while: isWorklogTimestampDigit) } ?? ""[...]
    guard digits.count <= 9, point == nil || !digits.isEmpty else { return nil }
    let whole: String
    if let point, let fractionStart {
        whole = String(value[..<point]) + String(value[value.index(fractionStart, offsetBy: digits.count)...])
    } else {
        whole = value
    }
    let dateFormatter = ISO8601DateFormatter()
    dateFormatter.formatOptions = [.withInternetDateTime]
    guard let date = dateFormatter.date(from: whole) else { return nil }
    return (
        Int64(date.timeIntervalSince1970.rounded()),
        String(digits) + String(repeating: "0", count: 9 - digits.count)
    )
}

private func isWorklogTimestampDigit(_ character: Character) -> Bool {
    character >= "0" && character <= "9"
}

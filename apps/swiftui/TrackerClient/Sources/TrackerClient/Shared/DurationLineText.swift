import Foundation

/// A duration as a line of text shows it: the main window's `H:MM:SS`, or a placeholder such as "-" while no duration is
/// known.
public struct DurationLineText: Equatable, Sendable {
    /// The whole text, such as "1:04:27", "~1:04:27" for a cached total, or "Unavailable".
    public let text: String
    /// The end of `text` that the line dims, such as ":27". A placeholder has none.
    public let seconds: String
    /// What VoiceOver reads, such as "1 hour, 4 minutes" or "about 1 hour, 4 minutes" for a cached total.
    public let spoken: String

    public init(placeholder: String) {
        text = placeholder
        seconds = ""
        spoken = placeholder
    }

    init(seconds: Int, cached: Bool = false) {
        let duration = DurationText(seconds: seconds)
        text = "\(cached ? "~" : "")\(duration.hoursAndMinutes)\(duration.seconds)"
        self.seconds = duration.seconds
        spoken = "\(cached ? "about " : "")\(spokenDuration(seconds))"
    }
}

import SwiftUI
import TrackerClient

/// A duration as `H:MM:SS` with the seconds dimmed. It's one text, so the seconds share the baseline and the font of
/// the hours and minutes, with no gap before them. The caller sets the font and the color.
struct DurationLabel: View {
    let duration: DurationText

    var body: some View { Self.text(duration) }

    /// The same duration as a `Text`, for a line that continues after it.
    static func text(_ duration: DurationText) -> Text {
        Text("\(duration.hoursAndMinutes)\(Text(duration.seconds).foregroundStyle(.secondary))")
    }
}

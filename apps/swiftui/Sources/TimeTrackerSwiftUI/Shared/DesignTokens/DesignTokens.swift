import SwiftUI
import TrackerClient

// scripts/swift-style.py rejects number literals in padding, stack spacing, corner radii, fixed font sizes and
// opacities outside this folder. Add a token here instead of typing the value in a view.

/// The spacing scale for padding and stack spacing, in points.
enum Spacing {
    static let xSmall: CGFloat = 4
    static let small: CGFloat = 8
    static let medium: CGFloat = 12
    static let large: CGFloat = 16
    static let xLarge: CGFloat = 24
    static let xxLarge: CGFloat = 32
}

enum CornerRadius {
    static let small: CGFloat = 4
    static let medium: CGFloat = 8
}

/// Fonts by the role of the text. They build on the system text styles, which are 13 pt for `.body`, 12 for
/// `.callout`, 11 for `.subheadline` and 10 for `.caption` on macOS. Only `display` uses a fixed size.
enum TextRole {
    /// The selected task's name at the top of the detail.
    static let display = Font.system(size: 26, weight: .semibold)
    static let sheetTitle = Font.title2.bold()
    static let sectionTitle = Font.title3.weight(.semibold)
    /// A short line that stands in for missing content, such as "Select a task".
    static let placeholder = Font.title3
    static let heading = Font.headline
    static let detail = Font.subheadline
    static let detailEmphasis = Font.subheadline.weight(.medium)
    static let caption = Font.caption
    static let captionEmphasis = Font.caption.weight(.medium)
    /// The SF Symbol that marks a task's color.
    static let taskMarker = Font.callout.weight(.semibold)

    /// A duration with monospaced digits, so the text keeps its width while the time runs.
    static func duration(_ style: Font.TextStyle = .callout) -> Font {
        .system(style).monospacedDigit()
    }
}

/// Opacities for interaction states and tints.
enum Emphasis {
    /// A control's tint in its task color while it rests.
    static let restingTint = 0.1
    static let hoverTint = 0.15
    static let pressedTint = 0.28
    static let disabled = 0.5
}

enum ColorRole {
    static let error = Color.red
    static let connected = Color.green
    static let confirmation = Color.accentColor

    static func task(_ color: TaskColor) -> Color {
        switch color {
        case .blue: return .blue
        case .teal: return .teal
        case .green: return .green
        case .orange: return .orange
        case .red: return .red
        case .purple: return .purple
        case .pink: return .pink
        }
    }
}

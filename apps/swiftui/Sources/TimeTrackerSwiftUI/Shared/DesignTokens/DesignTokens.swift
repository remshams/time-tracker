import SwiftUI
import TrackerClient

// scripts/swift-style.py rejects number literals in padding, stack, grid and spacer spacing, corner radii, fixed font
// sizes and opacities outside this folder. Add a token here instead of typing the value in a view. The check doesn't
// read icon sizes, border widths or animations; their tokens hold by convention.

/// The spacing scale for padding and stack, grid and spacer spacing, in points.
enum Spacing {
    /// The gap between the parts of one element.
    static let xxSmall: CGFloat = 2
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
    /// A floating panel, such as the Quick Track dialog.
    static let large: CGFloat = 16
}

/// Square frame sizes for icons and the controls that hold one, in points.
enum IconSize {
    /// The tracking pill's running dot and idle ring.
    static let marker: CGFloat = 10
    /// A task indicator, a small spinner, or the tracking pill's marker slot.
    static let regular: CGFloat = 16
    /// The disc of a sidebar row's start button, and the hover circle of a compact icon button.
    static let button: CGFloat = 22
    /// The hover circle of an icon button in the task header.
    static let largeButton: CGFloat = 28
}

enum BorderWidth {
    /// The outline of a card.
    static let hairline: CGFloat = 1
    /// A marker's ring.
    static let marker: CGFloat = 1.75
    /// A halo around a marker.
    static let halo: CGFloat = 3
    /// The "now" needle at the end of a running worklog's bar.
    static let needle: CGFloat = 2
}

/// Animation timing for interaction feedback.
enum Motion {
    /// A press.
    static let quick: Animation = .easeOut(duration: 0.12)
    /// A change of state or hover.
    static let standard: Animation = .easeOut(duration: 0.18)
    /// The scale of a pressed marker.
    static let pressScale = 0.92
}

/// Fonts by the role of the text. They build on the system text styles, which are 13 pt for `.body`, 12 for
/// `.callout`, 11 for `.subheadline` and 10 for `.caption` on macOS. Only `display` and `actionGlyph` use a fixed size.
enum TextRole {
    /// The selected task's name at the top of the detail.
    static let display = Font.system(size: 26, weight: .semibold)
    static let sheetTitle = Font.title2.bold()
    /// The Quick Track dialog's search field, which is its main control.
    static let quickTrackSearch = Font.title2
    static let sectionTitle = Font.title3.weight(.semibold)
    /// A short line that stands in for missing content, such as "Select a task".
    static let placeholder = Font.title3
    static let heading = Font.headline
    static let detail = Font.subheadline
    static let detailEmphasis = Font.subheadline.weight(.medium)
    static let caption = Font.caption
    static let captionEmphasis = Font.caption.weight(.medium)
    /// The tracking pill's task name.
    static let status = Font.body
    /// A day's total where it is the main figure.
    static let dayTotal = Font.system(.title, design: .rounded, weight: .medium).monospacedDigit()
    /// A small symbol next to status text, such as the stale connection's `wifi.slash`.
    static let statusGlyph = Font.caption.weight(.semibold)
    /// The stop or play glyph inside a marker disc. It has a fixed size because the disc has one.
    static let actionGlyph = Font.system(size: 8, weight: .bold)

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
    /// A halo around a marker.
    static let halo = 0.35
    static let disabled = 0.5
    /// An enabled element at full strength, the counterpart of `disabled`.
    static let full = 1.0
    /// A finished worklog's bar while its row isn't hovered.
    static let restingBar = 0.82
    /// The fill under the hatching of a running worklog's bar.
    static let runningFill = 0.28
}

enum ColorRole {
    static let error = Color.red
    static let connected = Color.green
    static let confirmation = Color.accentColor

    /// The disc behind a white action glyph in a task color. Dark mode darkens it so light task colors keep the
    /// contrast; light mode keeps the task color, because darkening turns orange brown.
    static func actionDisc(_ color: TaskColor, in scheme: ColorScheme) -> Color {
        let base = task(color)
        return scheme == .dark ? base.mix(with: .black, by: 0.3) : base
    }

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

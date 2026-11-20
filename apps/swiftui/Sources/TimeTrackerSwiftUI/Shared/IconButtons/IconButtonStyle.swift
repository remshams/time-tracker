import SwiftUI

/// An icon-only button that looks like plain text at rest. Hovering an enabled button shows a neutral circle behind the
/// glyph and pressing deepens it, as macOS toolbar buttons do. A disabled button dims and shows no circle. Menus take
/// the style through `.menuStyle(.button)`.
struct IconButtonStyle: ButtonStyle {
    /// The circle's diameter, which is also the button's hit area.
    let diameter: CGFloat

    func makeBody(configuration: Configuration) -> some View {
        IconButtonBody(configuration: configuration, diameter: diameter)
    }
}

extension EnvironmentValues {
    /// Shows every icon button as hovered, so the design snapshots can render the hover circle without a pointer.
    @Entry var iconButtonsShowHover = false
}

/// The style's view. It owns the hover, because the style itself can't hold state.
private struct IconButtonBody: View {
    let configuration: ButtonStyleConfiguration
    let diameter: CGFloat
    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.iconButtonsShowHover) private var showsHover
    @State private var isHovered = false

    var body: some View {
        let hovered = isEnabled && (isHovered || showsHover)
        let tint = isEnabled && configuration.isPressed ? Emphasis.pressedTint : hovered ? Emphasis.hoverTint : 0
        configuration.label
            .foregroundStyle(hovered ? .primary : .secondary)
            .frame(width: diameter, height: diameter)
            .background { Circle().fill(.primary.opacity(tint)) }
            .contentShape(Circle())
            .opacity(isEnabled ? 1 : Emphasis.disabled)
            .animation(Motion.quick, value: tint)
            .onHover { isHovered = $0 }
            // A disabled button gets no hover events, so the pointer may have left by the time it is enabled again.
            .onChange(of: isEnabled) { if !isEnabled { isHovered = false } }
    }
}

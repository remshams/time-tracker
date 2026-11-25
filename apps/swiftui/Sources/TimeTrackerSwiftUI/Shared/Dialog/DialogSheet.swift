import SwiftUI
import TrackerClient

/// The color of a dialog's header badge: the task's color when the dialog changes a task or its worklog, red when it
/// removes something, and the accent color otherwise.
enum DialogTint {
    case accent
    /// The color of the task with this ID, or the accent color while the dialog has no task.
    case task(id: String?)
    case destructive

    fileprivate func color(in scheme: ColorScheme) -> Color {
        switch self {
        case .accent: .accentColor
        case .task(let id): id.map { ColorRole.actionDisc(TaskColor.forTaskID($0), in: scheme) } ?? .accentColor
        case .destructive: ColorRole.error
        }
    }
}

/// The fixed sizes of a dialog sheet, in points. They are layout sizes, not design tokens, like the history's
/// columns.
enum DialogSize {
    /// A dialog about one task.
    static let width: CGFloat = 440
    /// A dialog that shows a worklog or a list of tasks.
    static let wideWidth: CGFloat = 480
    /// The least height of a row in a grouped section, so text rows line up with rows that hold a control.
    static let row: CGFloat = 40
    /// The least width of a footer button, so a short title such as "Save" still reads as a capsule.
    static let buttonMinWidth: CGFloat = 84
    /// A list of tasks. It keeps its height while it loads or shows a message, so the sheet doesn't resize.
    static let listHeight: CGFloat = 220
}

/// The frame of every dialog sheet, in the macOS settings-sheet style: a header with a glyph badge, the title and a
/// one-line message, then the grouped content and an optional error, then a footer with the progress at the leading
/// edge and large capsule buttons at the trailing edge.
///
/// The caller keeps the identifiers, keyboard shortcuts and disabled states on the controls it passes in. `isBusy`
/// stops a swipe or Escape from dismissing the sheet while a request runs.
struct DialogSheet<Content: View, Status: View, Actions: View>: View {
    let title: String
    let message: String
    let systemImage: String
    var tint = DialogTint.accent
    var width = DialogSize.width
    var error: String? = nil
    var errorIdentifier = ""
    let isBusy: Bool
    @ViewBuilder let content: Content
    @ViewBuilder let status: Status
    @ViewBuilder let actions: Actions

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            DialogHeader(title: title, message: message, systemImage: systemImage, tint: tint)
                .padding(Spacing.xLarge)
            Divider()
            VStack(alignment: .leading, spacing: Spacing.large) {
                content
                if let error {
                    Label(error, systemImage: "exclamationmark.triangle.fill")
                        .accessibilityIdentifier(errorIdentifier)
                        .font(TextRole.detail)
                        .foregroundStyle(ColorRole.error)
                        .fixedSize(horizontal: false, vertical: true)
                        .textSelection(.enabled)
                        .padding(.horizontal, Spacing.medium)
                }
            }
            .padding(.horizontal, Spacing.xLarge)
            .padding(.vertical, Spacing.large)
            Divider()
            HStack(spacing: Spacing.medium) {
                status
                Spacer(minLength: Spacing.medium)
                actions
                    .controlSize(.large)
                    .buttonBorderShape(.capsule)
                    // Flexible sizing lets the capsule fill the least width instead of centering in it.
                    .buttonSizing(.flexible)
                    .frame(minWidth: DialogSize.buttonMinWidth)
                    .fixedSize()
            }
            .padding(.horizontal, Spacing.xLarge)
            .padding(.vertical, Spacing.large)
        }
        .frame(width: width)
        .interactiveDismissDisabled(isBusy)
    }
}

private struct DialogHeader: View {
    let title: String
    let message: String
    let systemImage: String
    let tint: DialogTint
    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        HStack(spacing: Spacing.large) {
            Image(systemName: systemImage)
                .font(TextRole.sheetTitle)
                .foregroundStyle(.white)
                .frame(width: IconSize.dialogBadge, height: IconSize.dialogBadge)
                .background(tint.color(in: colorScheme).gradient, in: Circle())
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                Text(title)
                    .font(TextRole.sheetTitle)
                    .accessibilityAddTraits(.isHeader)
                Text(message)
                    .font(TextRole.body)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .help(message)
            }
        }
    }
}

/// A rounded group of rows in the System Settings style, with hairlines between the rows and helper text under the
/// group. `field` is the focus of a text field that fills the group: the group outlines itself in the accent color
/// while the field has focus, and a click anywhere in the group focuses the field, so the whole group reads as the
/// place to type.
struct DialogSection<Content: View, Footer: View>: View {
    var field: FocusState<Bool>.Binding? = nil
    @ViewBuilder let content: Content
    @ViewBuilder let footer: Footer

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.small) {
            let group = VStack(alignment: .leading, spacing: 0) {
                Group(subviews: content) { rows in
                    ForEach(rows) { row in
                        if row.id != rows.first?.id {
                            Divider().padding(.leading, Spacing.medium)
                        }
                        row
                            .padding(.horizontal, Spacing.medium)
                            .padding(.vertical, Spacing.small)
                            .frame(maxWidth: .infinity, minHeight: DialogSize.row, alignment: .leading)
                    }
                }
            }
            .labeledContentStyle(DialogRowStyle())
            .dialogGroup(isFocused: field?.wrappedValue == true)
            // Only a field's group takes clicks, so other groups pass them straight to their controls.
            if let field {
                group.contentShape(Rectangle()).onTapGesture { field.wrappedValue = true }
            } else {
                group
            }

            footer
                .font(TextRole.detail)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.horizontal, Spacing.medium)
        }
    }
}

extension DialogSection where Footer == EmptyView {
    init(field: FocusState<Bool>.Binding? = nil, @ViewBuilder content: () -> Content) {
        self.init(field: field, content: content, footer: { EmptyView() })
    }
}

extension View {
    /// The rounded fill and hairline of a dialog group, for content such as a list that doesn't take the rows of a
    /// `DialogSection`. `isFocused` draws the outline in the accent color.
    func dialogGroup(isFocused: Bool = false) -> some View {
        let shape = RoundedRectangle(cornerRadius: CornerRadius.medium)
        return background(.fill.quaternary, in: shape)
            .clipShape(shape)
            .overlay {
                shape.strokeBorder(
                    isFocused ? AnyShapeStyle(.tint) : AnyShapeStyle(.separator),
                    lineWidth: isFocused ? BorderWidth.focus : BorderWidth.hairline)
            }
            .animation(Motion.standard, value: isFocused)
    }
}

/// A row with the label at the leading edge and the value or control at the trailing edge.
private struct DialogRowStyle: LabeledContentStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: Spacing.medium) {
            configuration.label
            Spacer(minLength: Spacing.medium)
            configuration.content
        }
    }
}

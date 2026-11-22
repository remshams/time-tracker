import AppKit
import SwiftUI
import TrackerClient

/// Sizes of the Quick Track dialog.
enum QuickTrackLayout {
    static let width: CGFloat = 560
    static let rowHeight: CGFloat = 32
    /// The list shows this many rows before it scrolls.
    static let visibleRows = 10
    /// How far below the top of the screen's visible area the dialog's top edge sits, as a share of its height.
    static let topOffsetRatio: CGFloat = 0.22
    /// The scroll ID of the "Create and start" row.
    static let createRowID = "quick-track.create"
}

/// The Quick Track dialog: a search field, the running task with today's total, status lines and the task list.
///
/// Keyboard focus stays in the search field. The arrow keys move the store's highlight and Return acts on it.
struct QuickTrackView: View {
    let store: QuickTrackStore
    @FocusState private var searchFocused: Bool

    var body: some View {
        let content = store.content
        VStack(spacing: 0) {
            searchField
            Divider()
            QuickTrackHeaderRow(
                header: content.header, isHighlighted: content.highlight == .header,
                stop: { act(on: .header) })
            if !content.statusLines.isEmpty {
                QuickTrackStatusLines(lines: content.statusLines)
            }
            Divider()
            taskList(content)
            Divider()
            QuickTrackKeyHints()
        }
        .frame(width: QuickTrackLayout.width)
        .glassEffect(.regular, in: .rect(cornerRadius: CornerRadius.large))
        .onAppear { searchFocused = true }
        .onChange(of: store.isPresented) { _, isPresented in
            if isPresented { searchFocused = true }
        }
        .onChange(of: announcement(content)) { _, text in
            if let text { AccessibilityNotification.Announcement(text).post() }
        }
    }

    private var searchField: some View {
        HStack(spacing: Spacing.small) {
            Image(systemName: "magnifyingglass")
                .font(TextRole.quickTrackSearch)
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)
            TextField("Start tracking a task", text: Binding(get: { store.query }, set: { store.setQuery($0) }))
                .textFieldStyle(.plain)
                .font(TextRole.quickTrackSearch)
                .focused($searchFocused)
                .accessibilityIdentifier("quick-track.search")
                .accessibilityLabel("Search tasks")
                .onSubmit { store.activate() }
                .onKeyPress(.downArrow) {
                    store.moveHighlight(down: true)
                    return .handled
                }
                .onKeyPress(.upArrow) {
                    store.moveHighlight(down: false)
                    return .handled
                }
                .onKeyPress(.escape) {
                    store.cancel()
                    return .handled
                }
        }
        .padding(.horizontal, Spacing.large)
        .padding(.vertical, Spacing.medium)
    }

    private func taskList(_ content: QuickTrackContent) -> some View {
        let count = content.tasks.count + (content.createName == nil ? 0 : 1)
        let rows = min(max(count, 1), QuickTrackLayout.visibleRows)
        return ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: 0) {
                    ForEach(content.tasks) { task in
                        QuickTrackTaskRow(
                            task: task, isHighlighted: content.highlight == .task(task.id),
                            start: { act(on: .task(task.id)) },
                            copyValue: { store.copyValue($0, taskID: task.id) }
                        )
                        .id(task.id)
                    }
                    if let name = content.createName {
                        QuickTrackCreateRow(
                            name: name, isHighlighted: content.highlight == .create, isEnabled: content.canCreate,
                            isCreating: store.isCreating, create: { act(on: .create) }
                        )
                        .id(QuickTrackLayout.createRowID)
                    }
                    if content.tasks.isEmpty && content.createName == nil {
                        Text(emptyText)
                            .font(TextRole.detail)
                            .foregroundStyle(.secondary)
                            .frame(maxWidth: .infinity, minHeight: QuickTrackLayout.rowHeight)
                            .accessibilityIdentifier("quick-track.empty")
                    }
                }
                .padding(.horizontal, Spacing.small)
            }
            .scrollIndicators(.automatic)
            .frame(height: CGFloat(rows) * QuickTrackLayout.rowHeight)
            .padding(.vertical, Spacing.xSmall)
            .onChange(of: content.highlight) { _, highlight in
                switch highlight {
                case .task(let id): proxy.scrollTo(id)
                case .create: proxy.scrollTo(QuickTrackLayout.createRowID)
                case .header, nil: break
                }
            }
        }
    }

    private var emptyText: String {
        if store.isSearching { return "" }
        return QuickTrackContent.isBlank(store.query) ? "No tasks" : "No matching tasks"
    }

    /// Highlights the clicked row, then runs it like Return.
    private func act(on row: QuickTrackHighlight) {
        store.highlight(row)
        store.activate()
    }

    /// The result count VoiceOver reads once a search for a typed query finishes.
    private func announcement(_ content: QuickTrackContent) -> String? {
        guard !store.isSearching, !store.query.isEmpty else { return nil }
        return content.tasks.count == 1 ? "1 task" : "\(content.tasks.count) tasks"
    }
}

/// The running task with its elapsed time and a stop button, or "Not tracking", and today's total.
private struct QuickTrackHeaderRow: View {
    let header: QuickTrackHeader
    let isHighlighted: Bool
    let stop: () -> Void

    var body: some View {
        HStack(spacing: Spacing.small) {
            if let taskID = header.taskID {
                TaskIndicatorView(taskID: taskID, isRunning: true, hasTimeToday: true)
            } else {
                Image(systemName: "circle.dashed")
                    .foregroundStyle(.secondary)
                    .frame(width: IconSize.regular, height: IconSize.regular)
                    .accessibilityHidden(true)
            }
            VStack(alignment: .leading, spacing: Spacing.xxSmall) {
                Text(header.title)
                    .font(TextRole.heading)
                    .lineLimit(1)
                    .truncationMode(.middle)
                if let elapsed = header.elapsedText {
                    Text(elapsed.text)
                        .font(TextRole.duration(.caption))
                        .foregroundStyle(.secondary)
                        .accessibilityLabel(elapsed.spoken)
                }
            }
            Spacer(minLength: Spacing.medium)
            if header.isRunning {
                Button("Stop", systemImage: "stop.fill", action: stop)
                    .controlSize(.small)
                    .disabled(!header.canStop)
                    .help("Stop tracking")
                    .accessibilityLabel("Stop tracking")
                    .accessibilityIdentifier("quick-track.stop")
            }
            VStack(alignment: .trailing, spacing: Spacing.xxSmall) {
                Text("Today")
                    .font(TextRole.caption)
                    .foregroundStyle(.secondary)
                Text(header.totalText.text)
                    .font(TextRole.duration())
                    .accessibilityLabel(header.totalText.spoken)
            }
            .help(header.totalsExplanation)
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("quick-track.total")
        }
        .padding(.horizontal, Spacing.medium)
        .padding(.vertical, Spacing.small)
        .background(highlightBackground(isHighlighted))
        .padding(.horizontal, Spacing.small)
        .padding(.vertical, Spacing.xSmall)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("quick-track.header")
        .accessibilityValue(isHighlighted ? "Highlighted" : "")
    }
}

/// Connection, auto-pause, tracking and search messages.
private struct QuickTrackStatusLines: View {
    let lines: [String]

    var body: some View {
        VStack(alignment: .leading, spacing: Spacing.xxSmall) {
            ForEach(Array(lines.enumerated()), id: \.offset) { _, line in
                Text(line)
            }
        }
        .font(TextRole.caption)
        .foregroundStyle(.secondary)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, Spacing.large)
        .padding(.bottom, Spacing.small)
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("quick-track.status")
    }
}

/// A task with its color mark and its time today.
private struct QuickTrackTaskRow: View {
    let task: QuickTrackTask
    let isHighlighted: Bool
    let start: () -> Void
    let copyValue: (MenuShortcutAction) -> String?
    @State private var isHovered = false

    var body: some View {
        HStack(spacing: Spacing.small) {
            TaskIndicatorView(taskID: task.id, isRunning: false, hasTimeToday: task.durationText != nil)
            Text(task.task.name)
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer(minLength: Spacing.medium)
            if let duration = task.durationText {
                Text(duration.text)
                    .font(TextRole.duration())
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.horizontal, Spacing.small)
        .frame(height: QuickTrackLayout.rowHeight)
        .background(highlightBackground(isHighlighted, hovered: isHovered))
        .opacity(task.canStart ? Emphasis.full : Emphasis.disabled)
        .contentShape(Rectangle())
        .onHover { isHovered = $0 }
        .onTapGesture(perform: start)
        .contextMenu {
            ForEach([MenuShortcutAction.copyName, .copyExact, .copyRounded], id: \.self) { action in
                let value = copyValue(action)
                Button(action.title) { if let value { QuickTrackPasteboard.copy(value) } }
                    .disabled(value == nil)
                    .accessibilityIdentifier("quick-track.copy.\(task.id).\(action.rawValue)")
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityAddTraits(.isButton)
        .accessibilityLabel(accessibilityLabel)
        .accessibilityValue(isHighlighted ? "Highlighted" : "")
        .accessibilityIdentifier("quick-track.task.\(task.id)")
        .accessibilityAction(.default, start)
        .help(task.canStart ? "Start tracking \(task.task.name)" : "Tracking is unavailable right now")
    }

    private var accessibilityLabel: String {
        let time = task.durationText.map { "\($0.spoken) today" } ?? "no time today"
        return "\(task.task.name), \(time)\(task.canStart ? "" : ", unavailable")"
    }
}

/// The row that creates a task named after the search text and starts it.
private struct QuickTrackCreateRow: View {
    let name: String
    let isHighlighted: Bool
    let isEnabled: Bool
    let isCreating: Bool
    let create: () -> Void
    @State private var isHovered = false

    var body: some View {
        HStack(spacing: Spacing.small) {
            Image(systemName: "plus.circle")
                .foregroundStyle(.secondary)
                .frame(width: IconSize.regular, height: IconSize.regular)
            Text(isCreating ? "Creating \u{201C}\(name)\u{201D}" : "Create and start \u{201C}\(name)\u{201D}")
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer(minLength: Spacing.medium)
            if isCreating { ProgressView().controlSize(.small) }
        }
        .padding(.horizontal, Spacing.small)
        .frame(height: QuickTrackLayout.rowHeight)
        .background(highlightBackground(isHighlighted, hovered: isHovered))
        .opacity(isEnabled ? Emphasis.full : Emphasis.disabled)
        .contentShape(Rectangle())
        .onHover { isHovered = $0 }
        .onTapGesture(perform: create)
        .accessibilityElement(children: .ignore)
        .accessibilityAddTraits(.isButton)
        .accessibilityLabel("Create and start \(name)")
        .accessibilityValue(isHighlighted ? "Highlighted" : "")
        .accessibilityIdentifier(QuickTrackLayout.createRowID)
        .accessibilityAction(.default, create)
    }
}

/// Writes a copied value to the general pasteboard, or beeps when that fails.
enum QuickTrackPasteboard {
    static func copy(_ value: String) {
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        if !pasteboard.setString(value, forType: .string) { NSSound.beep() }
    }
}

/// The keys the dialog understands.
private struct QuickTrackKeyHints: View {
    var body: some View {
        HStack(spacing: Spacing.large) {
            hint("return", "Start or stop")
            hint("↑ ↓", "Move")
            hint("esc", "Clear or close")
            Spacer()
        }
        .font(TextRole.caption)
        .foregroundStyle(.secondary)
        .padding(.horizontal, Spacing.large)
        .padding(.vertical, Spacing.small)
        .accessibilityHidden(true)
    }

    private func hint(_ key: String, _ action: String) -> some View {
        HStack(spacing: Spacing.xSmall) {
            Text(key).font(TextRole.captionEmphasis)
            Text(action)
        }
    }
}

private func highlightBackground(_ isHighlighted: Bool, hovered: Bool = false) -> some View {
    RoundedRectangle(cornerRadius: CornerRadius.medium)
        .fill(
            isHighlighted
                ? Color.accentColor.opacity(Emphasis.pressedTint)
                : hovered ? Color.primary.opacity(Emphasis.restingTint) : Color.clear)
}

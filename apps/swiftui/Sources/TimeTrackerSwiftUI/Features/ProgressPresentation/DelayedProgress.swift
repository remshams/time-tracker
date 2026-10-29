import SwiftUI
import TrackerClient

struct DelayedProgress<Content: View>: View {
    let isBusy: Bool
    @ViewBuilder let content: (Bool) -> Content
    @State private var presentation = DelayedProgressPresentation()
    @State private var isVisible = false

    var body: some View {
        content(isBusy && isVisible)
            .onAppear { update(isBusy) }
            .onChange(of: isBusy) { _, busy in update(busy) }
            .onDisappear {
                presentation.cancel()
                isVisible = false
            }
    }

    private func update(_ busy: Bool) {
        let visibility = $isVisible
        presentation.update(isBusy: busy) { visibility.wrappedValue = $0 }
    }
}

struct DelayedProgressIndicator: View {
    let isBusy: Bool
    var title: String? = nil
    var identifier = "progress.indicator"

    var body: some View {
        DelayedProgress(isBusy: isBusy) { visible in
            HStack(spacing: 12) {
                ProgressView().controlSize(.small)
                    .frame(width: 16, height: 16)
                    .accessibilityIdentifier(identifier)
                if let title { Text(title).foregroundStyle(.secondary) }
            }
            .opacity(visible ? 1 : 0)
            .accessibilityHidden(!visible)
        }
    }
}

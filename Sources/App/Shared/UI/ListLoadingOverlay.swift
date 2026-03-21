import SwiftUI

extension View {
    func listLoadingOverlay<Empty: View>(
        isLoading: Bool,
        errorTitle: String,
        errorMessage: String?,
        @ViewBuilder emptyOverlay: () -> Empty
    ) -> some View {
        overlay {
            if isLoading {
                ProgressView()
            } else if let errorMessage {
                PlaceholderView(
                    systemImage: "exclamationmark.triangle",
                    title: errorTitle,
                    description: errorMessage)
            } else {
                emptyOverlay()
            }
        }
    }

    func listLoadingOverlay(
        isLoading: Bool,
        errorTitle: String,
        errorMessage: String?
    ) -> some View {
        listLoadingOverlay(
            isLoading: isLoading,
            errorTitle: errorTitle,
            errorMessage: errorMessage,
            emptyOverlay: { EmptyView() }
        )
    }
}

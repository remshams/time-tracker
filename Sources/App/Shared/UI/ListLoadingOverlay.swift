import SwiftUI

extension View {
    func listLoadingOverlay(
        isLoading: Bool,
        errorTitle: String,
        errorMessage: String?,
        emptyOverlay: AnyView? = nil
    ) -> some View {
        overlay {
            if isLoading {
                ProgressView()
            } else if let errorMessage {
                PlaceholderView(
                    systemImage: "exclamationmark.triangle",
                    title: errorTitle,
                    description: errorMessage)
            } else if let emptyOverlay {
                emptyOverlay
            }
        }
    }
}

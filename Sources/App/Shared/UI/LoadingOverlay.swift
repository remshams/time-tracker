import SwiftUI

extension View {
  func loadingOverlay(
    isLoading: Bool,
    errorTitle: String,
    errorMessage: String?,
    @ViewBuilder emptyOverlay: () -> some View
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

  func loadingOverlay(
    isLoading: Bool,
    errorTitle: String,
    errorMessage: String?
  ) -> some View {
    loadingOverlay(
      isLoading: isLoading,
      errorTitle: errorTitle,
      errorMessage: errorMessage,
      emptyOverlay: { EmptyView() }
    )
  }
}

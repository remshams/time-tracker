import SwiftUI

extension List {
  func listHeader(@ViewBuilder content: () -> some View) -> some View {
    safeAreaInset(edge: .top, spacing: 0) {
      content()
        .background(.bar)
    }
  }
}

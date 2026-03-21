import SwiftUI

extension View {
    func listHeader(@ViewBuilder content: () -> some View) -> some View {
        safeAreaInset(edge: .top, spacing: 0) {
            content()
                .background(.bar)
        }
    }
}

import SwiftUI

struct TrackerSplitLayout<Sidebar: View, Detail: View>: View {
    @Binding var columnVisibility: NavigationSplitViewVisibility
    @ViewBuilder var sidebar: () -> Sidebar
    @ViewBuilder var detail: () -> Detail

    var body: some View {
        NavigationSplitView(columnVisibility: $columnVisibility) {
            sidebar()
                .navigationSplitViewColumnWidth(min: 280, ideal: 340, max: 480)
        } detail: {
            detail()
                .frame(minWidth: 380)
        }
        .navigationSplitViewStyle(.balanced)
    }
}

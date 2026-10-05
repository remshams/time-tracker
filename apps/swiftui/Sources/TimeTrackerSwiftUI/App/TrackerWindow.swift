import SwiftUI

@MainActor
struct TrackerWindow: View {
    @ObservedObject var store: TrackerStore
    let statusItem: TrackerStatusItemController
    @Environment(\.openWindow) private var openWindow
    @State private var showsSettingsSheet = false

    private var trackingFailurePresented: Binding<Bool> {
        Binding(get: { store.trackingError != nil }, set: {
            if !$0 { store.dismissTrackingError() }
        })
    }

    var body: some View {
        layout
            .frame(minWidth: 760, minHeight: 480)
            .focusedSceneObject(store.creation)
            .background {
                TaskCreationDialog(creation: store.creation)
                TaskRenameDialog(rename: store.rename)
            }
            .sheet(isPresented: $showsSettingsSheet) {
                ConnectionSettingsView(store: store, showsDoneButton: true)
            }
            .onAppear {
                statusItem.start { openWindow(id: "tracker") }
            }
            .alert("Could not change tracking", isPresented: trackingFailurePresented) {
                Button("OK", role: .cancel) { store.dismissTrackingError() }
            } message: {
                Text(store.trackingError ?? "Please try again.")
            }
    }

    @ViewBuilder
    private var layout: some View {
        #if compiler(>=6.0)
        if #available(macOS 14, *) {
            TrackerSettingsLayout(store: store)
        } else {
            legacySettingsLayout
        }
        #else
        legacySettingsLayout
        #endif
    }

    private var legacySettingsLayout: some View {
        TrackerSplitLayout(store: store, openSettings: { showsSettingsSheet = true })
    }
}

#if compiler(>=6.0)
@available(macOS 14, *)
@MainActor
private struct TrackerSettingsLayout: View {
    let store: TrackerStore
    @Environment(\.openSettings) private var openSettings

    var body: some View {
        TrackerSplitLayout(store: store, openSettings: { openSettings() })
    }
}
#endif

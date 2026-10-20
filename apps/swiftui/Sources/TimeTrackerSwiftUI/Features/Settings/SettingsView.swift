import SwiftUI

@MainActor
struct SettingsView: View {
    let store: TrackerStore
    @State private var connectionDraft: ConnectionDraftStore

    init(store: TrackerStore) {
        self.store = store
        _connectionDraft = State(initialValue: ConnectionDraftStore(store: store))
    }

    var body: some View {
        Form {
            ConnectionSettingsSection(store: store, draft: connectionDraft)
            TrackingSettingsSection(store: store)
            MenuBarSettingsSection(store: store)
            MenuShortcutSettingsSection(store: store)
            ConnectionActionsSection(draft: connectionDraft)
            CurrentConnectionSection(store: store)
        }
        .formStyle(.grouped)
        .frame(width: 540)
        .connectionDraftLifecycle(connectionDraft, store: store)
    }
}

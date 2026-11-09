import SwiftUI

struct SettingsView: View {
    let store: TrackerStore
    @State private var connectionDraft: ConnectionDraftStore

    init(store: TrackerStore) {
        self.store = store
        _connectionDraft = State(initialValue: ConnectionDraftStore(store: store))
    }

    var body: some View {
        Form {
            ConnectionSettingsSection(draft: connectionDraft)
            TrackingSettingsSection(store: store)
            MenuBarSettingsSection()
            MenuShortcutSettingsSection()
            ConnectionActionsSection(draft: connectionDraft)
            CurrentConnectionSection()
        }
        .formStyle(.grouped)
        .frame(width: 540)
        .connectionDraftLifecycle(connectionDraft, store: store)
    }
}

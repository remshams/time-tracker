import SwiftUI
import TrackerClient

struct SettingsView: View {
    @State private var connectionDraft: ConnectionDraftStore

    init(connection: ConnectionStore, sync: TrackerSync, client: any TrackerClient) {
        _connectionDraft = State(
            initialValue: ConnectionDraftStore(connection: connection, sync: sync, client: client))
    }

    var body: some View {
        Form {
            ConnectionSettingsSection(draft: connectionDraft)
            TrackingSettingsSection()
            MenuBarSettingsSection()
            MenuShortcutSettingsSection()
            ConnectionActionsSection(draft: connectionDraft)
            CurrentConnectionSection()
        }
        .formStyle(.grouped)
        .frame(width: 540)
        .connectionDraftLifecycle(connectionDraft)
    }
}

import SwiftUI

@MainActor
struct SettingsView: View {
    let store: TrackerStore
    var showsDoneButton = false
    @Environment(\.dismiss) private var dismiss
    @StateObject private var connectionDraft: ConnectionDraftStore

    init(store: TrackerStore, showsDoneButton: Bool = false) {
        self.store = store
        self.showsDoneButton = showsDoneButton
        _connectionDraft = StateObject(wrappedValue: ConnectionDraftStore(store: store))
    }

    var body: some View {
        VStack(spacing: 0) {
            Form {
                ConnectionSettingsSection(store: store, draft: connectionDraft)
                TrackingSettingsSection(store: store)
                MenuBarSettingsSection(store: store)
                MenuShortcutSettingsSection(store: store)
                ConnectionActionsSection(draft: connectionDraft, activity: store.activity)
                CurrentConnectionSection(store: store)
            }
            .formStyle(.grouped)

            if showsDoneButton {
                HStack {
                    Spacer()
                    Button("Done") { dismiss() }
                        .keyboardShortcut(.cancelAction)
                }
                .padding(16)
            }
        }
        .frame(width: 540)
        .connectionDraftLifecycle(connectionDraft, store: store)
    }
}

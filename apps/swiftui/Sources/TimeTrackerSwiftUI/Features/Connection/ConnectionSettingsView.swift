import SwiftUI
import TrackerClient

@MainActor
struct ConnectionSettingsView: View {
    @ObservedObject var store: TrackerStore
    @ObservedObject private var activity: TrackerActivityStore
    var showsDoneButton = false
    @Environment(\.dismiss) private var dismiss
    @State private var draft: ConnectionSettings
    @State private var operationPending = false
    @State private var presentationGeneration = UUID()
    @State private var resultMessage: String?
    @State private var resultSucceeded = false

    init(store: TrackerStore, showsDoneButton: Bool = false) {
        self.store = store
        activity = store.activity
        self.showsDoneButton = showsDoneButton
        _draft = State(initialValue: store.connectionSettings)
    }

    private var controlsDisabled: Bool { operationPending || activity.isBlockingControls }
    private var missingEndpoint: Bool {
        draft.mode == .server && draft.serverURL.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    var body: some View {
        VStack(spacing: 0) {
            Form {
                Section("Connection") {
                    Picker("Data source", selection: $draft.mode) {
                        ForEach(ConnectionMode.allCases) { mode in
                            Text(mode.label).tag(mode)
                        }
                    }

                    if draft.mode == .server {
                        TextField("Server URL", text: $draft.serverURL,
                                  prompt: Text("http://server-address:port"))
                            .textFieldStyle(.roundedBorder)
                            .disableAutocorrection(true)
                        Text("Enter the full HTTP or HTTPS address of your tracker server.")
                            .foregroundStyle(.secondary)
                    } else {
                        Text("Use the task database stored on this Mac.")
                            .foregroundStyle(.secondary)
                    }
                }
                .disabled(operationPending || store.isChangingConnection)

                Section("Tracking") {
                    Toggle("Automatically pause tracking when the screen is locked", isOn: Binding(
                        get: { store.pauseOnScreenLock },
                        set: { store.setPauseOnScreenLock($0) }
                    ))
                    Text("Resume the same task when you unlock this Mac, unless another timer is already running.")
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                    if let status = store.autoPauseStatusText {
                        Text(status)
                            .foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }

                Section("Menu bar") {
                    Toggle("Show today's total next to the icon", isOn: Binding(
                        get: { store.showDailyTotalInMenuBar },
                        set: { store.setShowDailyTotalInMenuBar($0) }
                    ))
                }

                Section("Menu shortcut") {
                    ForEach([MenuShortcutAction.openMenu], id: \.self) { action in
                        HStack {
                            Text(action.title)
                            Spacer()
                            ShortcutRecorder(shortcut: store.menuShortcuts[action]) { shortcut in
                                store.setMenuShortcut(action, shortcut: shortcut)
                            }
                            .frame(width: 160, height: 28)
                            .accessibilityLabel("\(action.title) shortcut")
                        }
                    }
                    Text("Click the shortcut and press its new keys. Escape cancels. Open menu opens the native menu across apps. Use arrow keys to navigate and open task submenus for tracking and copying.")
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                    Text("Task submenus can copy the task name, today's exact time, or today's time rounded to the nearest 15 minutes.")
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                    if let error = store.menuShortcutError {
                        Text(error).foregroundStyle(.red)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    Button("Restore default shortcut") { store.resetMenuShortcuts() }
                }

                Section {
                    HStack(spacing: 12) {
                        Button("Test connection") { testConnection() }
                            .disabled(controlsDisabled || missingEndpoint)
                        Button("Connect") { connect() }
                            .buttonStyle(.borderedProminent)
                            .disabled(controlsDisabled || missingEndpoint)
                            .keyboardShortcut(.defaultAction)
                        if operationPending {
                            ProgressView().controlSize(.small)
                        }
                    }

                    if let resultMessage {
                        Label {
                            Text(resultMessage)
                                .fixedSize(horizontal: false, vertical: true)
                                .textSelection(.enabled)
                        } icon: {
                            Image(systemName: resultSucceeded ? "checkmark.circle" : "exclamationmark.triangle")
                                .foregroundStyle(resultSucceeded ? Color.accentColor : Color.red)
                        }
                    }
                }

                Section("Current connection") {
                    LabeledContent("Data source", value: store.connectionSettings.mode.label)
                    if store.connectionSettings.mode == .server {
                        Text(store.connectionSettings.serverURL)
                            .textSelection(.enabled)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    Text(store.connectionStatusText)
                        .foregroundStyle(.secondary)
                    if let message = store.connectionMessage {
                        Text(message)
                            .foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
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
        .onAppear {
            presentationGeneration = UUID()
            draft = store.connectionSettings
            operationPending = false
            resultMessage = nil
            resultSucceeded = false
        }
        .onDisappear { presentationGeneration = UUID() }
        .onChange(of: draft) { _ in resultMessage = nil }
        .onChange(of: store.connectionSettings) { settings in draft = settings }
    }

    private func testConnection() {
        let settings = draft
        let generation = presentationGeneration
        operationPending = true
        resultMessage = nil
        Task { @MainActor in
            do {
                try await store.testConnection(settings)
                guard presentationGeneration == generation else { return }
                resultSucceeded = true
                resultMessage = "Connection succeeded. Click Connect to use this data source."
            } catch {
                guard presentationGeneration == generation else { return }
                resultSucceeded = false
                resultMessage = error.localizedDescription
            }
            operationPending = false
        }
    }

    private func connect() {
        let settings = draft
        let generation = presentationGeneration
        operationPending = true
        resultMessage = nil
        Task { @MainActor in
            let succeeded = await store.connect(settings)
            guard presentationGeneration == generation else { return }
            resultSucceeded = succeeded
            resultMessage = resultSucceeded
                ? "Connected. This data source will be used next time you open the app."
                : store.connectionMessage ?? "Could not connect. Please try again."
            operationPending = false
        }
    }
}

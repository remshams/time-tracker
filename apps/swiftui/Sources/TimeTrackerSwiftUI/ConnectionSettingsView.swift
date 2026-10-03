import AppKit
import SwiftUI

@MainActor
struct ConnectionSettingsView: View {
    @ObservedObject var store: TrackerStore
    var showsDoneButton = false
    @Environment(\.dismiss) private var dismiss
    @State private var draft: ConnectionSettings
    @State private var operationPending = false
    @State private var resultMessage: String?
    @State private var resultSucceeded = false

    init(store: TrackerStore, showsDoneButton: Bool = false) {
        self.store = store
        self.showsDoneButton = showsDoneButton
        _draft = State(initialValue: store.connectionSettings)
    }

    private var controlsDisabled: Bool { operationPending || store.isBusy }
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
                                .foregroundStyle(resultSucceeded ? Color.accentColor : Color(nsColor: .systemRed))
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
            draft = store.connectionSettings
            resultMessage = nil
        }
        .onChange(of: draft) { _ in resultMessage = nil }
        .onChange(of: store.connectionSettings) { settings in draft = settings }
    }

    private func testConnection() {
        let settings = draft
        operationPending = true
        resultMessage = nil
        Task { @MainActor in
            do {
                try await store.testConnection(settings)
                resultSucceeded = true
                resultMessage = "Connection succeeded. Click Connect to use this data source."
            } catch {
                resultSucceeded = false
                resultMessage = error.localizedDescription
            }
            operationPending = false
        }
    }

    private func connect() {
        let settings = draft
        operationPending = true
        resultMessage = nil
        Task { @MainActor in
            resultSucceeded = await store.connect(settings)
            resultMessage = resultSucceeded
                ? "Connected. This data source will be used next time you open the app."
                : store.connectionMessage ?? "Could not connect. Please try again."
            operationPending = false
        }
    }
}

@MainActor
struct ConnectionSettingsButton: View {
    @ObservedObject var store: TrackerStore
    @State private var showsSheet = false

    var body: some View {
        Group {
            if #available(macOS 14, *) {
                SettingsLink {
                    Label("Connection settings", systemImage: "gearshape")
                }
            } else {
                Button { showsSheet = true } label: {
                    Label("Connection settings", systemImage: "gearshape")
                }
                .sheet(isPresented: $showsSheet) {
                    ConnectionSettingsView(store: store, showsDoneButton: true)
                }
            }
        }
        .labelStyle(.iconOnly)
        .help("Choose Local or Server and change the server address.")
    }
}

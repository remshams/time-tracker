import AppKit
import SwiftUI
import TrackerClient

struct TrackerWindow: View {
    @ObservedObject var store: TrackerStore

    private var tabBinding: Binding<TaskTab> {
        Binding(get: { store.tab }, set: { store.changeTab($0) })
    }

    private var selectionBinding: Binding<String?> {
        Binding(get: { store.selectedTaskID }, set: { store.select($0) })
    }

    private var trackingFailurePresented: Binding<Bool> {
        Binding(get: { store.trackingError != nil }, set: {
            if !$0 { store.dismissTrackingError() }
        })
    }

    var body: some View {
        NavigationSplitView {
            VStack(spacing: 0) {
                Picker("Tasks", selection: tabBinding) {
                    ForEach(TaskTab.allCases) { tab in
                        Text(tab.rawValue).tag(tab)
                    }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .padding(12)

                List(selection: selectionBinding) {
                    ForEach(store.visibleTasks) { task in
                        Label {
                            Text(task.name)
                                .lineLimit(2)
                                .padding(.vertical, 4)
                        } icon: {
                            Image(systemName: store.active?.taskId == task.id
                                  ? "timer" : "checklist")
                        }
                        .tag(task.id)
                        .help(task.name)
                    }
                }
                .listStyle(.sidebar)
                .overlay {
                    if store.visibleTasks.isEmpty {
                        VStack(spacing: 8) {
                            Image(systemName: "tray")
                            Text("No \(store.tab.rawValue.lowercased()) tasks")
                        }
                        .foregroundStyle(.secondary)
                    }
                }
            }
            .navigationTitle("Tasks")
            .navigationSplitViewColumnWidth(min: 240, ideal: 280, max: 400)
        } detail: {
            VStack(spacing: 0) {
                ConnectionSummary(store: store)
                Divider()
                TimerSummary(store: store)
                Divider()
                TaskDetails(store: store)
            }
        }
        .frame(minWidth: 760, minHeight: 480)
        .alert("Could not change tracking", isPresented: trackingFailurePresented) {
            Button("OK", role: .cancel) { store.dismissTrackingError() }
        } message: {
            Text(store.trackingError ?? "Please try again.")
        }
    }
}

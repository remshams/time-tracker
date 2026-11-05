import SwiftUI
import TrackerClient

struct TrackingSettingsSection: View {
    let store: TrackerStore
    @Environment(TrackingAutomationStore.self) private var automation

    var body: some View {
        Section("Tracking") {
            Toggle(
                "Automatically pause tracking when the screen is locked",
                isOn: Binding(
                    get: { automation.enabled },
                    set: { store.setPauseOnScreenLock($0) }
                )
            )
            .accessibilityIdentifier("tracking.pause-on-lock")
            Text("Resume the same task when you unlock this Mac, unless another timer is already running.")
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            if let status = automation.statusText {
                Text(status)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}

import QtQuick
import "../../Shared"

// Placeholder until the feature lands.
TrackerDialog {
    id: root
    title: "SettingsDialog"
    function openSettings() {
        root.open()
    }
    onAccepted: root.close()
}

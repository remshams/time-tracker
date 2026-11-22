import QtQuick
import "../../Shared"

// Placeholder until the feature lands.
TrackerDialog {
    id: root
    title: "TaskCreationDialog"
    function openNew() {
        root.open()
    }
    onAccepted: root.close()
}

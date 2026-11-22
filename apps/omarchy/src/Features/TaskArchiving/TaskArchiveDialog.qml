import QtQuick
import "../../Shared"

// Placeholder until the feature lands.
TrackerDialog {
    id: root
    title: "TaskArchiveDialog"
    function openFor(taskId) {
        root.open()
    }
    onAccepted: root.close()
}

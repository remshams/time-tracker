import QtQuick
import "../../Shared"

// Placeholder until the feature lands.
TrackerDialog {
    id: root
    title: "TaskRenameDialog"
    function openFor(taskId) {
        root.open()
    }
    onAccepted: root.close()
}

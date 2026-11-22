import QtQuick
import "../../Shared"

// Placeholder until the feature lands.
TrackerDialog {
    id: root
    title: "WorklogMoveDialog"
    function openFor(worklogId) {
        root.open()
    }
    onAccepted: root.close()
}

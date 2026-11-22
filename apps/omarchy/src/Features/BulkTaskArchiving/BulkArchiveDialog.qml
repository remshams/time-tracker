import QtQuick
import "../../Shared"

// Placeholder until the feature lands.
TrackerDialog {
    id: root
    title: "BulkArchiveDialog"
    function openPreview() {
        root.open()
    }
    onAccepted: root.close()
}

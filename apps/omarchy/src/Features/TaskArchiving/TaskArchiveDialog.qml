import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts
import "../../Shared"

// Confirms archiving a task. Unarchiving runs at once, so this dialog only shows its failure.
TrackerDialog {
    id: root

    function openFor(taskId) {
        taskArchiving.begin(taskId)
    }

    title: taskArchiving.unarchive ? qsTr("Unarchive task") : qsTr("Archive task")
    primaryText: taskArchiving.unarchive ? qsTr("Unarchive") : qsTr("Archive")
    primaryEnabled: taskArchiving.canSubmit
    destructive: !taskArchiving.unarchive
    busy: taskArchiving.pending
    error: taskArchiving.error

    onAccepted: taskArchiving.submit()

    Connections {
        target: taskArchiving
        function onDialogRequested() {
            root.open()
        }
        function onSucceeded() {
            root.close()
        }
    }

    Txt {
        Layout.fillWidth: true
        text: taskArchiving.taskName
        font.bold: true
        wrapMode: Text.Wrap
        elide: Text.ElideNone
    }
    Txt {
        Layout.fillWidth: true
        // A wrapping text reports its unwrapped width, which would widen the dialog.
        Layout.preferredWidth: root.dialogWidth - 2 * Tokens.spacing.xLarge
        text: taskArchiving.unarchive ? qsTr("The task returns to Active. Its worklog history stays available.") : qsTr("The task moves to Archived. Its worklog history stays available.")
        level: 1
        wrapMode: Text.Wrap
        elide: Text.ElideNone
    }
}

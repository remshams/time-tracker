import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts
import "../../Shared"

// Renames the task that was clicked. The draft stays while the list refreshes behind it.
TrackerDialog {
    id: root

    function openFor(taskId) {
        if (taskRename.begin(taskId))
            root.open()
    }

    title: qsTr("Rename task")
    primaryText: qsTr("Rename")
    primaryEnabled: taskRename.canSubmit
    busy: taskRename.pending
    error: taskRename.error

    onOpened: {
        field.forceActiveFocus()
        // Typing replaces the old name; the text binding lands after the dialog opens.
        Qt.callLater(field.selectAll)
    }
    onAccepted: taskRename.submit()

    Connections {
        target: taskRename
        function onSucceeded() {
            root.close()
        }
    }

    Txt {
        Layout.fillWidth: true
        text: qsTr("Current name: %1").arg(taskRename.originalName)
        level: 1
    }
    TextField {
        id: field
        Layout.fillWidth: true
        Layout.preferredWidth: root.dialogWidth - 2 * Tokens.spacing.xLarge
        placeholderText: qsTr("Task name")
        font.family: theme.fontFamily
        text: taskRename.draft
        readOnly: taskRename.pending
        onTextEdited: taskRename.draft = text
        Material.accent: theme.accent
        Material.foreground: theme.text
        Material.containerStyle: Material.Outlined
        Accessible.name: qsTr("Task name")
    }
}

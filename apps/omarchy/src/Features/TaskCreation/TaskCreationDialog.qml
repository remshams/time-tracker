import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts
import "../../Shared"

// Asks for a name and creates the task. The new task is selected once the refresh delivers it.
TrackerDialog {
    id: root

    function openNew() {
        taskCreation.begin()
        root.open()
    }

    title: qsTr("New task")
    primaryText: qsTr("Create")
    primaryEnabled: taskCreation.canSubmit
    busy: taskCreation.pending
    error: taskCreation.error

    onOpened: field.forceActiveFocus()
    onAccepted: taskCreation.submit()

    Connections {
        target: taskCreation
        function onSucceeded() {
            root.close()
        }
    }

    TextField {
        id: field
        Layout.fillWidth: true
        Layout.preferredWidth: root.dialogWidth - 2 * Tokens.spacing.xLarge
        placeholderText: qsTr("Task name")
        font.family: theme.fontFamily
        text: taskCreation.draft
        readOnly: taskCreation.pending
        onTextEdited: taskCreation.draft = text
        Material.accent: theme.accent
        Material.foreground: theme.text
        Material.containerStyle: Material.Outlined
        Accessible.name: qsTr("Task name")
    }
}

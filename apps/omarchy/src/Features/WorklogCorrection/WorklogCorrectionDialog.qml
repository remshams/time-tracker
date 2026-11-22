import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts
import "../../Shared"

// Edits the start and end of one worklog as local date and time text. The store keeps
// the draft, so an error leaves the fields as they were.
TrackerDialog {
    id: root

    // One date or time input. Return reaches the dialog, which saves.
    component DraftField: TextField {
        Layout.fillWidth: true
        Layout.preferredWidth: 1
        font.family: theme.fontFamily
        font.pixelSize: 13
        selectByMouse: true
        inputMethodHints: Qt.ImhPreferNumbers
        Material.accent: theme.accent
        Material.foreground: theme.text
        Material.containerStyle: Material.Outlined
        // Tab lands on a selected field, so typing replaces the old value.
        onActiveFocusChanged: if (activeFocus && focusReason !== Qt.MouseFocusReason) selectAll()
    }

    function openFor(worklogId) {
        if (worklogCorrection.begin(worklogId))
            root.open()
    }

    title: qsTr("Edit times")
    primaryText: qsTr("Save")
    primaryEnabled: worklogCorrection.canSave
    busy: worklogCorrection.pending
    error: worklogCorrection.error

    onAccepted: worklogCorrection.save()
    onCancelled: worklogCorrection.cancel()
    onOpened: startDate.forceActiveFocus()

    Connections {
        target: worklogCorrection
        function onSaved() {
            root.close()
        }
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: 8
        TaskDot {
            dotColor: theme.taskColor(catalog.colorNameOf(worklogCorrection.taskId))
        }
        Txt {
            Layout.fillWidth: true
            text: catalog.nameOf(worklogCorrection.taskId)
            font.bold: true
        }
    }

    GridLayout {
        Layout.fillWidth: true
        columns: 3
        columnSpacing: 10
        rowSpacing: 8

        Txt {
            text: qsTr("Start")
            level: 1
            Layout.preferredWidth: 40
        }
        DraftField {
            id: startDate
            text: worklogCorrection.startDate
            placeholderText: qsTr("YYYY-MM-DD")
            maximumLength: 10
            onTextEdited: worklogCorrection.startDate = text
            Accessible.name: qsTr("Start date")
        }
        DraftField {
            text: worklogCorrection.startTime
            placeholderText: qsTr("HH:MM")
            maximumLength: 5
            onTextEdited: worklogCorrection.startTime = text
            Accessible.name: qsTr("Start time")
        }

        Txt {
            visible: !worklogCorrection.running
            text: qsTr("End")
            level: 1
            Layout.preferredWidth: 40
        }
        DraftField {
            visible: !worklogCorrection.running
            text: worklogCorrection.endDate
            placeholderText: qsTr("YYYY-MM-DD")
            maximumLength: 10
            onTextEdited: worklogCorrection.endDate = text
            Accessible.name: qsTr("End date")
        }
        DraftField {
            visible: !worklogCorrection.running
            text: worklogCorrection.endTime
            placeholderText: qsTr("HH:MM")
            maximumLength: 5
            onTextEdited: worklogCorrection.endTime = text
            Accessible.name: qsTr("End time")
        }
    }

    Txt {
        Layout.fillWidth: true
        visible: worklogCorrection.running
        text: qsTr("This worklog is still running. It keeps running after you save.")
        level: 1
        wrapMode: Text.Wrap
        elide: Text.ElideNone
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: 8
        Txt {
            text: worklogCorrection.running ? qsTr("Running for") : qsTr("Duration")
            level: 1
        }
        Duration {
            visible: worklogCorrection.validationMessage.length === 0 && worklogCorrection.durationSeconds >= 0
            seconds: worklogCorrection.durationSeconds
            bold: true
        }
        Txt {
            visible: worklogCorrection.validationMessage.length === 0 && worklogCorrection.durationSeconds < 0
            text: qsTr("The end is before the start")
            level: 1
        }
        Item {
            Layout.fillWidth: true
        }
    }

    Txt {
        Layout.fillWidth: true
        visible: worklogCorrection.validationMessage.length > 0
        text: worklogCorrection.validationMessage
        level: 1
        wrapMode: Text.Wrap
        elide: Text.ElideNone
    }

    Txt {
        Layout.fillWidth: true
        text: qsTr("Times are in %1.").arg(worklogCorrection.timeZone)
        level: 2
        font.pixelSize: 12
    }
}

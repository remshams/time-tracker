import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../../Shared"

// Asks before a completed worklog is deleted for good. Escape cancels.
TrackerDialog {
    id: root

    function openFor(worklogId) {
        if (worklogDeletion.begin(worklogId))
            root.open()
    }

    title: qsTr("Delete worklog")
    primaryText: qsTr("Delete")
    primaryEnabled: worklogDeletion.canDelete
    destructive: true
    busy: worklogDeletion.pending
    error: worklogDeletion.error

    onAccepted: worklogDeletion.confirm()
    onCancelled: worklogDeletion.cancel()

    Connections {
        target: worklogDeletion
        function onDeleted() {
            root.close()
        }
    }

    Txt {
        Layout.fillWidth: true
        text: qsTr("This removes the entry permanently.")
        level: 1
        wrapMode: Text.Wrap
        elide: Text.ElideNone
    }

    GridLayout {
        Layout.fillWidth: true
        columns: 2
        columnSpacing: 16
        rowSpacing: 6

        Txt {
            text: qsTr("Task")
            level: 1
        }
        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            TaskDot {
                dotColor: theme.taskColor(catalog.colorNameOf(worklogDeletion.taskId))
            }
            Txt {
                Layout.fillWidth: true
                text: catalog.nameOf(worklogDeletion.taskId)
                font.bold: true
            }
        }

        Txt {
            text: qsTr("Day")
            level: 1
        }
        Txt {
            Layout.fillWidth: true
            text: worklogDeletion.dayLabel
        }

        Txt {
            text: qsTr("Time")
            level: 1
        }
        Txt {
            Layout.fillWidth: true
            text: qsTr("%1 to %2").arg(worklogDeletion.startLabel).arg(worklogDeletion.endLabel)
        }

        Txt {
            text: qsTr("Duration")
            level: 1
        }
        Duration {
            seconds: worklogDeletion.durationSeconds
            bold: true
        }
    }
}

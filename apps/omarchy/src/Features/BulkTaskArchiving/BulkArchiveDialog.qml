import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts
import "../../Shared"

// Archives every task that has been inactive for a number of days, after showing which ones.
TrackerDialog {
    id: root

    readonly property bool done: bulkArchiving.archivedCount >= 0

    function openPreview() {
        bulkArchiving.begin()
        root.open()
    }

    title: qsTr("Archive inactive tasks")
    dialogWidth: Tokens.size.wideDialogWidth
    primaryText: root.done ? qsTr("Close") : qsTr("Archive")
    primaryEnabled: root.done || bulkArchiving.canSubmit
    destructive: !root.done
    busy: bulkArchiving.pending
    error: bulkArchiving.error

    onOpened: days.forceActiveFocus()
    onAccepted: root.done ? root.close() : bulkArchiving.submit()
    onCancelled: bulkArchiving.cancel()

    Txt {
        Layout.fillWidth: true
        // A wrapping text reports its unwrapped width, which would widen the dialog.
        Layout.preferredWidth: root.dialogWidth - 2 * Tokens.spacing.xLarge
        text: qsTr("Archives tasks with no recent work or changes. Running tasks are never included. Worklog history stays available.")
        level: 1
        wrapMode: Text.Wrap
        elide: Text.ElideNone
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: Tokens.spacing.small

        Txt {
            text: qsTr("Inactive for more than")
        }
        TextField {
            id: days
            Layout.preferredWidth: Tokens.size.daysFieldWidth
            font.family: theme.fontFamily
            text: bulkArchiving.daysText
            readOnly: bulkArchiving.pending || root.done
            inputMethodHints: Qt.ImhDigitsOnly
            validator: RegularExpressionValidator {
                regularExpression: /[0-9]{0,6}/
            }
            onTextEdited: bulkArchiving.daysText = text
            Material.accent: theme.accent
            Material.foreground: theme.text
            Material.containerStyle: Material.Outlined
            Accessible.name: qsTr("Inactivity period in days")
        }
        Txt {
            text: qsTr("days")
        }
        Item {
            Layout.fillWidth: true
        }
        Button {
            text: qsTr("Reload")
            flat: true
            enabled: !bulkArchiving.pending && !bulkArchiving.loading && !root.done
            font.family: theme.fontFamily
            Material.foreground: theme.textDim
            onClicked: bulkArchiving.refresh()
            Accessible.name: qsTr("Reload the preview")
        }
    }

    Txt {
        Layout.fillWidth: true
        font.bold: true
        text: {
            if (root.done)
                return bulkArchiving.archivedCount === 1 ? qsTr("Archived 1 task.") : qsTr("Archived %1 tasks.").arg(bulkArchiving.archivedCount)
            if (bulkArchiving.loading)
                return qsTr("Loading the preview")
            if (!bulkArchiving.previewLoaded)
                return qsTr("No preview")
            const count = bulkArchiving.tasks.length
            return count === 1 ? qsTr("1 task to archive") : qsTr("%1 tasks to archive").arg(count)
        }
        Accessible.role: Accessible.StaticText
        Accessible.name: text
    }

    Rectangle {
        Layout.fillWidth: true
        Layout.preferredHeight: Tokens.size.previewListHeight
        visible: !root.done
        color: theme.panel
        radius: theme.radius

        ListView {
            id: list
            anchors.fill: parent
            anchors.margins: Tokens.spacing.small
            clip: true
            model: bulkArchiving.tasks
            boundsBehavior: Flickable.StopAtBounds
            ScrollBar.vertical: ScrollBar {}

            Accessible.role: Accessible.List
            Accessible.name: qsTr("Tasks to archive")

            delegate: Row {
                required property var modelData
                width: ListView.view.width
                height: Tokens.size.previewRowHeight
                spacing: Tokens.spacing.medium

                Accessible.role: Accessible.ListItem
                Accessible.name: modelData.name

                TaskDot {
                    anchors.verticalCenter: parent.verticalCenter
                    dotColor: theme.taskColor(parent.modelData.colorName)
                }
                Txt {
                    width: parent.width - Tokens.size.dot - Tokens.spacing.medium
                    height: parent.height
                    text: parent.modelData.name
                }
            }
        }
        Txt {
            anchors.centerIn: parent
            visible: bulkArchiving.previewLoaded && list.count === 0
            text: qsTr("No tasks match this period.")
            level: 1
        }
    }
}

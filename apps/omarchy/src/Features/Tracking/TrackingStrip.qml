import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../../Shared"

// The top of the window: what runs now and for how long. Clicking it stops the running
// task, or starts the selected one while idle.
Rectangle {
    id: root

    readonly property string shownTaskId: tracking.running ? tracking.runningTaskId : catalog.selectedArchived ? "" : catalog.selectedTaskId
    readonly property color taskColor: shownTaskId.length > 0 ? theme.taskColor(catalog.colorNameOf(shownTaskId)) : theme.textFaint
    readonly property bool canAct: tracking.available && shownTaskId.length > 0

    signal showTask(string taskId)

    implicitHeight: 64
    color: theme.panel

    Accessible.role: Accessible.Button
    Accessible.name: {
        if (!tracking.known)
            return qsTr("Tracking unavailable")
        if (tracking.running)
            return qsTr("Stop %1, running for %2").arg((catalog.names[tracking.runningTaskId] ?? "")).arg(format.duration(tracking.elapsedSeconds))
        if (shownTaskId.length > 0)
            return qsTr("Start %1").arg((catalog.names[shownTaskId] ?? ""))
        return qsTr("Nothing running")
    }
    Accessible.onPressAction: if (root.canAct) tracking.toggle(root.shownTaskId)

    // The running task's color runs down the left edge, like a focused window border.
    Rectangle {
        width: 4
        height: parent.height
        color: root.taskColor
        opacity: tracking.running ? 1 : 0.35
    }

    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        cursorShape: root.canAct ? Qt.PointingHandCursor : Qt.ArrowCursor
        onClicked: event => {
            if (event.button === Qt.RightButton) {
                if (root.shownTaskId.length > 0)
                    stripMenu.popup()
                return
            }
            if (root.canAct)
                tracking.toggle(root.shownTaskId)
        }
    }

    Menu {
        id: stripMenu
        MenuItem {
            text: qsTr("Show task")
            onTriggered: root.showTask(root.shownTaskId)
        }
        MenuItem {
            text: tracking.running ? qsTr("Stop tracking") : qsTr("Start tracking")
            enabled: root.canAct
            onTriggered: tracking.toggle(root.shownTaskId)
        }
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 20
        anchors.rightMargin: 20
        spacing: 14

        Txt {
            Layout.preferredWidth: 22
            horizontalAlignment: Text.AlignHCenter
            text: !tracking.known ? "" : tracking.stale ? "!" : tracking.running && mouse.containsMouse ? "\u25A0" : "\u25B6"
            font.pixelSize: 18
            color: tracking.running ? root.taskColor : theme.textDim
            opacity: root.canAct || tracking.stale ? 1 : 0.4
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 2

            Txt {
                Layout.fillWidth: true
                text: {
                    if (!tracking.known)
                        return connection.status === "connecting" ? qsTr("Connecting") : qsTr("Unavailable")
                    if (tracking.running)
                        return (catalog.names[tracking.runningTaskId] ?? "")
                    if (root.shownTaskId.length > 0)
                        return (catalog.names[root.shownTaskId] ?? "")
                    return catalog.activeCount === 0 ? qsTr("No tasks yet") : qsTr("Nothing running")
                }
                font.pixelSize: 16
                font.bold: tracking.running
            }
            Txt {
                Layout.fillWidth: true
                font.pixelSize: 11
                level: tracking.stale ? 0 : 1
                color: tracking.stale ? theme.danger : theme.textDim
                text: {
                    if (tracking.stale)
                        return qsTr("Last confirmed state. The connection is unavailable.")
                    if (tracking.error.length > 0)
                        return tracking.error
                    if (tracking.running)
                        return qsTr("Running since %1. Space stops it.").arg(format.clockTime(tracking.runningSince))
                    if (root.shownTaskId.length > 0)
                        return qsTr("Idle. Space starts the selected task.")
                    return ""
                }
            }
        }

        Duration {
            visible: tracking.running
            seconds: tracking.elapsedSeconds
            pixelSize: 34
            bold: true
            color: theme.text
        }
        Txt {
            visible: !tracking.running && root.shownTaskId.length > 0 && tracking.known
            text: qsTr("Start")
            font.pixelSize: 16
            font.bold: true
            color: root.canAct ? theme.accent : theme.textFaint
        }

        Rectangle {
            Layout.preferredWidth: 1
            Layout.fillHeight: true
            Layout.topMargin: 14
            Layout.bottomMargin: 14
            color: theme.line
        }

        ColumnLayout {
            spacing: 2
            Txt {
                Layout.alignment: Qt.AlignRight
                text: qsTr("Today")
                font.pixelSize: 11
                level: 1
            }
            Duration {
                Layout.alignment: Qt.AlignRight
                seconds: totals.totalSeconds
                pixelSize: 16
                color: totals.known ? theme.text : theme.textFaint
            }
        }
    }
}

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../Shared"

// The top of the window: what runs now and for how long. Clicking it stops the running
// task, or starts the selected one while idle.
Rectangle {
    id: root

    readonly property string shownTaskId: tracking.running ? tracking.runningTaskId : catalog.selectedArchived ? "" : catalog.selectedTaskId
    readonly property color taskColor: shownTaskId.length > 0 ? theme.taskColor(catalog.colorNameOf(shownTaskId)) : theme.textFaint
    readonly property bool canAct: tracking.available && shownTaskId.length > 0

    signal showTask(string taskId)

    implicitHeight: Tokens.size.stripHeight
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
        width: Tokens.border.stripe
        height: parent.height
        color: root.taskColor
        opacity: tracking.running ? 1 : Tokens.emphasis.idleStripe
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
        anchors.leftMargin: Tokens.spacing.xLarge
        anchors.rightMargin: Tokens.spacing.xLarge
        spacing: Tokens.spacing.medium

        Txt {
            Layout.preferredWidth: Tokens.size.glyphSlot
            horizontalAlignment: Text.AlignHCenter
            text: !tracking.known ? "" : tracking.stale ? "!" : tracking.running && mouse.containsMouse ? "\u25A0" : "\u25B6"
            font.pixelSize: Tokens.text.glyph
            color: tracking.running ? root.taskColor : theme.textDim
            opacity: root.canAct || tracking.stale ? 1 : Tokens.emphasis.disabled
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: Tokens.spacing.xxSmall

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
                font.pixelSize: Tokens.text.title
                font.bold: tracking.running
            }
            Txt {
                Layout.fillWidth: true
                font.pixelSize: Tokens.text.caption
                color: tracking.stale ? theme.danger : theme.textDim
                text: {
                    if (tracking.stale)
                        return qsTr("Last confirmed state. The connection is unavailable.")
                    if (tracking.error.length > 0)
                        return tracking.error
                    if (tracking.running) {
                        const since = format.clockTime(tracking.runningSince)
                        // Space acts on the selected task, so the hint follows the selection.
                        if (catalog.selectedTaskId === tracking.runningTaskId)
                            return qsTr("Running since %1. Space stops it.").arg(since)
                        if (catalog.selectedArchived || catalog.selectedTaskId.length === 0)
                            return qsTr("Running since %1. Press s to stop it.").arg(since)
                        return qsTr("Running since %1. Space switches to the selected task.").arg(since)
                    }
                    if (root.shownTaskId.length > 0)
                        return qsTr("Idle. Space starts the selected task.")
                    return ""
                }
            }
        }

        Duration {
            visible: tracking.running
            seconds: tracking.elapsedSeconds
            pixelSize: Tokens.text.display
            bold: true
        }
        Txt {
            visible: !tracking.running && root.shownTaskId.length > 0 && tracking.known
            text: qsTr("Start")
            font.pixelSize: Tokens.text.title
            font.bold: true
            color: root.canAct ? theme.accent : theme.textFaint
        }

        Rectangle {
            Layout.preferredWidth: Tokens.border.hairline
            Layout.fillHeight: true
            Layout.topMargin: Tokens.spacing.medium
            Layout.bottomMargin: Tokens.spacing.medium
            color: theme.line
        }

        ColumnLayout {
            spacing: Tokens.spacing.xxSmall
            Txt {
                Layout.alignment: Qt.AlignRight
                // Cached totals come from an earlier report because the latest one failed.
                text: totals.cached ? qsTr("Today, cached") : qsTr("Today")
                font.pixelSize: Tokens.text.caption
                level: 1
            }
            Duration {
                Layout.alignment: Qt.AlignRight
                visible: totals.known
                seconds: totals.totalSeconds
                pixelSize: Tokens.text.title
                color: totals.cached ? theme.textDim : theme.text
            }
            Txt {
                Layout.alignment: Qt.AlignRight
                visible: !totals.known
                text: qsTr("Unavailable")
                font.pixelSize: Tokens.text.title
                level: 2
            }
        }
    }
}

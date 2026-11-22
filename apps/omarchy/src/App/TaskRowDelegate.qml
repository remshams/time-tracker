import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../Shared"

// One sidebar row: color dot, name, when it last ran and today's time.
Item {
    id: row

    required property string taskId
    required property string name
    required property string colorName
    required property string lastWorked
    required property bool archived
    property bool focusedList: true

    readonly property bool selected: catalog.selectedTaskId === taskId
    readonly property bool running: tracking.runningTaskId === taskId
    readonly property color taskColor: theme.taskColor(colorName)
    readonly property var todaySeconds: {
        totals.totalSeconds // Re-evaluates on every totals change.
        return totals.secondsFor(taskId)
    }
    readonly property bool hasTime: todaySeconds > 0

    signal renameRequested(string id)
    signal archiveRequested(string id)

    implicitHeight: Tokens.size.taskRowHeight

    Accessible.role: Accessible.ListItem
    Accessible.name: {
        let text = name
        if (running)
            text += ", " + qsTr("running")
        if (hasTime)
            text += ", " + qsTr("%1 today").arg(format.duration(todaySeconds))
        return text
    }
    Accessible.selected: selected
    Accessible.onPressAction: catalog.select(taskId)

    Rectangle {
        anchors.fill: parent
        anchors.leftMargin: Tokens.spacing.small
        anchors.rightMargin: Tokens.spacing.small
        radius: Math.min(Tokens.radius.small, theme.radius)
        color: row.selected ? theme.selection : mouse.containsMouse ? Qt.alpha(theme.selection, Tokens.emphasis.hover) : "transparent"
        opacity: row.selected && !row.focusedList ? Tokens.emphasis.unfocusedSelection : 1
    }
    Rectangle {
        visible: row.selected
        x: Tokens.spacing.small
        width: Tokens.border.focus
        height: parent.height
        color: row.taskColor
    }

    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        onClicked: event => {
            catalog.select(row.taskId)
            if (event.button === Qt.RightButton)
                menu.popup()
        }
        onDoubleClicked: if (!row.archived && tracking.available) tracking.start(row.taskId)
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: Tokens.spacing.large
        anchors.rightMargin: Tokens.spacing.large
        spacing: Tokens.spacing.medium

        TaskDot {
            dotColor: row.taskColor
            filled: row.hasTime || row.running
            size: row.running ? Tokens.size.runningDot : Tokens.size.dot
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: Tokens.spacing.xxSmall
            Txt {
                Layout.fillWidth: true
                text: row.name
                font.bold: row.running
                level: row.archived ? 1 : 0
            }
            Txt {
                Layout.fillWidth: true
                text: row.running ? qsTr("Running since %1").arg(format.clockTime(tracking.runningSince)) : row.lastWorked
                font.pixelSize: Tokens.text.caption
                color: row.running ? row.taskColor : theme.textFaint
            }
        }

        // Starts the task from the row, shown on hover, like the macOS sidebar.
        Txt {
            visible: mouse.containsMouse && !row.running && !row.archived && tracking.available
            text: "▶"
            color: row.taskColor
            Accessible.role: Accessible.Button
            Accessible.name: qsTr("Start %1").arg(row.name)
            MouseArea {
                anchors.fill: parent
                anchors.margins: -Tokens.spacing.small
                cursorShape: Qt.PointingHandCursor
                onClicked: tracking.start(row.taskId)
            }
        }

        Duration {
            // Without a report for today the time is unknown, so the row shows none.
            visible: row.hasTime || (row.running && row.todaySeconds >= 0)
            seconds: Math.max(0, row.todaySeconds)
            color: row.running ? theme.text : theme.textDim
        }
    }

    Menu {
        id: menu
        MenuItem {
            text: row.running ? qsTr("Stop tracking") : qsTr("Start tracking")
            enabled: tracking.available && !row.archived
            onTriggered: row.running ? tracking.stop() : tracking.start(row.taskId)
        }
        MenuItem {
            text: qsTr("Rename")
            onTriggered: row.renameRequested(row.taskId)
        }
        MenuItem {
            text: row.archived ? qsTr("Unarchive") : qsTr("Archive")
            onTriggered: row.archiveRequested(row.taskId)
        }
    }
}

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../../Shared"

// One worklog: its times, a bar on the day's hour axis, and its duration.
Item {
    id: row

    required property var modelData
    required property int labelWidth
    required property int durationWidth
    property color barColor: theme.accent
    property bool selected: false
    property bool focusedList: false
    readonly property bool running: modelData.running
    // The running part ticks with the tracking timer; the model itself rebuilds once a minute.
    readonly property bool live: running && tracking.runningWorklogId === modelData.worklogId
    readonly property var seconds: live ? Math.max(0, tracking.elapsedSeconds - modelData.runningOffset) : modelData.seconds

    signal clicked
    signal editTimesRequested
    signal moveRequested
    signal deleteRequested

    height: Tokens.size.worklogRowHeight

    Accessible.role: Accessible.ListItem
    Accessible.name: qsTr("%1 to %2, %3").arg(modelData.startLabel).arg(modelData.endLabel).arg(format.duration(row.seconds))
    Accessible.selected: selected
    Accessible.onPressAction: {
        row.clicked()
        row.editTimesRequested()
    }

    Rectangle {
        anchors.fill: parent
        anchors.leftMargin: Tokens.spacing.xLarge
        anchors.rightMargin: Tokens.spacing.medium
        radius: Math.min(Tokens.radius.small, theme.radius)
        color: row.selected ? theme.selection : mouse.containsMouse ? Qt.alpha(theme.selection, Tokens.emphasis.hover) : "transparent"
        opacity: row.selected && !row.focusedList ? Tokens.emphasis.unfocusedSelection : 1
    }

    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        onClicked: event => {
            row.clicked()
            if (event.button === Qt.RightButton)
                menu.popup()
        }
        onDoubleClicked: row.editTimesRequested()
    }

    Txt {
        x: Tokens.spacing.xxLarge
        width: row.labelWidth - 12
        height: parent.height
        text: row.modelData.startLabel + " – " + row.modelData.endLabel
        color: row.running ? row.barColor : theme.text
        font.bold: row.running
    }

    // The track spans the hour axis; the bar marks this worklog's part of the day.
    Item {
        id: track
        x: Tokens.spacing.xxLarge + row.labelWidth
        width: row.width - x - 24 - row.durationWidth
        height: parent.height

        Rectangle {
            anchors.verticalCenter: parent.verticalCenter
            width: parent.width
            height: Tokens.size.barHeight
            radius: Math.min(Tokens.radius.small, theme.radius)
            color: theme.line
            opacity: Tokens.emphasis.track
        }
        Rectangle {
            id: bar
            anchors.verticalCenter: parent.verticalCenter
            x: track.width * row.modelData.from
            width: Math.max(3, track.width * (row.modelData.to - row.modelData.from))
            height: Tokens.size.barHeight
            radius: Math.min(Tokens.radius.small, theme.radius)
            color: row.barColor
            opacity: row.running ? Tokens.emphasis.runningBar : 1
        }
        Rectangle {
            visible: row.running
            anchors.verticalCenter: parent.verticalCenter
            x: bar.x + bar.width
            width: Tokens.size.cursorWidth
            height: Tokens.size.cursorHeight
            color: row.barColor
            SequentialAnimation on opacity {
                running: row.running
                loops: Animation.Infinite
                NumberAnimation {
                    to: 0
                    duration: Tokens.motion.blink
                    easing.type: Easing.InOutQuad
                }
                NumberAnimation {
                    to: 1
                    duration: Tokens.motion.blink
                    easing.type: Easing.InOutQuad
                }
            }
        }
    }

    Duration {
        anchors.right: parent.right
        anchors.rightMargin: Tokens.spacing.xLarge
        anchors.verticalCenter: parent.verticalCenter
        seconds: row.seconds
        color: row.running ? row.barColor : theme.text
        bold: row.running
    }

    Menu {
        id: menu
        MenuItem {
            text: qsTr("Edit times")
            onTriggered: row.editTimesRequested()
        }
        MenuItem {
            text: qsTr("Move to task")
            onTriggered: row.moveRequested()
        }
        MenuItem {
            text: qsTr("Delete")
            enabled: !row.modelData.worklogRunning
            onTriggered: row.deleteRequested()
        }
    }
}

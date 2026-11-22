import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../../Shared"

// One worklog: its times, a bar on the day's hour axis, and its duration.
Item {
    id: row

    required property var modelData
    property int labelWidth: 132
    property int durationWidth: 92
    property color barColor: theme.accent
    property bool selected: false
    property bool focusedList: false
    readonly property bool running: modelData.running

    signal clicked
    signal editTimesRequested
    signal moveRequested
    signal deleteRequested

    height: 32

    Accessible.role: Accessible.ListItem
    Accessible.name: qsTr("%1 to %2, %3").arg(modelData.startLabel).arg(modelData.endLabel).arg(format.duration(modelData.seconds))
    Accessible.selected: selected
    Accessible.onPressAction: row.editTimesRequested()

    Rectangle {
        anchors.fill: parent
        anchors.leftMargin: 20
        anchors.rightMargin: 12
        radius: Math.min(4, theme.radius)
        color: row.selected ? theme.selection : mouse.containsMouse ? Qt.alpha(theme.selection, 0.45) : "transparent"
        opacity: row.selected && !row.focusedList ? 0.6 : 1
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
        x: 32
        width: row.labelWidth - 12
        height: parent.height
        text: row.modelData.startLabel + " – " + row.modelData.endLabel
        color: row.running ? row.barColor : theme.text
        font.bold: row.running
    }

    // The track spans the hour axis; the bar marks this worklog's part of the day.
    Item {
        id: track
        x: 32 + row.labelWidth
        width: row.width - x - 24 - row.durationWidth
        height: parent.height

        Rectangle {
            anchors.verticalCenter: parent.verticalCenter
            width: parent.width
            height: 6
            radius: Math.min(3, theme.radius)
            color: theme.line
            opacity: 0.35
        }
        Rectangle {
            id: bar
            anchors.verticalCenter: parent.verticalCenter
            x: track.width * row.modelData.from
            width: Math.max(3, track.width * (row.modelData.to - row.modelData.from))
            height: 6
            radius: Math.min(3, theme.radius)
            color: row.barColor
            opacity: row.running ? 0.75 : 1
        }
        Rectangle {
            visible: row.running
            anchors.verticalCenter: parent.verticalCenter
            x: bar.x + bar.width
            width: 7
            height: 16
            color: row.barColor
            SequentialAnimation on opacity {
                running: row.running
                loops: Animation.Infinite
                NumberAnimation {
                    to: 0
                    duration: 600
                    easing.type: Easing.InOutQuad
                }
                NumberAnimation {
                    to: 1
                    duration: 600
                    easing.type: Easing.InOutQuad
                }
            }
        }
    }

    Duration {
        anchors.right: parent.right
        anchors.rightMargin: 24
        anchors.verticalCenter: parent.verticalCenter
        seconds: row.modelData.seconds
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
            enabled: !row.running
            onTriggered: row.deleteRequested()
        }
    }
}

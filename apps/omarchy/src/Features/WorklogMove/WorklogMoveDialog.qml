import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts
import "../../Shared"

// Moves one worklog to another task. The search field keeps the keyboard: typing
// narrows the list, Up and Down choose, Return moves and Escape cancels.
TrackerDialog {
    id: root

    function openFor(worklogId) {
        if (worklogMove.begin(worklogId))
            root.open()
    }

    title: qsTr("Move to task")
    primaryText: qsTr("Move")
    primaryEnabled: worklogMove.canMove
    busy: worklogMove.pending
    error: worklogMove.error

    onAccepted: worklogMove.move()
    onCancelled: worklogMove.cancel()
    onOpened: search.forceActiveFocus()

    Connections {
        target: worklogMove
        function onMoved() {
            root.close()
        }
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: Tokens.spacing.small
        TaskDot {
            dotColor: theme.taskColor(catalog.colorNameOf(worklogMove.sourceTaskId))
        }
        Txt {
            Layout.fillWidth: true
            text: worklogMove.running ? qsTr("%1, %2 to now").arg((catalog.names[worklogMove.sourceTaskId] ?? "")).arg(format.clockTime(worklogMove.start)) : qsTr("%1, %2 to %3").arg((catalog.names[worklogMove.sourceTaskId] ?? "")).arg(format.clockTime(worklogMove.start)).arg(format.clockTime(worklogMove.end))
            font.bold: true
        }
    }

    TextField {
        id: search
        Layout.fillWidth: true
        text: worklogMove.query
        placeholderText: qsTr("Search tasks")
        font.family: theme.fontFamily
        font.pixelSize: Tokens.text.body
        selectByMouse: true
        Material.accent: theme.accent
        Material.foreground: theme.text
        Material.containerStyle: Material.Outlined
        // The field has the keyboard, so Up and Down choose the destination from here.
        Keys.onUpPressed: worklogMove.moveSelection(-1)
        Keys.onDownPressed: worklogMove.moveSelection(1)
        onTextEdited: worklogMove.query = text
        Accessible.name: qsTr("Search tasks")
    }

    Rectangle {
        Layout.fillWidth: true
        Layout.preferredHeight: 6 * Tokens.size.listRowHeight
        color: theme.panel
        border.color: theme.line
        radius: theme.radius

        ListView {
            anchors.fill: parent
            anchors.margins: Tokens.border.hairline
            clip: true
            model: worklogMove.candidates
            currentIndex: worklogMove.currentIndex
            highlightFollowsCurrentItem: false
            boundsBehavior: Flickable.StopAtBounds
            // The older results stay up while a new search runs, dimmed so they read as pending.
            opacity: worklogMove.fresh ? 1 : Tokens.emphasis.staleResults
            ScrollBar.vertical: ScrollBar {}

            Accessible.role: Accessible.List
            Accessible.name: qsTr("Destination tasks")

            onCurrentIndexChanged: positionViewAtIndex(currentIndex, ListView.Contain)

            delegate: Rectangle {
                id: row
                required property var modelData
                required property int index
                readonly property bool selected: index === worklogMove.currentIndex

                width: ListView.view.width
                height: Tokens.size.listRowHeight
                color: selected ? theme.selection : "transparent"

                Accessible.role: Accessible.ListItem
                Accessible.name: modelData.name
                Accessible.selected: selected

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: Tokens.spacing.medium
                    anchors.rightMargin: Tokens.spacing.medium
                    spacing: Tokens.spacing.small
                    TaskDot {
                        dotColor: theme.taskColor(catalog.colorNameOf(row.modelData.id))
                    }
                    Txt {
                        Layout.fillWidth: true
                        text: row.modelData.name
                    }
                }
                MouseArea {
                    anchors.fill: parent
                    onClicked: worklogMove.select(row.index)
                    onDoubleClicked: {
                        worklogMove.select(row.index)
                        root.accept()
                    }
                }
            }
        }

        Txt {
            anchors.centerIn: parent
            visible: worklogMove.candidates.length === 0
            text: worklogMove.searching ? qsTr("Searching tasks") : qsTr("No matching tasks")
            level: 1
        }
    }
}

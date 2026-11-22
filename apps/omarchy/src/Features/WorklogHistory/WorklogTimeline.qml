import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../../Shared"

// Worklogs grouped by day on a shared hour axis. Each bar sits where the work happened
// in the day; the running one ends in a blinking block, like a terminal cursor.
Item {
    id: root

    property bool focused: false
    property string selectedWorklogId
    readonly property int labelWidth: 132
    readonly property int durationWidth: 92
    readonly property color taskColor: theme.taskColor(catalog.selectedColorName)

    signal selected(string id)
    signal editTimesRequested(string id)
    signal moveRequested(string id)
    signal deleteRequested(string id)

    // The hour axis above the rows.
    Item {
        id: axis
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: 32 + root.labelWidth
        anchors.rightMargin: 24 + root.durationWidth
        height: 34
        visible: history.count > 0

        Repeater {
            model: Math.max(0, history.axisEndHour - history.axisStartHour + 1)
            Txt {
                required property int index
                readonly property int hour: history.axisStartHour + index
                readonly property int span: Math.max(1, history.axisEndHour - history.axisStartHour)
                // Label every hour while there is room, otherwise every second one.
                visible: span <= 10 || hour % 2 === 0
                x: axis.width * index / span - width / 2
                anchors.verticalCenter: parent.verticalCenter
                text: String(hour).padStart(2, "0")
                font.pixelSize: 11
                level: 2
            }
        }
    }
    Rectangle {
        anchors.top: axis.bottom
        width: parent.width
        height: 1
        color: theme.line
        visible: axis.visible
    }

    ListView {
        id: days
        anchors.fill: parent
        anchors.topMargin: axis.visible ? axis.height + 1 : 0
        clip: true
        model: history.days
        spacing: 6
        boundsBehavior: Flickable.StopAtBounds
        ScrollBar.vertical: ScrollBar {}
        topMargin: 12
        bottomMargin: 16

        Accessible.role: Accessible.List
        Accessible.name: qsTr("Worklogs")

        onAtYEndChanged: if (atYEnd && history.hasMore && !history.loading) history.loadMore()

        delegate: Column {
            id: dayBlock
            required property var modelData
            width: ListView.view.width

            RowLayout {
                width: parent.width
                height: 30
                Txt {
                    Layout.leftMargin: 32
                    Layout.fillWidth: true
                    text: dayBlock.modelData.label
                    font.bold: true
                    Accessible.role: Accessible.Heading
                    Accessible.name: text
                }
                Duration {
                    Layout.rightMargin: 24
                    seconds: dayBlock.modelData.totalSeconds
                    color: theme.textDim
                    pixelSize: 11
                }
            }

            Repeater {
                model: dayBlock.modelData.entries
                delegate: WorklogEntryRow {
                    width: dayBlock.width
                    labelWidth: root.labelWidth
                    durationWidth: root.durationWidth
                    barColor: root.taskColor
                    selected: root.selectedWorklogId === modelData.worklogId
                    focusedList: root.focused
                    onClicked: root.selected(modelData.worklogId)
                    onEditTimesRequested: root.editTimesRequested(modelData.worklogId)
                    onMoveRequested: root.moveRequested(modelData.worklogId)
                    onDeleteRequested: root.deleteRequested(modelData.worklogId)
                }
            }
        }

        footer: Item {
            width: days.width
            height: history.hasMore || history.loading ? 44 : 0
            Txt {
                anchors.centerIn: parent
                visible: history.hasMore || history.loading
                text: history.loading ? qsTr("Loading worklogs") : qsTr("Load older worklogs")
                level: 1
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: history.loadMore()
                }
            }
        }
    }

    Txt {
        anchors.left: parent.left
        anchors.top: parent.top
        anchors.margins: 32
        visible: history.loaded && history.count === 0 && history.error.length === 0
        text: catalog.selectedArchived ? qsTr("No worklogs.") : qsTr("No worklogs yet. Space starts tracking this task.")
        Accessible.role: Accessible.StaticText
        Accessible.name: text
        level: 1
    }
    Txt {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: 32
        visible: history.error.length > 0
        text: history.error
        Accessible.role: Accessible.StaticText
        Accessible.name: text
        color: theme.danger
        wrapMode: Text.Wrap
    }
}

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../../Shared"

// Worklogs grouped by day on a shared hour axis. Each bar sits where the work happened
// in the day; the running one ends in a blinking block, like a terminal cursor.
Item {
    id: root

    property bool focused: false
    property string selectedKey
    readonly property int labelWidth: Tokens.size.timelineLabelWidth
    readonly property int durationWidth: Tokens.size.timelineDurationWidth
    readonly property color taskColor: theme.taskColor(catalog.selectedColorName)

    signal selected(string key)
    signal editTimesRequested(string id)
    signal moveRequested(string id)
    signal deleteRequested(string id)

    // The hour axis above the rows.
    Item {
        id: axis
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: Tokens.spacing.xxLarge + root.labelWidth
        anchors.rightMargin: Tokens.spacing.xLarge + root.durationWidth
        height: Tokens.size.axisHeight
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
                font.pixelSize: Tokens.text.caption
                level: 2
            }
        }
    }
    Rectangle {
        anchors.top: axis.bottom
        width: parent.width
        height: Tokens.border.hairline
        color: theme.line
        visible: axis.visible
    }

    ListView {
        id: days
        anchors.fill: parent
        anchors.topMargin: axis.visible ? axis.height + Tokens.border.hairline : 0
        clip: true
        // The store rebuilds the days once a minute while a worklog runs, and after reloads.
        // Replacing the model resets the scroll, so the same task keeps its position.
        property string shownTaskId
        model: []
        Component.onCompleted: {
            shownTaskId = catalog.selectedTaskId
            model = history.days
        }
        Connections {
            target: history
            function onDaysChanged() {
                const sameTask = days.shownTaskId === catalog.selectedTaskId
                const y = days.contentY
                days.shownTaskId = catalog.selectedTaskId
                days.model = history.days
                if (sameTask)
                    days.contentY = y
            }
        }
        spacing: Tokens.spacing.small
        boundsBehavior: Flickable.StopAtBounds
        ScrollBar.vertical: ScrollBar {}
        topMargin: Tokens.spacing.medium
        bottomMargin: Tokens.spacing.large

        Accessible.role: Accessible.List
        Accessible.name: qsTr("Worklogs")

        onAtYEndChanged: if (atYEnd && history.hasMore && !history.loading) history.loadMore()

        delegate: Column {
            id: dayBlock
            required property var modelData
            width: ListView.view.width

            RowLayout {
                width: parent.width
                height: Tokens.size.dayHeaderHeight
                Txt {
                    Layout.leftMargin: Tokens.spacing.xxLarge
                    Layout.fillWidth: true
                    text: dayBlock.modelData.label
                    font.bold: true
                    Accessible.role: Accessible.Heading
                    Accessible.name: text
                }
                Duration {
                    Layout.rightMargin: Tokens.spacing.xLarge
                    seconds: {
                        const day = dayBlock.modelData
                        // Only the day that holds the running worklog ticks, and only while it still runs.
                        if (day.runningWorklogId.length === 0 || day.runningWorklogId !== tracking.runningWorklogId)
                            return day.totalSeconds
                        return day.totalSeconds - day.runningSeconds + Math.max(0, tracking.elapsedSeconds - day.runningOffset)
                    }
                    color: theme.textDim
                    pixelSize: Tokens.text.caption
                }
            }

            Repeater {
                model: dayBlock.modelData.entries
                delegate: WorklogEntryRow {
                    width: dayBlock.width
                    labelWidth: root.labelWidth
                    durationWidth: root.durationWidth
                    barColor: root.taskColor
                    selected: root.selectedKey === modelData.rowKey
                    focusedList: root.focused
                    onClicked: root.selected(modelData.rowKey)
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
        anchors.margins: Tokens.spacing.xxLarge
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
        anchors.margins: Tokens.spacing.xxLarge
        visible: history.error.length > 0
        text: history.error
        Accessible.role: Accessible.StaticText
        Accessible.name: text
        color: theme.danger
        wrapMode: Text.Wrap
    }
}

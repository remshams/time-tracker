import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../../Shared"

// The selected task: its name, today's time and its worklogs on a timeline.
Rectangle {
    id: root

    property bool focused: false
    property string selectedWorklogId

    signal renameRequested(string taskId)
    signal archiveRequested(string taskId)
    signal editTimesRequested(string worklogId)
    signal moveRequested(string worklogId)
    signal deleteRequested(string worklogId)
    signal createRequested

    // Worklog IDs in display order, for keyboard navigation.
    readonly property var worklogIds: {
        const ids = []
        for (const day of history.days)
            for (const entry of day.entries)
                ids.push(entry.worklogId)
        return ids
    }

    function moveWorklogSelection(step) {
        if (worklogIds.length === 0)
            return
        const current = worklogIds.indexOf(selectedWorklogId)
        const next = current < 0 ? 0 : Math.max(0, Math.min(worklogIds.length - 1, current + step))
        selectedWorklogId = worklogIds[next]
        if (next === worklogIds.length - 1 && history.hasMore)
            history.loadMore()
    }

    readonly property string taskId: catalog.selectedTaskId
    readonly property bool running: taskId.length > 0 && tracking.runningTaskId === taskId
    readonly property var todaySeconds: {
        totals.totalSeconds
        return totals.secondsFor(taskId)
    }

    onTaskIdChanged: selectedWorklogId = ""
    onWorklogIdsChanged: if (selectedWorklogId.length > 0 && worklogIds.indexOf(selectedWorklogId) < 0) selectedWorklogId = ""

    color: theme.background

    // No task at all: invite the first one.
    Column {
        anchors.centerIn: parent
        visible: catalog.loaded && catalog.activeCount === 0 && root.taskId.length === 0
        spacing: 14
        Txt {
            anchors.horizontalCenter: parent.horizontalCenter
            text: qsTr("Create a task to start tracking your time.")
            level: 1
        }
        Button {
            anchors.horizontalCenter: parent.horizontalCenter
            text: qsTr("New task")
            font.family: theme.fontFamily
            highlighted: true
            onClicked: root.createRequested()
        }
    }

    ColumnLayout {
        anchors.fill: parent
        visible: root.taskId.length > 0
        spacing: 0

        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: 32
            Layout.rightMargin: 24
            Layout.topMargin: 24
            spacing: 16

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 6
                Txt {
                    Layout.fillWidth: true
                    text: catalog.selectedName
                    font.pixelSize: 24
                    font.bold: true
                    Accessible.role: Accessible.Heading
                    Accessible.name: text
                }
                RowLayout {
                    spacing: 10
                    TaskDot {
                        dotColor: theme.taskColor(catalog.selectedColorName)
                        filled: root.todaySeconds > 0 || root.running
                    }
                    Duration {
                        visible: root.todaySeconds > 0 || root.running
                        seconds: Math.max(0, root.todaySeconds)
                        color: theme.textDim
                    }
                    Txt {
                        text: {
                            if (root.todaySeconds > 0 || root.running)
                                return catalog.selectedArchived ? qsTr("today, archived") : qsTr("today")
                            // Reading the selection too keeps this current when another task is picked.
                            const last = catalog.selectedTaskId.length > 0 ? catalog.selectedLastWorked : ""
                            const when = last.length === 0 ? qsTr("Not tracked yet") : qsTr("Last tracked %1").arg(last.toLowerCase())
                            return catalog.selectedArchived ? qsTr("%1, archived").arg(when) : when
                        }
                        level: 1
                    }
                }
            }

            KeyHint {
                keys: "e"
                action: qsTr("rename")
                clickable: true
                onClicked: root.renameRequested(root.taskId)
            }
            KeyHint {
                keys: "a"
                action: catalog.selectedArchived ? qsTr("unarchive") : qsTr("archive")
                clickable: true
                onClicked: root.archiveRequested(root.taskId)
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.topMargin: 20
            Layout.preferredHeight: 1
            color: theme.line
        }

        WorklogTimeline {
            Layout.fillWidth: true
            Layout.fillHeight: true
            focused: root.focused
            selectedWorklogId: root.selectedWorklogId
            onSelected: id => root.selectedWorklogId = id
            onEditTimesRequested: id => root.editTimesRequested(id)
            onMoveRequested: id => root.moveRequested(id)
            onDeleteRequested: id => root.deleteRequested(id)
        }
    }
}

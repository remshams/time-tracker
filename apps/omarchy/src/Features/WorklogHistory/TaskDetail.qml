import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../../Shared"

// The selected task: its name, today's time and its worklogs on a timeline.
Rectangle {
    id: root

    property bool focused: false
    // The selected row. A worklog past midnight has one row per day, so the key names the day too.
    property string selectedKey
    readonly property string selectedWorklogId: selectedKey.split("@")[0]

    signal renameRequested(string taskId)
    signal archiveRequested(string taskId)
    signal editTimesRequested(string worklogId)
    signal moveRequested(string worklogId)
    signal deleteRequested(string worklogId)
    signal createRequested

    // Row keys in display order, for keyboard navigation.
    readonly property var rowKeys: {
        const keys = []
        for (const day of history.days)
            for (const entry of day.entries)
                keys.push(entry.rowKey)
        return keys
    }

    function moveWorklogSelection(step) {
        if (rowKeys.length === 0)
            return
        const current = rowKeys.indexOf(selectedKey)
        const next = current < 0 ? 0 : Math.max(0, Math.min(rowKeys.length - 1, current + step))
        selectedKey = rowKeys[next]
        if (next === rowKeys.length - 1 && history.hasMore)
            history.loadMore()
    }

    readonly property string taskId: catalog.selectedTaskId
    readonly property bool running: taskId.length > 0 && tracking.runningTaskId === taskId
    readonly property var todaySeconds: {
        totals.totalSeconds
        return totals.secondsFor(taskId)
    }

    onTaskIdChanged: selectedKey = ""
    onRowKeysChanged: if (selectedKey.length > 0 && rowKeys.indexOf(selectedKey) < 0) selectedKey = ""

    color: theme.background

    // No task at all: invite the first one.
    Column {
        anchors.centerIn: parent
        visible: catalog.loaded && Object.keys(catalog.names).length === 0
        spacing: Tokens.spacing.medium
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

    // Tasks exist, but none is selected, for example when every task is archived.
    Txt {
        anchors.centerIn: parent
        visible: catalog.loaded && Object.keys(catalog.names).length > 0 && root.taskId.length === 0
        text: qsTr("Select a task to see its worklogs.")
        level: 1
        Accessible.role: Accessible.StaticText
        Accessible.name: text
    }

    ColumnLayout {
        anchors.fill: parent
        visible: root.taskId.length > 0
        spacing: 0

        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: Tokens.spacing.xxLarge
            Layout.rightMargin: Tokens.spacing.xLarge
            Layout.topMargin: Tokens.spacing.xLarge
            spacing: Tokens.spacing.large

            ColumnLayout {
                Layout.fillWidth: true
                spacing: Tokens.spacing.small
                Txt {
                    Layout.fillWidth: true
                    text: catalog.selectedName
                    font.pixelSize: Tokens.text.heading
                    font.bold: true
                    Accessible.role: Accessible.Heading
                    Accessible.name: text
                }
                RowLayout {
                    spacing: Tokens.spacing.medium
                    TaskDot {
                        dotColor: theme.taskColor(catalog.selectedColorName)
                        filled: root.todaySeconds > 0 || root.running
                    }
                    // Without a report for today the total is unknown, so it isn't shown.
                    Duration {
                        visible: root.todaySeconds > 0 || (root.running && root.todaySeconds >= 0)
                        seconds: Math.max(0, root.todaySeconds)
                        color: theme.textDim
                    }
                    Txt {
                        text: {
                            if (root.todaySeconds > 0 || (root.running && root.todaySeconds >= 0))
                                return qsTr("today")
                            if (root.running)
                                return qsTr("Running")
                            // Reading the selection too keeps this current when another task is picked.
                            const last = catalog.selectedTaskId.length > 0 ? catalog.selectedLastWorked : ""
                            return last.length === 0 ? qsTr("Not tracked yet") : qsTr("Last tracked %1").arg(last.toLowerCase())
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
            Layout.topMargin: Tokens.spacing.xLarge
            Layout.preferredHeight: Tokens.border.hairline
            color: theme.line
        }

        WorklogTimeline {
            Layout.fillWidth: true
            Layout.fillHeight: true
            focused: root.focused
            selectedKey: root.selectedKey
            onSelected: key => root.selectedKey = key
            onEditTimesRequested: id => root.editTimesRequested(id)
            onMoveRequested: id => root.moveRequested(id)
            onDeleteRequested: id => root.deleteRequested(id)
        }
    }
}

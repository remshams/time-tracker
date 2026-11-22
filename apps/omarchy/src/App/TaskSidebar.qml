import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts
import "../Shared"

// The task list with its Active and Archived tabs and the filter.
Rectangle {
    id: root

    property bool focused: true
    property alias filterField: filter

    signal renameRequested(string taskId)
    signal archiveRequested(string taskId)
    signal createRequested
    signal bulkArchiveRequested
    // The filter field let go of the keyboard.
    signal filterDone

    color: theme.panel

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: Tokens.spacing.large
            Layout.rightMargin: Tokens.spacing.medium
            Layout.topMargin: Tokens.spacing.medium
            Layout.bottomMargin: Tokens.spacing.small
            spacing: Tokens.spacing.large

            Repeater {
                model: [{
                        "archived": false,
                        "label": qsTr("Active")
                    }, {
                        "archived": true,
                        "label": qsTr("Archived")
                    }]
                delegate: Column {
                    required property var modelData
                    readonly property bool current: catalog.archivedTab === modelData.archived
                    spacing: Tokens.spacing.xSmall

                    Accessible.role: Accessible.PageTab
                    Accessible.name: modelData.label
                    Accessible.selected: current
                    Accessible.onPressAction: catalog.archivedTab = modelData.archived

                    Txt {
                        text: parent.modelData.label
                        font.bold: parent.current
                        level: parent.current ? 0 : 1
                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: catalog.archivedTab = parent.parent.modelData.archived
                        }
                    }
                    Rectangle {
                        width: parent.width
                        height: Tokens.border.focus
                        color: parent.current ? theme.accent : "transparent"
                    }
                }
            }
            Item {
                Layout.fillWidth: true
            }
            Txt {
                text: "+"
                font.pixelSize: Tokens.text.glyph
                level: 1
                Accessible.role: Accessible.Button
                Accessible.name: qsTr("New task")
                Accessible.onPressAction: root.createRequested()
                MouseArea {
                    anchors.fill: parent
                    anchors.margins: -Tokens.spacing.small
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.createRequested()
                }
            }
        }

        TextField {
            id: filter
            Layout.fillWidth: true
            Layout.leftMargin: Tokens.spacing.medium
            Layout.rightMargin: Tokens.spacing.medium
            visible: activeFocus || text.length > 0
            placeholderText: qsTr("Filter tasks")
            font.family: theme.fontFamily
            font.pixelSize: Tokens.text.body
            text: catalog.filterText
            onTextEdited: catalog.filterText = text
            Material.accent: theme.accent
            Material.foreground: theme.text
            Material.containerStyle: Material.Outlined
            Keys.onEscapePressed: {
                catalog.filterText = ""
                root.filterDone()
            }
            Keys.onDownPressed: catalog.moveSelection(1)
            Keys.onUpPressed: catalog.moveSelection(-1)
            Keys.onReturnPressed: root.filterDone()
            Keys.onEnterPressed: root.filterDone()
            // Tab ends filtering instead of reaching the window, where it would switch tabs.
            Keys.onTabPressed: root.filterDone()
            Accessible.name: qsTr("Filter tasks")
        }

        ListView {
            id: list
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.topMargin: Tokens.spacing.xSmall
            clip: true
            model: catalog.model
            // Reading the names too re-runs this after a reorder, which keeps the selected row in view.
            currentIndex: {
                catalog.names
                return catalog.model.indexOf(catalog.selectedTaskId)
            }
            highlightFollowsCurrentItem: false
            boundsBehavior: Flickable.StopAtBounds
            ScrollBar.vertical: ScrollBar {}

            Accessible.role: Accessible.List
            Accessible.name: catalog.archivedTab ? qsTr("Archived tasks") : qsTr("Active tasks")

            onCurrentIndexChanged: if (currentIndex >= 0) positionViewAtIndex(currentIndex, ListView.Contain)

            delegate: TaskRowDelegate {
                width: ListView.view.width
                focusedList: root.focused
                onRenameRequested: id => root.renameRequested(id)
                onArchiveRequested: id => root.archiveRequested(id)
            }

            Column {
                anchors.centerIn: parent
                visible: list.count === 0 && catalog.loaded
                spacing: Tokens.spacing.medium
                width: parent.width - 2 * Tokens.spacing.large

                Txt {
                    width: parent.width
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.Wrap
                    level: 1
                    text: catalog.filterText.length > 0 ? qsTr("No task matches \"%1\".").arg(catalog.filterText) : catalog.archivedTab ? qsTr("No archived tasks.") : qsTr("No active tasks. Press n to create one.")
                    Accessible.role: Accessible.StaticText
                    Accessible.name: text
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: Tokens.border.hairline
            color: theme.line
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.margins: Tokens.spacing.medium
            Layout.leftMargin: Tokens.spacing.large
            visible: !catalog.archivedTab
            Txt {
                Layout.fillWidth: true
                text: qsTr("Archive inactive tasks")
                font.pixelSize: Tokens.text.caption
                level: 1
                Accessible.role: Accessible.Button
                Accessible.name: text
                Accessible.onPressAction: root.bulkArchiveRequested()
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.bulkArchiveRequested()
                }
            }
        }
    }
}

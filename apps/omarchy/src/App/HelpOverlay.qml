import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../Shared"

// Every shortcut, opened with ?.
Popup {
    id: root

    readonly property var groups: [{
            "title": qsTr("Tasks"),
            "keys": [["j  k  ↓  ↑", qsTr("Select the next or previous task")], ["Tab", qsTr("Switch between Active and Archived")], ["/", qsTr("Filter tasks")], ["n", qsTr("New task")], ["e", qsTr("Rename the selected task")], ["a", qsTr("Archive or unarchive the selected task")], ["A", qsTr("Archive inactive tasks")]]
        }, {
            "title": qsTr("Tracking"),
            "keys": [["space", qsTr("Start the selected task, or stop it while it runs")], ["s", qsTr("Stop tracking")]]
        }, {
            "title": qsTr("Worklogs"),
            "keys": [["l  →", qsTr("Move to the worklogs")], ["h  ←", qsTr("Back to the tasks")], ["j  k", qsTr("Select the next or previous worklog")], ["t", qsTr("Edit the worklog's times")], ["m", qsTr("Move the worklog to another task")], ["d", qsTr("Delete the worklog")]]
        }, {
            "title": qsTr("App"),
            "keys": [["r", qsTr("Reload")], [",", qsTr("Settings")], ["?", qsTr("Show or hide this list")], ["q", qsTr("Quit. Tracking keeps running.")]]
        }]

    parent: Overlay.overlay
    anchors.centerIn: parent
    modal: true
    focus: true
    padding: Tokens.spacing.xLarge
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

    Overlay.modal: Rectangle {
        color: Qt.alpha(theme.background, Tokens.emphasis.scrim)
    }
    background: Rectangle {
        color: theme.background
        border.color: theme.accent
        border.width: Tokens.border.focus
        radius: theme.radius
    }

    contentItem: ColumnLayout {
        spacing: Tokens.spacing.large
        focus: true
        Keys.onPressed: event => {
            if (event.text === "?") {
                root.close()
                event.accepted = true
            }
        }
        Txt {
            text: qsTr("Keys")
            font.pixelSize: Tokens.text.title
            font.bold: true
        }
        GridLayout {
            columns: 2
            columnSpacing: Tokens.spacing.xxLarge
            rowSpacing: Tokens.spacing.large
            Repeater {
                model: root.groups
                ColumnLayout {
                    required property var modelData
                    Layout.alignment: Qt.AlignTop
                    spacing: Tokens.spacing.small
                    Txt {
                        text: parent.modelData.title
                        font.bold: true
                        level: 1
                    }
                    Repeater {
                        model: parent.modelData.keys
                        RowLayout {
                            required property var modelData
                            spacing: Tokens.spacing.medium
                            Txt {
                                Layout.preferredWidth: Tokens.size.keyColumnWidth
                                text: parent.modelData[0]
                                color: theme.accent
                                font.bold: true
                            }
                            Txt {
                                text: parent.modelData[1]
                            }
                        }
                    }
                }
            }
        }
    }
}

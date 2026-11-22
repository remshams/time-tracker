import QtQuick
import QtQuick.Layouts
import "../Shared"

// The bottom line: the keys for the focused pane and where the data comes from.
Rectangle {
    id: root

    property bool historyFocused: false

    implicitHeight: 30
    color: theme.panel

    Rectangle {
        width: parent.width
        height: 1
        color: theme.line
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 16
        anchors.rightMargin: 16
        spacing: 18

        KeyHint {
            keys: "space"
            action: tracking.running && tracking.runningTaskId === catalog.selectedTaskId ? qsTr("stop") : qsTr("start")
            active: tracking.available
        }
        KeyHint {
            visible: !root.historyFocused
            keys: "n"
            action: qsTr("new")
        }
        KeyHint {
            visible: !root.historyFocused
            keys: "e"
            action: qsTr("rename")
        }
        KeyHint {
            visible: !root.historyFocused
            keys: "/"
            action: qsTr("filter")
        }
        KeyHint {
            visible: root.historyFocused
            keys: "t"
            action: qsTr("edit times")
        }
        KeyHint {
            visible: root.historyFocused
            keys: "m"
            action: qsTr("move")
        }
        KeyHint {
            visible: root.historyFocused
            keys: "d"
            action: qsTr("delete")
        }
        KeyHint {
            keys: "?"
            action: qsTr("all keys")
        }
        Item {
            Layout.fillWidth: true
        }
        Txt {
            font.pixelSize: 11
            color: connection.status === "unavailable" ? theme.danger : theme.textFaint
            text: {
                const source = connection.remote ? connection.serverUrl : qsTr("Local database")
                if (connection.status === "unavailable")
                    return qsTr("%1 unavailable").arg(source)
                if (connection.status === "connecting")
                    return qsTr("Connecting to %1").arg(source)
                return source
            }
            Accessible.role: Accessible.StaticText
            Accessible.name: text
        }
    }
}

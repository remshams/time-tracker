import QtQuick
import "../../Shared"

// Today's time split by task across the full window width, largest share first.
Item {
    id: root

    implicitHeight: Tokens.size.shareBarHeight

    Accessible.role: Accessible.Indicator
    Accessible.name: totals.leaderTaskId.length > 0 ? qsTr("Today: %1 has %2 percent").arg((catalog.names[totals.leaderTaskId] ?? "")).arg(totals.leaderPercent) : qsTr("No time today")

    Rectangle {
        anchors.fill: parent
        color: theme.line
        opacity: Tokens.emphasis.shareTrack
    }

    Row {
        anchors.fill: parent
        Repeater {
            model: totals.shares
            Rectangle {
                required property var modelData
                height: root.height
                width: root.width * modelData.fraction
                color: theme.taskColor(modelData.colorName)
            }
        }
    }
}

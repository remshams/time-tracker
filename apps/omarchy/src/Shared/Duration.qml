import QtQuick

// A duration as H:MM with the seconds dimmed after it, the way the timer reads.
Row {
    id: root

    property var seconds: 0
    property int pixelSize: 13
    property color color: theme.text
    property bool bold: false

    Accessible.role: Accessible.StaticText
    Accessible.name: format.duration(root.seconds)

    Txt {
        text: format.hoursMinutes(root.seconds)
        font.pixelSize: root.pixelSize
        font.bold: root.bold
        color: root.color
    }
    Txt {
        text: format.secondsSuffix(root.seconds)
        font.pixelSize: root.pixelSize
        font.bold: root.bold
        color: Qt.alpha(root.color, 0.45)
    }
}

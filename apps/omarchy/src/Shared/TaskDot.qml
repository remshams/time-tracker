import QtQuick

// A task's color dot. Filled while the task has time today, outlined otherwise,
// like the Omarchy bar widget and the macOS app.
Rectangle {
    property color dotColor: theme.accent
    property bool filled: true
    property int size: Tokens.size.dot

    implicitWidth: size
    implicitHeight: size
    radius: size / 2
    color: filled ? dotColor : "transparent"
    border.color: dotColor
    border.width: filled ? 0 : Tokens.border.focus
}

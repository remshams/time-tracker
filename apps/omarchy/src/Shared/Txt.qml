import QtQuick

// Text in the theme's font and colors. `level` picks the foreground strength:
// 0 full, 1 dim, 2 faint.
Text {
    property int level: 0

    font.family: theme.fontFamily
    font.pixelSize: Tokens.text.body
    color: level === 0 ? theme.text : level === 1 ? theme.textDim : theme.textFaint
    elide: Text.ElideRight
    verticalAlignment: Text.AlignVCenter
}

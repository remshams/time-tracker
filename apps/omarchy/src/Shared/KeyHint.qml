import QtQuick

// One "key action" pair in the status line or a header. A clickable hint runs its action
// on click too.
Item {
    id: root

    property string keys
    property string action
    property bool active: true
    property bool clickable: false

    signal clicked

    implicitWidth: row.implicitWidth
    implicitHeight: row.implicitHeight
    opacity: active ? 1 : Tokens.emphasis.disabled

    Accessible.role: clickable ? Accessible.Button : Accessible.StaticText
    Accessible.name: action
    Accessible.onPressAction: if (clickable) root.clicked()

    Row {
        id: row
        spacing: Tokens.spacing.small

        Txt {
            text: root.keys
            font.pixelSize: Tokens.text.caption
            font.bold: true
            color: theme.accent
        }
        Txt {
            text: root.action
            font.pixelSize: Tokens.text.caption
            level: 1
        }
    }

    MouseArea {
        anchors.fill: parent
        anchors.margins: -Tokens.spacing.xSmall
        enabled: root.clickable
        cursorShape: Qt.PointingHandCursor
        onClicked: root.clicked()
    }
}

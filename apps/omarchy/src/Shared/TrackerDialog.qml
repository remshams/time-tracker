import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts

// The frame every dialog uses: a title, the feature's content, an error line and the
// actions. It looks like a focused Hyprland window: flat, with the accent as border.
// Return runs the primary action, Escape cancels.
Popup {
    id: root

    property string title
    property string primaryText: qsTr("Save")
    property bool primaryEnabled: true
    property bool destructive: false
    property bool busy: false
    property string error
    property int dialogWidth: 460
    default property alias content: body.data

    signal accepted
    signal cancelled

    function accept() {
        if (root.primaryEnabled && !root.busy) {
            root.accepted()
        }
    }

    parent: Overlay.overlay
    anchors.centerIn: parent
    // A layout ignores an implicit width set on it, so the frame fixes the width itself.
    width: root.dialogWidth
    modal: true
    focus: true
    padding: 0
    // A running write keeps the dialog open, so its answer always has somewhere to show.
    closePolicy: root.busy ? Popup.NoAutoClose : Popup.CloseOnEscape
    onClosed: root.cancelled()

    Material.roundedScale: theme.radius
    Overlay.modal: Rectangle {
        color: Qt.alpha(theme.background, 0.7)
    }

    background: Rectangle {
        color: theme.background
        border.color: root.destructive ? theme.danger : theme.accent
        border.width: 2
        radius: theme.radius
    }

    contentItem: ColumnLayout {
        // Without a text field in the dialog, this keeps the keyboard on the Return and Escape handling.
        focus: true
        spacing: 0

        Keys.onReturnPressed: root.accept()
        Keys.onEnterPressed: root.accept()

        Txt {
            Layout.fillWidth: true
            Layout.margins: 20
            Layout.bottomMargin: 12
            text: root.title
            font.pixelSize: 16
            font.bold: true
            Accessible.role: Accessible.Heading
        }

        ColumnLayout {
            id: body
            Layout.fillWidth: true
            Layout.leftMargin: 20
            Layout.rightMargin: 20
            spacing: 10
        }

        Txt {
            Layout.fillWidth: true
            Layout.leftMargin: 20
            Layout.rightMargin: 20
            Layout.topMargin: 10
            visible: root.error.length > 0
            text: root.error
            color: theme.danger
            wrapMode: Text.Wrap
            elide: Text.ElideNone
            Accessible.role: Accessible.AlertMessage
            Accessible.name: root.error
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.margins: 20
            Layout.topMargin: 16
            spacing: 8

            BusyIndicator {
                Layout.preferredWidth: 22
                Layout.preferredHeight: 22
                running: root.busy
                visible: root.busy
                Material.accent: theme.accent
            }
            Item {
                Layout.fillWidth: true
            }
            Button {
                text: qsTr("Cancel")
                flat: true
                enabled: !root.busy
                font.family: theme.fontFamily
                Material.foreground: theme.textDim
                onClicked: root.close()
            }
            Button {
                text: root.primaryText
                enabled: root.primaryEnabled && !root.busy
                font.family: theme.fontFamily
                highlighted: true
                Material.accent: root.destructive ? theme.danger : theme.accent
                Material.foreground: theme.background
                onClicked: root.accept()
            }
        }
    }
}

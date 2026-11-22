import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts
import "../../Shared"

// Chooses where the tracker data lives. Test connection checks the draft without
// changing anything; Connect switches to it and saves it only when the connection works.
TrackerDialog {
    id: root

    // Set while Connect waits for its answer, so the dialog closes only for that
    // outcome and not for a successful test.
    property bool applying: false
    readonly property bool serverMissing: connection.draftRemote && connection.draftServerUrl.trim().length === 0

    title: qsTr("Settings")
    primaryText: qsTr("Connect")
    primaryEnabled: !root.serverMissing
    busy: connection.draftPending
    dialogWidth: Tokens.size.wideDialogWidth

    function openSettings() {
        connection.resetDraft()
        root.applying = false
        root.open()
        local.forceActiveFocus()
    }

    onAccepted: {
        root.applying = true
        connection.applyDraft()
    }

    Connections {
        target: connection

        function onDraftChanged() {
            if (!root.applying || connection.draftPending) {
                return
            }
            root.applying = false
            if (connection.draftResultOk) {
                root.close()
            }
        }
    }

    Txt {
        Layout.fillWidth: true
        text: qsTr("Data source")
        level: 1
        font.bold: true
    }

    RadioButton {
        id: local
        Layout.fillWidth: true
        text: qsTr("Local database")
        checked: !connection.draftRemote
        onClicked: connection.draftRemote = false
        font.family: theme.fontFamily
        Material.accent: theme.accent
        Material.foreground: theme.text
        Accessible.name: text
    }

    RadioButton {
        Layout.fillWidth: true
        text: qsTr("Server")
        checked: connection.draftRemote
        onClicked: connection.draftRemote = true
        font.family: theme.fontFamily
        Material.accent: theme.accent
        Material.foreground: theme.text
        Accessible.name: text
    }

    TextField {
        Layout.fillWidth: true
        visible: connection.draftRemote
        placeholderText: qsTr("http://host:port")
        text: connection.draftServerUrl
        onTextEdited: connection.draftServerUrl = text
        inputMethodHints: Qt.ImhUrlCharactersOnly | Qt.ImhNoPredictiveText
        font.family: theme.fontFamily
        font.pixelSize: Tokens.text.body
        Material.accent: theme.accent
        Material.foreground: theme.text
        Material.containerStyle: Material.Outlined
        Accessible.name: qsTr("Server URL")
        Accessible.description: qsTr("The full HTTP or HTTPS origin, including the port")
    }

    Txt {
        Layout.fillWidth: true
        visible: connection.draftRemote
        text: qsTr("Use the full HTTP or HTTPS origin of the tracker server, including its port.")
        level: 2
        wrapMode: Text.Wrap
        elide: Text.ElideNone
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: Tokens.spacing.medium

        Button {
            text: qsTr("Test connection")
            flat: true
            enabled: !connection.draftPending && !root.serverMissing
            onClicked: connection.testDraft()
            font.family: theme.fontFamily
            Material.foreground: theme.accent
        }

        Txt {
            Layout.fillWidth: true
            visible: connection.draftResult.length > 0
            text: connection.draftResult
            color: connection.draftResultOk ? theme.taskColor("green") : theme.danger
            wrapMode: Text.Wrap
            elide: Text.ElideNone
            Accessible.role: Accessible.StaticText
            Accessible.name: connection.draftResult
        }
    }

    Txt {
        Layout.fillWidth: true
        Layout.topMargin: Tokens.spacing.small
        text: qsTr("Switching never copies data between the local database and a server. "
                   + "A server that is unreachable never falls back to the local database, "
                   + "and the app does not queue changes while it is offline.")
        level: 1
        wrapMode: Text.Wrap
        elide: Text.ElideNone
    }

    Txt {
        Layout.fillWidth: true
        visible: Qt.application.version.length > 0
        text: qsTr("Version %1").arg(Qt.application.version)
        level: 2
    }
}

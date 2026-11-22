import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts
import "../Shared"
import "../Features/Connection"
import "../Features/DailyTotals"
import "../Features/WorklogHistory"
import "../Features/TaskCreation"
import "../Features/TaskRename"
import "../Features/TaskArchiving"
import "../Features/BulkTaskArchiving"
import "../Features/WorklogCorrection"
import "../Features/WorklogMove"
import "../Features/WorklogDeletion"

ApplicationWindow {
    id: window

    property bool historyFocused: false

    // Gives the keyboard back to the window after a dialog or the filter closes.
    function refocus() {
        keys.forceActiveFocus()
    }
    function rename(taskId) {
        if (taskId.length > 0)
            renameDialog.openFor(taskId)
    }
    function archive(taskId) {
        if (taskId.length > 0)
            archiveDialog.openFor(taskId)
    }
    function worklogAction(dialog) {
        if (detail.selectedWorklogId.length > 0)
            dialog.openFor(detail.selectedWorklogId)
    }

    width: Tokens.size.windowWidth
    height: Tokens.size.windowHeight
    minimumWidth: Tokens.size.minimumWindowWidth
    minimumHeight: Tokens.size.minimumWindowHeight
    visible: true
    title: tracking.running ? qsTr("%1 %2 - Time Tracker").arg(format.duration(tracking.elapsedSeconds)).arg((catalog.names[tracking.runningTaskId] ?? "")) : qsTr("Time Tracker")
    color: theme.background
    font.family: theme.fontFamily

    Material.theme: theme.dark ? Material.Dark : Material.Light
    Material.accent: theme.accent
    Material.primary: theme.accent
    Material.background: theme.background
    Material.foreground: theme.text
    Material.roundedScale: theme.radius

    Shortcut {
        sequences: [StandardKey.Quit]
        context: Qt.ApplicationShortcut
        onActivated: Qt.quit()
    }

    // Single-key commands only reach the window while no dialog or text field has the keyboard.
    FocusScope {
        id: keys
        anchors.fill: parent
        focus: true

        Keys.onPressed: event => {
            const plain = !(event.modifiers & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier))
            if (!plain)
                return
            event.accepted = true
            switch (event.key) {
            case Qt.Key_J:
            case Qt.Key_Down:
                if (window.historyFocused)
                    detail.moveWorklogSelection(1)
                else
                    catalog.moveSelection(1)
                return
            case Qt.Key_K:
            case Qt.Key_Up:
                if (window.historyFocused)
                    detail.moveWorklogSelection(-1)
                else
                    catalog.moveSelection(-1)
                return
            case Qt.Key_L:
            case Qt.Key_Right:
                if (catalog.selectedTaskId.length > 0) {
                    window.historyFocused = true
                    if (detail.selectedWorklogId.length === 0)
                        detail.moveWorklogSelection(0)
                }
                return
            case Qt.Key_H:
            case Qt.Key_Left:
                window.historyFocused = false
                return
            case Qt.Key_Tab:
                catalog.archivedTab = !catalog.archivedTab
                window.historyFocused = false
                return
            case Qt.Key_Space:
            case Qt.Key_Return:
            case Qt.Key_Enter:
                if (!catalog.selectedArchived)
                    tracking.running && tracking.runningTaskId === catalog.selectedTaskId ? tracking.stop() : tracking.start(catalog.selectedTaskId)
                return
            case Qt.Key_Slash:
                window.historyFocused = false
                sidebar.filterField.forceActiveFocus()
                return
            case Qt.Key_Escape:
                if (window.historyFocused)
                    window.historyFocused = false
                else if (catalog.filterText.length > 0)
                    catalog.filterText = ""
                return
            case Qt.Key_Comma:
                settingsDialog.openSettings()
                return
            case Qt.Key_Question:
                help.open()
                return
            }
            switch (event.text) {
            case "s":
                tracking.stop()
                return
            case "n":
                createDialog.openNew()
                return
            case "e":
                window.rename(catalog.selectedTaskId)
                return
            case "a":
            case "A":
                // Some keyboards report Shift+A with the text "a", so the modifier decides.
                if (event.modifiers & Qt.ShiftModifier)
                    bulkArchiveDialog.openPreview()
                else
                    window.archive(catalog.selectedTaskId)
                return
            case "t":
                window.worklogAction(correctionDialog)
                return
            case "m":
                window.worklogAction(moveDialog)
                return
            case "d":
                window.worklogAction(deletionDialog)
                return
            case "r":
                sync.refresh(true)
                return
            case "q":
                Qt.quit()
                return
            case "?":
                help.open()
                return
            }
            event.accepted = false
        }

        ColumnLayout {
            anchors.fill: parent
            spacing: 0

            TrackingStrip {
                Layout.fillWidth: true
                onShowTask: taskId => catalog.select(taskId)
            }
            DayShareBar {
                Layout.fillWidth: true
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: 0

                TaskSidebar {
                    id: sidebar
                    Layout.preferredWidth: Math.max(Tokens.size.sidebarMinimumWidth, Math.min(Tokens.size.sidebarMaximumWidth, window.width * 0.3))
                    Layout.fillHeight: true
                    focused: !window.historyFocused
                    onRenameRequested: taskId => window.rename(taskId)
                    onArchiveRequested: taskId => window.archive(taskId)
                    onCreateRequested: createDialog.openNew()
                    onBulkArchiveRequested: bulkArchiveDialog.openPreview()
                    onFilterDone: {
                        // The scope would hand the keyboard straight back to the field it last focused.
                        sidebar.filterField.focus = false
                        window.refocus()
                    }
                }
                Rectangle {
                    Layout.preferredWidth: Tokens.border.hairline
                    Layout.fillHeight: true
                    color: theme.line
                }
                TaskDetail {
                    id: detail
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    focused: window.historyFocused
                    onRenameRequested: taskId => window.rename(taskId)
                    onArchiveRequested: taskId => window.archive(taskId)
                    onCreateRequested: createDialog.openNew()
                    onEditTimesRequested: id => correctionDialog.openFor(id)
                    onMoveRequested: id => moveDialog.openFor(id)
                    onDeleteRequested: id => deletionDialog.openFor(id)
                }
            }

            StatusLine {
                Layout.fillWidth: true
                historyFocused: window.historyFocused
            }
        }
    }

    HelpOverlay {
        id: help
        onClosed: window.refocus()
    }
    SettingsDialog {
        id: settingsDialog
        onClosed: window.refocus()
    }
    TaskCreationDialog {
        id: createDialog
        onClosed: window.refocus()
    }
    TaskRenameDialog {
        id: renameDialog
        onClosed: window.refocus()
    }
    TaskArchiveDialog {
        id: archiveDialog
        onClosed: window.refocus()
    }
    BulkArchiveDialog {
        id: bulkArchiveDialog
        onClosed: window.refocus()
    }
    WorklogCorrectionDialog {
        id: correctionDialog
        onClosed: window.refocus()
    }
    WorklogMoveDialog {
        id: moveDialog
        onClosed: window.refocus()
    }
    WorklogDeletionDialog {
        id: deletionDialog
        onClosed: window.refocus()
    }
}

pragma Singleton
import QtQuick

// The design tokens of tt-desktop. Views take font sizes, spacing, corner radii, opacities,
// fixed sizes and motion from here; colors come from Theme, which follows the Omarchy theme.
// bin/lint rejects number literals for font sizes, spacing, margins, padding, radii and
// opacities in any other QML file, so add a token here instead of typing a value in a view.
// Sizes and motion are not checked; their tokens hold by convention.
QtObject {
    // The spacing scale for margins, padding and layout spacing, in pixels.
    readonly property QtObject spacing: QtObject {
        // The gap between the parts of one element.
        readonly property int xxSmall: 2
        readonly property int xSmall: 4
        readonly property int small: 8
        readonly property int medium: 12
        readonly property int large: 16
        readonly property int xLarge: 24
        readonly property int xxLarge: 32
    }

    // Font sizes by the role of the text, in pixels. Everything uses the Omarchy system font.
    readonly property QtObject text: QtObject {
        // Key hints, the hour axis, secondary lines and the status line.
        readonly property int caption: 11
        readonly property int body: 13
        // Dialog titles, the tracking strip's task name and today's total.
        readonly property int title: 16
        // A glyph that stands in for an icon, such as the play triangle or the new-task plus.
        readonly property int glyph: 18
        // The selected task's name.
        readonly property int heading: 24
        // The running timer.
        readonly property int display: 34
    }

    // Corner radii. Corners follow Hyprland's rounding (theme.radius); small elements cap it.
    readonly property QtObject radius: QtObject {
        // The largest radius of a row highlight or a timeline bar.
        readonly property int small: 4
    }

    readonly property QtObject border: QtObject {
        // A rule between areas.
        readonly property int hairline: 1
        // The accent border of a dialog, an outlined task dot and a tab's underline, like a focused window.
        readonly property int focus: 2
        // The task color on the left edge of the tracking strip.
        readonly property int stripe: 4
    }

    // Opacities for states and tints.
    readonly property QtObject emphasis: QtObject {
        // The seconds after a duration's hours and minutes.
        readonly property real seconds: 0.45
        readonly property real hover: 0.45
        // A selected row while its list doesn't have the keyboard.
        readonly property real unfocusedSelection: 0.6
        readonly property real disabled: 0.4
        // The tracking strip's stripe while nothing runs.
        readonly property real idleStripe: 0.35
        // The track behind a timeline bar, and the day-share bar's background.
        readonly property real track: 0.35
        readonly property real shareTrack: 0.5
        // The running worklog's bar, so the cursor at its end stands out.
        readonly property real runningBar: 0.75
        // Results the move search shows while a newer search runs.
        readonly property real staleResults: 0.55
        // The dimmed window behind a dialog.
        readonly property real scrim: 0.7
    }

    // Fixed sizes of recurring elements, in pixels.
    readonly property QtObject size: QtObject {
        readonly property int stripHeight: 64
        readonly property int statusLineHeight: 30
        readonly property int shareBarHeight: 3
        readonly property int taskRowHeight: 54
        readonly property int worklogRowHeight: 32
        readonly property int dayHeaderHeight: 30
        readonly property int axisHeight: 34
        readonly property int timelineLabelWidth: 132
        readonly property int timelineDurationWidth: 92
        readonly property int barHeight: 6
        readonly property int cursorWidth: 7
        readonly property int cursorHeight: 16
        readonly property int dot: 9
        readonly property int runningDot: 11
        // A glyph's slot, and the small spinner in a dialog.
        readonly property int glyphSlot: 22
        readonly property int dialogWidth: 460
        readonly property int wideDialogWidth: 520
        readonly property int sidebarMinimumWidth: 240
        readonly property int sidebarMaximumWidth: 340
        // A label column in a form, such as "Start" in Edit times.
        readonly property int formLabelWidth: 40
        readonly property int keyColumnWidth: 110
        readonly property int daysFieldWidth: 72
        readonly property int listRowHeight: 32
        readonly property int previewListHeight: 200
        readonly property int previewRowHeight: 28
    }

    readonly property QtObject motion: QtObject {
        // Half a blink of the running worklog's cursor.
        readonly property int blink: 600
    }
}

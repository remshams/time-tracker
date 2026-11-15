# 0040: use design tokens for spacing, type and color in the macOS app

## Status

Accepted.

Implemented by #404. #405 added `IconSize`, `BorderWidth`, `Motion` and the tracking pill's text roles.

## Date

2026-11-15

## Context

The macOS app's views typed their spacing, radii, font sizes and colors as literals. `spacing: 12` appeared 13 times, `padding(24)` 9 times and `cornerRadius: 8` 7 times. Some text used system text styles and some used fixed sizes. Three values, 10, 3 and a vertical 10, sat off any scale.

The main-window redesign (#401) adds about 55 more literal values in its prototype, across three slices. Without a shared vocabulary, each slice would pick its own numbers.

## Decision

The app's views take spacing, corner radii, fonts, opacities and colors from named tokens in `apps/swiftui/Sources/TimeTrackerSwiftUI/Shared/DesignTokens/`. A check in `scripts/swift-style.py` rejects number literals in view styling outside that folder.

### Tokens

- `Spacing`: a 4-point scale, 4, 8, 12, 16, 24 and 32, for padding, stack spacing and spacer lengths.
- `CornerRadius`: 4 and 8.
- `TextRole`: fonts named after the role of the text, such as `sheetTitle`, `detail` and `caption`. They build on the system text styles. `duration(_:)` adds monospaced digits to a text style, so a running time keeps its width. `display` and `actionGlyph`, the 8-point glyph inside a marker disc, are the only roles with a fixed point size. The disc has a fixed size, so its glyph has one too.
- `Emphasis`: the opacities for a resting tint, hover, pressed and disabled.
- `ColorRole`: semantic colors (`error`, `connected`, `confirmation`), `task(_:)`, the SwiftUI color for a `TaskColor`, and `actionDisc(_:in:)`, which darkens that color in dark mode behind a white glyph. The rule that picks a task's color stays in the package's `TaskIndicator`.
- `IconSize`: 10 for a marker dot or ring, 16 for a task indicator, a small spinner or a marker slot, and 22 for a row's start button.
- `BorderWidth`: 1 for a card outline and 1.75 for the tracking pill's rings.
- `Motion`: `quick` (0.12 s) for a press, `standard` (0.18 s) for a state or hover change, and the 0.92 scale of a pressed marker.

Hierarchical styles such as `.primary` and `.secondary` stay as they are; they already name a role. Control heights stay with the system. Frame sizes other than icon sizes stay out of scope, such as the tracking pill's 34-point height. `scripts/swift-style.py` doesn't read icon sizes, border widths or animations, so `IconSize`, `BorderWidth` and `Motion` hold by convention. AppKit code in the menu bar keeps its own values.

The move onto the tokens snapped the off-scale values: the task header's 10 became 8, and the connection summary's 3 became 4 and its vertical 10 became 12. The durations in the worklog card and in the delete sheet's summary changed from a monospaced font to the system font with monospaced digits. Everything else looks the same. #405 snapped the prototype pill's padding of 14 to 12, its 18-point marker slot to 16, its 1.5-point ring to 1.75, and its 0.15 and 0.2 second animations to 0.18.

### The check

`scripts/swift-style.py` fails on a number literal in `.padding(`, `spacing:`, `horizontalSpacing:`, `verticalSpacing:`, `minLength:`, `cornerRadius:`, `.cornerRadius(`, `.system(size:` or `.opacity(` in a file under `apps/swiftui/Sources/TimeTrackerSwiftUI/` outside `Shared/DesignTokens/`. `0` is allowed everywhere, and `1` in opacity, because hiding or showing a view isn't a design value. The check reads one line at a time and skips what follows `//` and strings that open and close on that line. Multi-line and raw strings are not parsed. `0.0` and `1.0` count as `0` and `1`.

### Rejected alternatives

- **A SwiftLint custom rule.** SwiftLint runs custom rules only with SourceKit. The style check runs on Linux with `--disable-sourcekit`, so SwiftLint skipped the rule. The same regular expression runs in `swift-style.py` instead and has unit tests in `scripts/tests/test_swift_style.py`.
- **Tokens in the `TrackerClient` package.** The package is Foundation-only and has no SwiftUI types.
- **Static members on `Font` and `Color`**, such as `.font(.sheetTitle)`. Names like `caption` would shadow the system members.

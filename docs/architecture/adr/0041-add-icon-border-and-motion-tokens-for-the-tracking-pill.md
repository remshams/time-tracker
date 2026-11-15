# 0041: add icon, border and motion tokens for the tracking pill

## Status

Accepted. It partially supersedes the clauses of ADR 0040 listed under [Superseded clauses](#superseded-clauses). All other decisions in that record remain accepted.

Implemented by #405.

## Date

2026-11-15

## Context

[ADR 0040](0040-use-design-tokens-for-spacing-type-and-color-in-the-macos-app.md) gave the macOS app tokens for spacing, corner radii, fonts, opacities and colors. It left frame sizes out of scope and allowed one fixed font size, `display`.

The tracking pill (#405) is the first slice of the main-window redesign. Its prototype used about 20 literals: a 34-point height, 9- and 10-point markers, an 18-point marker slot, ring widths of 1.5 and 1.75, an 8-point glyph, four animation durations and a press scale. Three existing views already framed a spinner or a task indicator at 16 points, and three drew a card outline at 1 point. Without tokens, the next slices would pick their own sizes for the same kinds of things.

## Decision

`Shared/DesignTokens/DesignTokens.swift` gains:

- `IconSize`: `marker` 10 for a marker dot or ring, `regular` 16 for a task indicator, a small spinner or a marker slot, and `button` 22 for a sidebar row's start button.
- `BorderWidth`: `hairline` 1 for a card outline and `marker` 1.75 for the tracking pill's rings.
- `Motion`: `quick` (0.12 s ease-out) for a press, `standard` (0.18 s ease-out) for a state or hover change, and `pressScale` 0.92 for a pressed marker.
- `TextRole.status` (`.body`) for the pill's task name, `TextRole.statusGlyph` (`.caption`, semibold) for a symbol next to status text, and `TextRole.actionGlyph` (8 points, bold) for the stop or play glyph inside a marker disc.
- `ColorRole.actionDisc(_:in:)`, the task color behind a white glyph, darkened by 0.3 in dark mode only.

The three 16-point frames and the three 1-point outlines move onto `IconSize.regular` and `BorderWidth.hairline`.

`actionGlyph` has a fixed size because the disc it sits in has one; a text style would grow the glyph with the system font but not the disc.

`scripts/swift-style.py` doesn't read frame sizes, line widths or animations, so `IconSize`, `BorderWidth` and `Motion` hold by convention. Frame sizes that aren't icons stay literals, such as the pill's 34-point height and its 220-point name width.

The pill snapped the prototype's values: its horizontal padding of 14 to `Spacing.medium` (12), its 18-point marker slot to 16, its 1.5-point "Nothing running" ring to 1.75, and its 0.15 and 0.2 second animations to 0.18.

### Rejected alternatives

- **Extending the style check to frames and line widths.** A square `.frame(width:height:)` isn't always an icon, so the check would flag layout frames too. Three tokens with a handful of uses each don't justify the false positives.
- **A text style for the action glyph.** `.caption2` is about 10 points on macOS and looked too large in a 16-point disc.

### Superseded clauses

This record replaces these clauses of ADR 0040:

- "Tokens", `TextRole`: "`display` is the only role with a fixed point size." `display` and `actionGlyph` have fixed sizes.
- "Tokens": "Control heights stay with the system, and frame sizes are out of scope." Control heights stay with the system. Icon sizes are tokens; other frame sizes stay out of scope.

## Consequences

The task catalog and worklog history slices (#406, #407) can reuse these tokens for their markers, outlines and animations.

Nothing checks that views use `IconSize`, `BorderWidth` and `Motion`. A reviewer has to catch a literal icon size, line width or animation duration.

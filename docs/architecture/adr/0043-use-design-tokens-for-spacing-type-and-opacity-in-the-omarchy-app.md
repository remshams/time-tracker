# 0043: use design tokens for spacing, type and opacity in the Omarchy app

## Status

Proposed

## Date

2026-11-22

## Context

[ADR 0042](0042-build-the-omarchy-client-with-qt-quick-on-the-shared-c-bridge.md) added `tt-desktop`. Its colors, corner radius and font already come from `Shared/Theme`, which follows the Omarchy theme. Font sizes, spacing, opacities and fixed sizes were number literals in the views: 7 font sizes and 14 spacing values across 18 of 23 QML files, several of them off the planned scale.

The macOS app solved the same problem with design tokens and a style check ([ADR 0040](0040-use-design-tokens-for-spacing-type-and-color-in-the-macos-app.md), [ADR 0041](0041-add-icon-border-and-motion-tokens-for-the-tracking-pill.md)).

## Decision

- `apps/omarchy/src/Shared/Tokens.qml` is a QML singleton, registered in `Shared/qmldir`. Every QML file in `Shared/` is listed there, because a directory with a `qmldir` exposes only the types it lists. It holds:
  - `spacing`: 2, 4, 8, 12, 16, 24 and 32;
  - `text`: caption 11, body 13, title 16, glyph 18, heading 24, display 34;
  - `radius.small`, which caps Hyprland's rounding on small elements;
  - `border`: hairline 1, focus 2 and stripe 4;
  - `emphasis` opacities and the tint factors for `Qt.alpha`;
  - `size`, for recurring fixed sizes;
  - `motion`.
- Colors stay in `Theme`, because they come from the Omarchy theme while the app runs.
- `scripts/omarchy-qml-style.py` rejects number literals in `pixelSize`, `pointSize`, every spacing and padding property (such as `rowSpacing` or `leftPadding`), margins, `radius`, `opacity` and the `Qt.alpha` factor outside `Shared/Tokens.qml`. It reads every such property on a line, including inside `{ }` and after `;`. `apps/omarchy/bin/lint` runs it. It allows 0, opacity 1 and divisors such as `size / 2`.
- Sizes and motion are tokens by convention, not checked, as in the macOS app.
- Values off the scale snapped to the nearest step: 6 to 8, 10 and 14 to 12, 18 to 16, 20 to 24, the help overlay's column gap 40 to 32, and the font size 12 to 11. The timeline bar's radius cap went from 3 to 4, which renders the same, because Qt limits a radius to half the bar's 6-pixel height.

## Consequences

- A new view takes its values from the tokens, and the lint fails on a typed one.
- The snapped values shift some margins by 2 to 4 pixels. The timeline, dialogs and sidebar keep their layout.
- The two native apps have separate token files with similar roles. They don't share values, because macOS and Omarchy use different fonts and metrics.

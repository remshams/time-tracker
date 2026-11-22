# 0042: build the Omarchy client with Qt Quick on the shared C bridge

## Status

Proposed

## Date

2026-11-22

## Context

The user also works on an Omarchy machine (Arch Linux with Hyprland). There the terminal UI and an Omarchy bar widget, which drives `tt-cli`, are the only clients. The bar widget covers what the macOS menu bar item does. Nothing covers the macOS main window: the task list, the worklog timeline, today's totals and the dialogs that edit tasks and worklogs.

Omarchy's own desktop apps are C++ and Qt Quick, built with qmake6, themed from the current Omarchy theme and installed as Arch packages. Its shell and the bar widget are QML too.

The macOS app reaches the Rust core through `tracker-swift-bridge`, a static library with a C ABI that answers every call with a JSON envelope and runs its own Tokio runtime. Apart from its name and a few comments, the crate held nothing specific to Swift.

Options for the new client:

- Qt Quick with C++ on the C bridge, as Omarchy's apps are built.
- Qt Quick with Rust objects through cxx-qt, which keeps presentation state in Rust but adds a build layer Omarchy's apps don't use.
- GTK4 and libadwaita through gtk-rs, which is pure Rust but looks like GNOME and needs custom CSS to follow Omarchy themes.
- A pure Rust toolkit (iced or Slint), with weaker text input and accessibility and theming built from scratch.
- A web UI through Tauri, which ships a webview and a second UI stack.
- Driving `tt-cli` like the bar widget, which spawns a process per action and exposes only part of the guarded writes.

## Decision

- The Omarchy client is `tt-desktop` in `apps/omarchy`: C++ and Qt Quick, built with qmake6. QML owns layout and interaction, and C++ `QObject` feature stores own presentation state, the way the Swift `TrackerClient` package does. Domain rules stay in Rust.
- The C bridge moves to `crates/tracker-bridge` and is renamed `tracker-bridge`. The macOS app and `tt-desktop` link the same static library and header. The exports and JSON envelopes stay unchanged.
- `tt-desktop` calls the bridge on one worker thread, in order, and answers on the GUI thread. Feature stores depend on the `TrackerGateway` interface, so tests script the replies.
- `tt-desktop` covers the main window only. The bar widget stays the status indicator and owns pausing on screen lock, so the app has no tray icon and no lifecycle handling.
- The app follows Omarchy conventions: it reads `colors.toml` and Hyprland's rounding live, uses the system font, is keyboard first, wraps every string in `qsTr`, and installs as the `tt-desktop` Arch package.
- C++ quality gates are Qt Test unit tests, clang-format, clang-tidy and coverage in an Arch Linux CI container, so CI builds against the Qt that Omarchy ships. There is no C++ mutation testing for now.

## Consequences

- A protocol change reaches both native clients through one crate.
- Presentation rules exist twice, once in Swift and once in C++. They stay small because the bridge and the core enforce the rules.
- The macOS build reads the header from `crates/tracker-bridge/include`.
- Mutation checks for the bridge now run on `crates/tracker-bridge`.

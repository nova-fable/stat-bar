# StatBar

[中文](README.md) | **English**

[![CI](https://github.com/nova-fable/stat-bar/actions/workflows/ci.yml/badge.svg)](https://github.com/nova-fable/stat-bar/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A macOS menu bar system monitor. CPU, memory and network speed stay in the menu bar; click to open a panel with live data and per-process details.

## Features

- **Menu bar**: a compact combined icon showing CPU and network speed in real time
- **Panel**: CPU, memory, network and disk cards. Click one to open its detail view (large trend charts, memory breakdown, per-process and per-interface details)
- **Settings**: theme (follow system / light / dark), language (follow system / 中文 / English), panel base color and opacity, all persisted
- **Low footprint**: the main view never enumerates processes; process and network details are only sampled while a detail view is open

## Technical approach

objc2 / AppKit owns the persistent menu bar item; GPUI (through gpui-kit) renders the panel that pops up on click. Both run in the same process: GPUI drives the main loop, the status item is created in its run callback, and clicks are bridged to GPUI through async-channel, so the app does not poll while idle.

Measured in release builds (vmmap Physical footprint):

| State | Private footprint | Startup |
|---|---|---|
| Panel closed (typical) | about 16 MB | status item ready in about 160 ms |
| Panel open | about 31 MB (about 64 MB peak while shaders load) | first open about 390 ms (one-time) |

## Installation

Download the latest `StatBar-vX.Y.Z-macos-arm64.dmg` from [Releases](https://github.com/nova-fable/stat-bar/releases), open it and drag StatBar into Applications.

The current build is only ad-hoc signed and not notarized by Apple, so macOS will say it cannot verify the developer the first time you open it. Right-click StatBar in Finder and choose "Open". If it is still blocked, go to System Settings → Privacy & Security and click "Open Anyway".

## Requirements

- macOS 27+ on Apple Silicon (aarch64)
- Rust toolchain (stable)
- Xcode 27+ and the **Metal Toolchain** (gpui compiles shaders at build time):

  ```sh
  xcodebuild -runFirstLaunch
  xcodebuild -downloadComponent MetalToolchain
  ```

## Build and run

```sh
cargo run --release
```

Left-click the menu bar icon to open or close the panel.

To build the `.app` and the DMG (put the icon and background art in `assets/` first):

```sh
scripts/bundle.sh                        # creates target/bundle/StatBar.app
scripts/make-dmg.sh StatBar.dmg          # creates a drag-to-install DMG
```

Debug flags (launch straight into a view, handy during development):

| Flag | Effect |
|---|---|
| `--open-panel` | open the panel on launch |
| `--open-cpu` | open the CPU detail on launch |
| `--open-net` | open the network detail on launch |
| `--open-disk` | open the disk detail on launch |
| `--open-settings` | open settings on launch |

## Project layout

```text
src/
├── main.rs          entry point (app, theme, language, status item, click bridge, debug flags)
├── app.rs           panel window lifecycle: open / position / resize per view / dismiss on blur
├── status_item.rs   menu bar item: combined icon (CoreGraphics + CoreText), click bridge, sampling timer
├── sampler.rs       system sampling and detail publishing (snapshot / history / memory breakdown / processes / interfaces / disk)
├── i18n.rs          UI localization: Chinese / English, follows the system language by default
├── prefs.rs         persisted settings (base color / opacity / theme / language, stored in NSUserDefaults)
├── fmt.rs           number formatting
├── icons.rs         app icon extraction for process details
└── ui/              panel views: home, resource details, settings, theme modes
```

`scripts/` contains the packaging scripts (`bundle.sh` builds the `.app`, `make-dmg.sh` builds the DMG). `assets/` holds the app icon and the DMG background.

## Contributing

Issues and pull requests are welcome. Development conventions, code layering and platform notes are in [CONTRIBUTING.md](CONTRIBUTING.md) (in Chinese) and [AGENTS.md](AGENTS.md).

## Roadmap

- Package the `.app` with `LSUIElement`, launch at login via SMAppService, signing and notarized distribution
- Polish: open on mouse-down, a right-click menu, a mini chart in the menu bar

## License

[MIT](LICENSE) © 2026 dsjerry

# Onyx Launcher

![Rust](https://img.shields.io/badge/language-Rust-orange) ![Platform](https://img.shields.io/badge/platform-Windows%2011-blue) ![License](https://img.shields.io/badge/license-MIT-green)

A near-instant app drawer that slides up from your Windows taskbar — written in Rust, with no Electron, no GPU driver, no bloat.

![Onyx Launcher screenshot](docs/screenshot.png)

## Installation

1. Grab the latest `.zip` from **[Releases](../../releases/latest)**.
2. Unzip it anywhere (e.g. `C:\Tools\OnyxLauncher\`) — there's no installer, and it doesn't need administrator rights.
3. **Pin `onyx-launcher.exe` to your taskbar first, before running it**: right-click the `.exe` file in File Explorer → **Pin to taskbar** (if you don't see that option, click **Show more options** first, then **Pin to taskbar**). The drawer intentionally never shows its own taskbar button while running, so this only works from the file itself, not from a running window.
4. Click that new taskbar icon to launch it — it slides up immediately, and stays running in the background afterward so every later click is instant.

> **Windows SmartScreen note:** since this is an unsigned, independently-built binary, Windows may show a "Windows protected your PC" prompt the first time you run it. Click **More info → Run anyway**. This is expected for any executable that isn't purchased through a code-signing certificate — the source is fully here if you'd rather build it yourself (see [Building from source](#building-from-source)).

For the full walkthrough — pinning apps, categories, uninstalling, troubleshooting — see the [User Guide](docs/USER_GUIDE.md).

## What it is

Onyx Launcher is a Spotlight-style pinned-app drawer for Windows 11. Click its pinned taskbar icon and it slides up out of the taskbar as a rounded-corner, near-opaque black panel. Click a tile to launch, right-click to remove, type to filter. Click the icon again (or click away) and it slides back down.

Its renderer is hand-written on plain GDI+ specifically to avoid any GPU driver dependency — see [Architecture](#architecture) for why that mattered.

## Features

- **Dead-simple lifecycle, reopens every time** — the process runs *only while the drawer is on screen*. Dismissing it (click away, `Esc`, launch an app, or click the pin again) exits the process entirely, so there's never a hidden background instance to get "stuck". Reopening is just launching the exe again, which the OS does reliably — no resident hub, no wake-up channel that can silently fail.
- **Native Windows 11 look** — rounded, near-opaque black panel with real per-pixel alpha compositing via `UpdateLayeredWindow`, not a faked shape.
- **Crisp at any DPI** — app icons are pulled through the shell's image factory at the exact on-screen pixel size (the same path Explorer uses), and text is grid-fitted, so both stay sharp instead of blurry on scaled displays.
- **Search-as-you-type**, with clipboard paste (`Ctrl+V`) support.
- **Scrollable grid** for when you've pinned more apps than fit on one screen.
- **DPI-aware and follows your cursor's monitor** — same physical size on 100%/150%/200% scaled displays, and opens on whichever screen you're currently on, not a fixed monitor.
- **Smooth hover animation** on tiles, without paying for continuous repaints while idle.
- **Categories**: a companion tool (`onyx-category-maker`) lets you build additional standalone, independently-pinnable `.exe`s — each with its own icon, name, and app list (e.g. a "Games" drawer and a "Work" drawer, each pinned separately to the taskbar). Each is a real distinct executable with its own pinned-app list.
- **Right-click to remove** a pinned app, either via the native context menu or a hover-revealed "×" badge.

## Why it's small

The whole point of this project was chasing "how light can a real, good-looking Windows launcher actually get." The numbers, measured on this machine:

| | |
|---|---|
| Binary size | ~490 KB |
| RAM while open (working set) | ~27 MB |
| RAM / CPU while closed | none — the process doesn't exist |
| GPU driver dependency | None |

That last line is the interesting one. An immediate-mode UI stack like `egui`/`eframe` over OpenGL is a perfectly good choice in general — but loading a real OpenGL context pulls in your GPU vendor's driver DLLs (shader compiler, command buffer infrastructure, the works), which alone accounts for tens of MB of resident memory regardless of how lean the application code is. Onyx Launcher's renderer is instead hand-written on top of **GDI+** — a plain system DLL every Windows process already has access to — composited onto the window via `UpdateLayeredWindow` for real per-pixel alpha. No GPU context, no shader compiler, no vendor driver ever loads.

(DWM's acrylic backdrop material would paint across the window's full rectangular bounds regardless of our rounded alpha shape — producing a visible grey fringe outside the rounded corners — so it's deliberately not used. The panel is already ~96% opaque, so live blur wouldn't add much anyway, and skipping it keeps the corners clean.)

The tradeoff: no immediate-mode UI framework to lean on. Hit-testing, hover state, text input, and layout are all hand-rolled in `app.rs`.

On top of the RAM/binary-size numbers, the render path itself is tuned to do near-zero allocation per frame: GDI+ brushes, string formats, fonts, and per-icon bitmaps are all created once and reused (not recreated on every fill/text/icon draw), icons are decoded once per app at their exact display size in the byte order GDI+ wants natively (no per-frame format conversion, scaling, or buffer copy), and the pinned-app list is only re-filtered when the search text or app list actually changes rather than on every mouse move. Animations and hover transitions stay pegged to the display's real refresh rate with no busy-polling, so idle CPU stays at 0% and interaction stays smooth without burning cycles reconstructing the same GDI+ objects dozens of times a frame.

## Usage

1. Click **+ Add app** and pick an `.exe` to pin it into the grid.
2. Right-click a tile (or hover and click the "×" badge) to remove it.
3. Type to filter, scroll if you've pinned more than fits, `Esc` to clear the search.

### Adding a category

Run `onyx-category-maker.exe` (it must sit next to `onyx-launcher.exe`), give it a name and an icon image (PNG/JPG/BMP/ICO), and it produces a standalone `<name>.exe` under `%LOCALAPPDATA%\OnyxLauncher\categories\<name>\`. Pin that exe to your taskbar like any other app — it's a real distinct executable with your chosen icon, and it maintains its own independent app list.

## Building from source

Requires Rust with the `x86_64-pc-windows-msvc` target (MSVC Build Tools + Windows SDK — no full Visual Studio install needed).

```sh
cargo build --release
```

This produces both binaries in `target/release/`:

- `onyx-launcher.exe` — the drawer itself.
- `onyx-category-maker.exe` — a small GUI tool for generating additional pinnable category drawers.

## Architecture

100% Rust — no C/C++ in the application itself. All Win32/GDI+/DWM access goes through the [`windows`](https://crates.io/crates/windows) crate; [`winit`](https://crates.io/crates/winit) handles windowing/input only (it doesn't own rendering here).

The lifecycle is the load-bearing design decision: **a process lives only while its drawer is visible.** Every way of dismissing the drawer ends by exiting the process, so "closed" and "not running" are the same state. This is what makes reopening bulletproof — there's no resident background process that a later launch has to wake through some channel that might silently fail; you just run the exe again. A per-category named **mutex** stops two copies of the same drawer showing at once, and a named **event** lets a second launch tell an already-open drawer to close (clicking the pin also removes focus, which independently dismisses it, so the close path has a built-in fallback).

- `src/app.rs` — the drawer's state machine, hit-testing, and GDI+ rendering.
- `src/gdiplus.rs` — a minimal safe(ish) wrapper around the raw GDI+ Win32 API.
- `src/main.rs` — a `winit`-based event loop (window/input handling only — no rendering backend).
- `src/single_instance.rs` — the named mutex + event coordination and the exit-on-hide guarantee described above.
- `src/config.rs` — per-category JSON config, with category identity derived purely from the running exe's filename.
- `src/icon.rs` — extracts crisp, display-sized `.exe` icons via `IShellItemImageFactory` (with an `SHGetFileInfoW` fallback) for display in the grid.
- `src/resource_icon.rs` — the reverse: patches a *new* icon resource into a copied `.exe` (used by the category maker), via `BeginUpdateResourceW`/`UpdateResourceW`.
- `src/geometry.rs` — computes the taskbar-flush, bottom-center window position from monitor work-area info.

## License

MIT — see [LICENSE](LICENSE).

Bundles [Ubuntu Light](https://design.ubuntu.com/font) (used by the category-maker tool's UI), under the [Ubuntu Font License](assets/UFL.txt).

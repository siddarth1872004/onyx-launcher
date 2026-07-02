# Onyx Launcher — User Guide

A complete walkthrough of installing, using, and troubleshooting Onyx Launcher.

For a quick overview and the technical architecture, see the [README](../README.md). This guide goes deeper on day-to-day usage.

## Contents

- [Installing](#installing)
- [First run](#first-run)
- [Pinning apps](#pinning-apps)
- [Opening and closing the drawer](#opening-and-closing-the-drawer)
- [Searching](#searching)
- [Removing a pinned app](#removing-a-pinned-app)
- [Categories](#categories)
- [Multiple monitors and DPI](#multiple-monitors-and-dpi)
- [Uninstalling](#uninstalling)
- [Troubleshooting](#troubleshooting)
- [Where things are stored](#where-things-are-stored)

## Installing

1. Download the latest `onyx-launcher-vX.Y.Z-windows-x64.zip` from the [Releases page](https://github.com/siddarth1872004/onyx-launcher/releases/latest).
2. Unzip it to a permanent folder — Onyx Launcher runs from wherever you put it, so don't unzip into `Downloads` and forget about it. `C:\Tools\OnyxLauncher\` or similar works well.
3. There is no installer and no admin rights are required.

You'll see two files:

| File | Purpose |
|---|---|
| `onyx-launcher.exe` | The drawer itself. |
| `onyx-category-maker.exe` | Optional tool for making extra pinnable drawers (see [Categories](#categories)). |

### SmartScreen warning

Because the binary isn't signed with a paid code-signing certificate, Windows SmartScreen may show **"Windows protected your PC"** the first time you run it. Click **More info → Run anyway**. This is a one-time prompt for a given binary/version. If you'd rather avoid it entirely, build from source (see the README).

## First run

Double-click `onyx-launcher.exe`. It slides up from the taskbar immediately with an empty grid — that's expected, you haven't pinned anything yet.

To make it easy to reach later:

1. While it's running, right-click its icon in the taskbar.
2. Choose **Pin to taskbar**.
3. Close the window (click away, or click the pinned icon again) — the taskbar pin stays even though the process keeps running in the background.

From now on, click that pinned icon to summon it — you don't need to keep launching it from the folder. There's no global hotkey; each drawer opens only when you click its own pinned icon, which is what makes [categories](#categories) work cleanly — every one gets its own dedicated icon instead of fighting over a single shared shortcut.

## Pinning apps

Click the **+ Add app** tile in the grid. A native file picker opens — navigate to any `.exe` and select it. It's added to the grid immediately with its real icon extracted straight from the executable.

There's no limit on how many apps you can pin. If you pin more than fit on one screen, the grid scrolls (see [Searching](#searching) below for the mouse/keyboard scroll behavior).

## Opening and closing the drawer

| Action | How |
|---|---|
| Open | Click the pinned taskbar icon |
| Close | Click the pinned icon again, click anywhere outside the drawer, or click a tile to launch it (closes automatically after launching) |

The drawer is a single resident background process — after the very first launch, opening it is instant with no cold-start delay.

## Searching

Just start typing while the drawer is open — no need to click into a search box first. The grid filters to matching app names as you type.

- `Ctrl+V` pastes clipboard text into the search field.
- `Esc` clears the current search.
- Scroll with the mouse wheel (or trackpad) if you have more pinned apps than fit on screen.

## Removing a pinned app

Two ways:

- **Right-click** a tile and choose **Remove** from the context menu.
- **Hover** over a tile until a small "×" badge appears in the corner, then click it.

Either way, this only unpins it from Onyx Launcher — it does not uninstall or delete the actual application.

## Categories

If you want more than one themed drawer (say, a "Games" drawer separate from a "Work" drawer), each pinned separately to the taskbar with its own icon:

1. Run `onyx-category-maker.exe` — it needs to sit in the same folder as `onyx-launcher.exe`.
2. Give the category a name and pick an icon image (PNG, JPG, BMP, or ICO all work).
3. It generates a new standalone `<name>.exe` under `%LOCALAPPDATA%\OnyxLauncher\categories\<name>\`.
4. Pin that new `.exe` to your taskbar just like any normal app.

Each category keeps its own independent list of pinned apps, but under the hood every category and the main drawer all share the *same* single resident background process — pinning more categories doesn't cost you more RAM. Clicking a category's taskbar icon always opens that specific category, never a different one, since each icon pings the shared process with its own name.

## Multiple monitors and DPI

The drawer always opens flush with the taskbar on the monitor your mouse/focus was on, and is DPI-aware — it renders at the same physical size whether that display is scaled to 100%, 150%, or 200%.

## Uninstalling

1. Close the drawer and make sure the process has fully exited: right-click the taskbar and open **Task Manager**, find `onyx-launcher.exe` (and any category `.exe`s you made) under **Background processes**, and click **End task** if still present.
2. Unpin it from the taskbar (right-click the pinned icon → **Unpin from taskbar**).
3. Delete the folder you unzipped it into.
4. Optionally, delete its config folder: `%LOCALAPPDATA%\OnyxLauncher\` (this removes your pinned-app list and any categories you created).

## Troubleshooting

**The drawer won't open at all.**
Make sure no `onyx-launcher.exe` process is already stuck — check Task Manager's **Background processes** tab, end it if present, and relaunch from the exe.

**A pinned app's icon looks wrong or generic.**
This happens if the `.exe` you pinned doesn't embed a proper icon resource itself (common for some portable/scripted tools) — Onyx Launcher shows whatever Windows itself reports for that file.

**Windows SmartScreen keeps appearing.**
Expected for unsigned builds — see [SmartScreen warning](#smartscreen-warning) above. Alternatively, build from source yourself (see the [README](../README.md#building-from-source)).

**I moved or deleted a pinned app's `.exe` and now the tile does nothing.**
Onyx Launcher stores the file path at the time you pinned it. Remove the stale tile and re-add it from its new location.

## Where things are stored

Everything lives under `%LOCALAPPDATA%\OnyxLauncher\`:

| Path | Contents |
|---|---|
| `%LOCALAPPDATA%\OnyxLauncher\apps.json` | Your pinned apps for the main drawer. |
| `%LOCALAPPDATA%\OnyxLauncher\categories\<name>\` | A generated category's `.exe` and its own `apps.json`. |

Both are plain JSON — safe to back up or hand-edit if you know what you're doing, though the app UI is the supported way to manage them.

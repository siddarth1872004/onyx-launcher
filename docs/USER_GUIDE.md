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

Pin `onyx-launcher.exe` to your taskbar **before** running it the first time - this is the one counter-intuitive step, so it's worth calling out explicitly:

1. In File Explorer, right-click the `onyx-launcher.exe` file itself (it doesn't need to be running).
2. Choose **Pin to taskbar**. If you don't see that option in the first menu, click **Show more options** to get the full right-click menu, then choose it there.
3. Click the new taskbar icon to launch it — it slides up immediately with an empty grid, that's expected, you haven't pinned any apps into it yet.

Why pin the file and not a running window: the drawer deliberately never shows its own button in the taskbar's list of open windows (that's what lets it hide instead of visibly "closing"), so there's no running-window icon to right-click later. Pinning the `.exe` file itself sidesteps that entirely and is the only way to get a permanent launch point.

From then on, that taskbar icon both opens and closes the drawer. There's no global hotkey; each drawer opens only when you click its own pinned icon, which is what makes [categories](#categories) work cleanly — every one gets its own dedicated icon instead of fighting over a single shared shortcut.

Onyx Launcher only runs while its drawer is actually on screen. The moment you dismiss it, the program fully exits — it does **not** sit in the background. That's deliberate: it means opening it is always a clean fresh launch, so it can never get into a "stuck, won't reopen" state. (The trade-off is a tiny cold-start each time, but the drawer is small enough that you won't notice.)

## Pinning apps

Click the **+ Add app** tile in the grid. A native file picker opens — navigate to any `.exe` and select it. It's added to the grid immediately with its real icon extracted straight from the executable.

There's no limit on how many apps you can pin. If you pin more than fit on one screen, the grid scrolls (see [Searching](#searching) below for the mouse/keyboard scroll behavior).

## Opening and closing the drawer

| Action | How |
|---|---|
| Open | Click the pinned taskbar icon |
| Close | Click the pinned icon again, click anywhere outside the drawer, press `Esc` on an empty search box, or click a tile to launch it (closes automatically after launching) |

Closing genuinely exits the program (it doesn't linger in the background), and opening launches it fresh — so reopening works every single time, with no way for it to get "stuck".

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

Each category is a genuinely separate `.exe` with its own independent list of pinned apps, and clicking a category's taskbar icon always opens that specific category. Like the main drawer, a category only runs while it's on screen, so having several pinned costs nothing while they're all closed.

## Multiple monitors and DPI

Each time it opens, the drawer positions itself flush with the taskbar on whichever monitor your cursor is currently on - so it follows you across screens rather than always appearing on one fixed monitor. It's also DPI-aware, rendering at the same physical size whether that display is scaled to 100%, 150%, or 200%.

## Uninstalling

1. Close the drawer (it exits on its own once closed — there's no background process to end).
2. Unpin it from the taskbar (right-click the pinned icon → **Unpin from taskbar**).
3. Delete the folder you unzipped it into.
4. Optionally, delete its config folder: `%LOCALAPPDATA%\OnyxLauncher\` (this removes your pinned-app list and any categories you created).

## Troubleshooting

**The drawer won't open.**
Because it exits fully whenever it closes, a "won't reopen" situation shouldn't happen — but if it ever does, open Task Manager's **Details** tab and end any stray `onyx-launcher.exe`, then launch again. Also confirm your taskbar pin points at the exe where you actually unzipped it (if you moved or deleted that folder, the pin is broken — re-pin from the current location).

**I can't find its icon in the taskbar to pin it.**
The drawer never shows a taskbar button for its own window (see [First run](#first-run)), so there's nothing to right-click while it's running. Pin the **file** instead: right-click `onyx-launcher.exe` in File Explorer → **Pin to taskbar** (use **Show more options** first if you don't see it directly).

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

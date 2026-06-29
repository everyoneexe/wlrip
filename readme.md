# wlrip

simple cs2 aimbot and esp, for linux only. native wayland.

> [!NOTE]
> wlrip is based on [avitran0/deadlocked](https://github.com/avitran0/deadlocked)
> by avitran0 (which itself tracks the `rdbtCVS` upstream). All credit for the
> core implementation goes to the upstream authors; wlrip is licensed under
> GPL-3.0, the same as the original.
>
> The original targets X11 and only runs on Wayland through XWayland. **wlrip
> reworks the overlay and GUI to run as pure native Wayland clients** — no X11,
> no XWayland — and adds an autowall/penetration system, multipoint head aiming,
> and a much tighter ESP latency path.
>
> See [Differences from upstream](#differences-from-upstream) for the full list.

## Differences from upstream

Compared to [avitran0/deadlocked](https://github.com/avitran0/deadlocked):

| Area | upstream | wlrip |
| --- | --- | --- |
| Display server | X11 / Wayland via XWayland | **native Wayland** (no X11, no XWayland) |
| Overlay | X11 window | **`wlr-layer-shell`** layer + click-through, EGL/glow/egui |
| GUI | X11 window | native **`xdg-shell`** window with client-side decorations |
| ESP latency | render-rate bound | **2 ms view-matrix loop + decoupled render thread** |
| Autowall | — | **thickness-based penetration check** (`penetration.rs`) |
| Head aiming | single bone-center point | **multipoint** — aims at the visible part of a peeking head |
| HiDPI | — | overlay renders at physical resolution, lines up on scaled outputs |

### Native Wayland

wlrip replaces the X11-based rendering with native Wayland:

- **Overlay (ESP):** drawn through `wlr-layer-shell` on the overlay layer, with
  a click-through input region. Renders via EGL + glow + egui.
- **GUI (settings menu):** native Wayland window (`xdg-shell`) with client-side
  decorations (sctk-adwaita), so it has a title bar / close button even on
  compositors that don't do server-side decorations.
- **No X11 at all:** all `x11` cargo features were removed from `winit`,
  `egui-winit`, `glutin` and `glutin-winit`. The binary no longer links or
  connects to X11/XWayland. Verified on niri (which has no XWayland).
- **HiDPI / fractional scale:** the overlay tracks the output scale and renders
  its buffer at physical resolution, so ESP lines up with the game on scaled
  monitors.
- **Window rule targeting:** the GUI sets the app_id `wlrip`, and the ESP
  overlay uses the layer-shell namespace `wlrip-esp`.

### Low-latency ESP

Because this is an external overlay (no game hook), ESP freshness is bounded by
how often the view matrix is read and how fast the overlay redraws. Two changes
keep ESP tight during fast camera turns:

- **2 ms game loop:** the thread that reads the view matrix and entity data runs
  on a 2 ms cycle (`data()` costs <1 ms), so the shared snapshot the overlay
  draws is always current.
- **Decoupled render thread:** the overlay redraws on its own loop at the
  configured `fps` (default 240) with vsync off, instead of being pinned to a
  lower rate.

> [!NOTE]
> The remaining lag is the compositor's own present cycle (+1 frame), which is
> unavoidable for any external overlay on Wayland. Closing that gap would
> require an internal (injected) overlay, which this project deliberately avoids.

### Autowall (penetration)

`penetration.rs` estimates whether the held weapon can shoot through the
geometry between you and a target. It casts a ray through the physics BVH
(`cast_walls`), sums the solid thickness, and applies per-weapon penetration
power and damage falloff. When **autowall** is enabled, the aimbot will still
target an enemy behind penetrable cover.

> [!NOTE]
> Surfaces are currently treated as a single default material. Per-surface
> material classification (concrete vs glass vs metal) is scaffolded
> (`surface_name_to_index`) but not yet wired to the geometry.

### Multipoint head aiming

Instead of aiming at the single head-bone center, the aimbot samples several
points around the head (center + up/down/left/right, on the plane facing you)
and aims at the closest one with a clear line of sight. This targets the exposed
part of a head peeking around cover instead of the (possibly occluded) center.
Controlled by `multipoint` and `multipoint_radius` in the aim config.

### Requirements

A Wayland compositor that implements **`wlr-layer-shell`**. This covers most
compositors: Hyprland, Sway, niri, river, Wayfire, KDE KWin, and others.

> [!IMPORTANT]
> **GNOME (Mutter) is not supported** — it does not implement
> `wlr-layer-shell`, so the overlay can't be created. The GUI still opens, but
> there will be no ESP. Use a layer-shell compositor instead.

## Setup

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
git clone https://github.com/everyoneexe/wlrip
cd wlrip
./setup.sh
# Restart your machine (required)
```

Also make sure that the `uinput` kernel module is loaded.

Running NixOS or Fedora Atomic? See [OS-Specific Setup](os-setup.md).

## Running

```bash
./run.sh
```

## Features

### Aimbot

- Hotkey
- FOV
- Smooth
- Start bullet
- Targeting mode
- Visibility check (VPK parsing)
- Autowall (thickness-based penetration check)
- Multipoint head aiming (aims at the visible part of a peeking head)
- Aim jitter (randomizes the aim point to soften the lock)
- Head only/whole body
- Flash check
- FOV circle

### ESP

- Hotkey
- Box
- Skeleton
- Health bar
- Armor bar
- Player name
- Weapon icon
- Player tags (helmet, defuser, bomb)
- Dropped weapons
- Bomb timer

### Triggerbot

- Activation mode
- Min/max delay
- Additional Duration
- Refire delay (randomized cooldown after each shot)
- Visibility check
- Flash check
- Scope check
- Velocity threshold
- Head only mode

### Standalone RCS

- Smoothing

### Per-Weapon Overrides

- Aimbot
- Triggerbot
- RCS

### Misc

- Sniper crosshair
- Bomb timer
- Bomb ESP (C4 location, time-colored, with a defuse race indicator)
- Shot log (headshot ratio and a best-effort risk indicator)
- Stream-proof panic key (hotkey that instantly hides the whole overlay)

### Unsafe

> [!WARNING]
> These features write to game memory and might get you banned.

- No flash (with max flash alpha)
- FOV changer
- No smoke
- Smoke color change

## FAQ

### Which desktop environments and window managers are supported?

Any Wayland compositor that implements **`wlr-layer-shell`**:

**Supported:**

- Hyprland
- Sway
- niri
- river
- Wayfire
- KDE (KWin)

**Not supported:**

- GNOME (Mutter) — no `wlr-layer-shell`, so no overlay. The GUI still opens.
- Pure X11 sessions — wlrip is Wayland-only. Use the
  [upstream](https://github.com/avitran0/deadlocked) for X11.

### The overlay doesn't show up

Your compositor probably doesn't implement `wlr-layer-shell` (e.g. GNOME).
The log will say `failed to create wayland overlay`. Use a layer-shell
compositor instead.

### The ESP is offset or the wrong size

The overlay renders at the output's physical resolution. If you run the game
through Gamescope, the game may report a 16:9 resolution that doesn't match the
real output, throwing off the projection. Try running without Gamescope.

### My screen/overlay is black

Your compositor doesn't have transparency enabled. On KDE, go into
`Display and Monitor` settings, then `Compositor`, and tick
`Enable compositor on startup`.

### Where are my configs saved?

Configs are saved in `$XDG_CONFIG_HOME` with fallback to `$HOME/.config`. Otherwise they're saved alongside the executable.

### Will I get banned?

No one can promise you won't. This is a cheat for a VAC-protected game, so there
is always risk. wlrip is **external** (it reads game memory from another process
and moves the mouse through `uinput`) — it does not inject into the game or hook
its rendering, which keeps the client-side detection surface low. But most bans
are *behavioral*: VACnet and Overwatch look at how you play, not just your
memory. Inhuman flick speed, pre-aiming through walls, and obvious ESP plays get
you reported and reviewed regardless of how the tool is built. VAC also bans in
retroactive waves, so being undetected today is not a guarantee. Keep aim smooth,
keep your FOV small, don't be obvious, and don't use it on an account you can't
afford to lose.

### The ESP still lags behind during fast turns

A small amount of lag is unavoidable for an external overlay (see
[Low-latency ESP](#low-latency-esp) — the compositor adds one frame of its own).
The biggest lever you control is the overlay's render rate: raise `fps` in your
config (default 240) toward or above your monitor's refresh rate. vsync is
already off, so a higher `fps` directly tightens how fresh the drawn snapshot is.

### The aimbot shoots the wall when an enemy is behind cover

Two settings help, depending on what you want:

- **Multipoint** (`multipoint = true`) makes the aimbot target the *visible*
  part of a head peeking around a corner instead of the (occluded) head center.
  Tune `multipoint_radius` (world units, ~3.0) if it over/under-shoots the edge.
- **Autowall** (`autowall = true`) lets the aimbot target an enemy through
  penetrable cover, using a thickness-based penetration estimate for the held
  weapon. It won't target through cover the weapon can't actually shoot through.

## License

GPL-3.0, inherited from [avitran0/deadlocked](https://github.com/avitran0/deadlocked).
See [license](license). If you distribute wlrip or a build of it, you must keep
it under GPL-3.0 and make the source available.

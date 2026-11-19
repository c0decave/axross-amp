# Changelog

axross-amp is axross's Media Player as a program of its own, in Rust. The
versions before 0.14.0 were not published; they are listed so the history
reads whole.

## 0.14.0 - 2026-11-19

The first public release.

Not 1.0: its latest independent review scored 6.5 of 10, the HDMI /
DisplayPort sound paths of "Show on display" have been checked against a
fake sound server only, and the flash guard does not hold 60 frames a
second on the shape-heavy faces (see *Known limitations*).

### Added
- **Ten interface designs** (⚙ → Design), each a layout of its own over the
  same player: Modern, Minimal / Zen, Hi-fi rack, 90s desktop, Cassette deck,
  Terminal, Brutalist, Glass, Soft, Collage. Every control in every design at
  420 x 420, 800 x 600 and 1920 x 1080, Tab in the order they are seen.
- **Show on display**: the pictures full screen on another monitor, with the
  sound moved to its HDMI / DisplayPort output (matched by the display's own
  name) and put back - output and card profile - when the view closes,
  after a restart, a crash or an unplugged display.
- **Wind 15 s back and on** (the keys J and L, two buttons beside play).
- **Twelve window themes** on design tokens; following the desktop's light
  or dark (XDG portal, GNOME, GTK).
- A fourth reading of the flash meter: red from the gamma-encoded values.

### Changed
- **No saturated red in any face**, whatever the colour scheme: Classic's
  and Fire's reds, a cover's, a custom one are taken toward a warm white,
  a red ground darkened and turned from red.
- A focused control keeps its keys (the arrows move a focused volume
  slider, not the position too).
- Paused stays paused: Next, Previous and seeks past the end (keys, MPRIS).
- The Options' extremes are bounded (smoothing at least 75 %, peaks falling
  at most 100 % a second); the terrain, beat rings and starfield calmed at
  them; the ring calm at a slow renderer's 12 frames a second.

### Fixed
- File names in bytes that are not UTF-8 (from the command line too).
- The theme of one window no longer leaks into another (tests, galleries).
- Cover accents keep 4.5 : 1 on hover, pressed and chosen rows.

### Known limitations
- The flash guard costs the CPU about 1.5 - 2 ms a frame, and 5 - 9 ms on the
  particles, the spirograph and the skyline at 4K: 60 frames a second is not
  supported for those faces on a laptop's CPU.
- Glass, Soft and Zen share the Modern design's arrangement (Zen in one
  column); their difference is their surfaces.
- "Show on display" with the sound has been tested against a fake pactl
  only, not on real HDMI hardware; on Wayland the compositor places the
  view.
- egui-file-dialog is younger than the 14 days the supply-chain gate asks
  until 2026-11-23.

## 0.13.0 - 2026-11-18 (not published)
Photosensitivity first: both flash meters enforced, a pattern stage in the
guard, the sweep under the stricter reading; window themes.

## 0.12.0 - 2026-11-17 (not published)
The whole sweep under a stricter meter; the remaining faces calmed.

## 0.11.0 - 2026-11-17 (not published)
An output-level flash guard; a flash meter; contrast; wide windows.

## 0.5.0 - 0.10.0 - 2026-11-16 / 17 (not published)
Undo, play next, fades and a limiter; 48 faces; the README's pictures from
real playback.

## 0.1.0 - 2026-11-16 (not published)
The player in Rust: symphonia and cpal, a ten-band equalizer, playlists and
internet radio, the visualizations, the Mixer and Bluetooth tabs, axross's
state shared.

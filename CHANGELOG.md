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
- **Started from a terminal it goes to the background** (Linux, macOS): the
  prompt comes back, the player goes on after the terminal closes, a start
  that fails is said; `--foreground` / `-F` keeps it in the terminal.

### Changed
- **No saturated red in any face**, whatever the colour scheme: Classic's
  and Fire's reds, a cover's, a custom one are taken toward a warm white,
  a red ground darkened and turned from red.
  The faces still move as calmly as the scheme as chosen asks (Fire as
  red): the ridges in Fire had come to 11 - 12 by a 100 ms reading, as
  before 0 again.
- The line spectrum's peaks are drawn faint and its curve falls eased: the
  peaks falling all along the width with the edge swept a field ten times
  a second by a 100 ms reading (Fire, Intense 7, Classic 10; now 3 and 6).
- A focused control keeps its keys (the arrows move a focused volume
  slider, not the position too).
- Paused stays paused: Next, Previous and seeks past the end (keys, MPRIS).
- The Options' extremes are bounded (smoothing at least 75 %, peaks falling
  at most 100 % a second); the terrain, beat rings and starfield calmed at
  them; the ring calm at a slow renderer's 12 frames a second.

- At a slow renderer's rate (under 40 frames a second) the pictures step
  at most about half the way a frame: a sweep at 12 frames a second over
  every scheme passes.
- Show on display: the card's profile before is kept before the card is
  switched and put back also when the view is closed while the sound is
  still moving; the display's name on the card's port is understood; a
  profile on already is never switched again; two displays of one name get
  no sound by guessing; the 90s desktop's title rests while paused.

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
- By a review's stricter 100 ms reading (motion over a field counted too),
  34 of 196 cases at 60 frames a second in Classic and Fire read over 6
  (the wave ribbon most, 41); at 12 frames a second none.
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

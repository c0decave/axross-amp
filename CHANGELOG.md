# Changelog

axross-amp is axross's Media Player as a program of its own, in Rust. The
versions before 0.14.0 were not published; they are listed so the history
reads whole.

## 0.14.3 - 2026-11-20

What independent review 12 (8.0 of 10) asked for. (0.14.2 was not
released; its changes below come with this one.)

### Fixed
- **The shared state file** (media_player.json): a file name that is no
  UTF-8, which axross writes as Python's surrogate escape ("\udce9"),
  made the whole file unreadable - the player started empty and its next
  save replaced axross's playlist. Such names are read as their bytes, any
  other odd escape costs that string alone, and they are written back as
  axross writes them.
- **No blink on a track change** (the flash rule): Next and Previous no
  longer clear the cover, the Glass backdrop and the faces' thumbnail;
  the old ones stay until the new track's come, a new cover is shown at
  most twice a second (the last one asked for while skipping) and they
  crossfade. Holding N read up to 30 transitions a second; now 1.
- **The stereo scope with trails and a glow** in white on black through
  gated stereo noise read 10 transitions a second, Calm too: its glow is
  as bright as the figure, none on a faint tangle.
- **The flash guard** veils a flicker that starts within the rule and
  speeds up (3 Hz to 4, 2 rising to 5) before its seventh transition in a
  second: from the fifth when it comes at 2.6 Hz or quicker.
- **"Off"** is crossfaded and guarded as any face (switching a face and
  off quickly read 15 transitions a second).
- **The picture's own window** takes clicks and keys as the picture in
  the main window: a click the next face, the right button the face menu,
  a double click or F11 full screen, Escape out of it.
- **Playlists**: CR line ends, Windows-1252 (Winamp), `file:/path`, and a
  PLS in its entries' order (File10 after File3).

### Changed
- The windows have the program's icon.
- README: the hero picture, the punchlines and the contents first, the
  other pictures after them; macOS 15's System Settings → Privacy &
  Security → Open Anyway (right-click → Open no longer passes Gatekeeper).
- LIMITATIONS: the flow field in Classic *Intense* shows a pattern for a
  few frames.
- Tests: the flash tests read with review 12's own meter too; the frame
  costs are the drawing thread's CPU time; the meters, the sound and the
  window tests go by the sound, not the clock (robust on a busy machine).

## 0.14.2 - 2026-11-20

What independent review 11 (7.5 of 10) asked for.

### Fixed
- **The stereo scope** through stereo unlike left and right (noise, a wide
  mix): a new tangle every frame, 33 transitions a second by the review's
  meter - now drawn faint (0); its 0.4 - 0.5 s frames at 1080p (the flash
  guard laid its lines' miters along millions of cells) - 5 ms.
- **The flash guard** veils a flicker of eight a second or more within
  its first three flashes (it let four or five through); its pattern
  stage acts in the frame it sees a pattern.
- **Faces over the project's own limits** through the review's music:
  the LED equalizer, the particles, the spectrum, the mirrored spectrum
  and the bell curves fall eased (half the height a second); the piano
  roll draws no notes for noise; the oscilloscope dims a dense wave
  sooner in the starkest schemes; the wave ribbon's twist is limited in
  speed. Two cases remain over the strict reading in *Intense*, said in
  FLASH_SAFETY.md.
- A file whose **sample rate** is 0 or 1 Hz is refused and skipped (it
  showed "Playing 0:00" for ever, 560 MB at 1 Hz).
- **M3U**: names starting with '#' or a space are saved as file:// URLs
  and read back; a playlist over 1 MiB is read (its first 10,000
  entries), not refused.
- An unreadable **media_player.json** (or audio.json, axross-amp.json) is
  kept aside as `<name>.unreadable-<time>-<hex>` before anything replaces
  it, as the Python player does.
- **Wide and low windows**: the 90s desktop docks its playlist, the
  terminal and Zen put it beside the player (their pictures were strips).
- **MPRIS** Seek asked while the first track still opens is done once it
  is open (Seek(i64::MAX) left it Stopped).
- A **cover** is decoded within limits (an 11000 x 11000 PNG took 552 MB).
- Kinetic typography keeps its words in the picture.
- **The meters** (loudness, true peak, beat, key) hear every frame while
  their thread gets no turn for up to 5.5 s on a busy machine (1.4 s at
  192 kHz; it was 1.4 s and 0.34 s).

### Changed
- **Release**: Linux built on a glibc 2.34 floor (AlmaLinux 9) and
  checked; macOS one universal binary (arm64 and x86_64, neither left out
  quietly) also as axross-amp.app in a DMG; the supply-chain gate's axross
  pinned to v0.15.180.
- **The picture in a window of its own** (⚙), as the Python player's
  Detach.
- "Follow the theme" follows each of the twelve themes.
- A very large window (2200 points and more) is used, not a column of
  1180 points; the bars above and below take the same width.
- With the pictures off the window wakes once a second; on a software
  renderer it keeps within 0.35 of a core.
- The flash test music has stereo unlike left and right, antiphase and
  30 ms bursts; the review's own music is in the tests too.
- README: the screenshots first, then the punchlines, then the contents.
- `--help` names --version, F11, Esc and Ctrl+F.
- The tunnel's walls have panels and a lit side (on a light ground the
  panels fade out and its checker keeps a fifth of its contrast: black on
  white stays within the strict reading).


What independent review 10 (8.0 of 10; no flash or red problem found)
asked for.

### Fixed
- **Show on display**: quitting with the view open (the close button,
  SIGTERM, Ctrl+C in its terminal) puts the sound and the card's profile
  back before the program ends - the card was left on HDMI and the
  analog output gone until the next start; the next start opens the view
  again and switches once more. SIGTERM and SIGINT close the window as
  its close button does.
- Two monitors of one name, the one remembered gone by its connector:
  the view is not opened again on the first of them by guess (nor by D);
  it is said, and asked.
- The cassette deck's label at 420 x 420 and 1400 x 460 wrote the title
  over the artist: one line there. The collage's title scraps no longer
  lie over its artist line in a low window. The terminal at 420 x 420
  pushed Clear and the durations out of the playlist's box: the
  playlist's switch row shortens, then puts its actions on a row of
  their own, where they do not fit.
- Wide and low windows (900 x 400, 1600 x 420, 1400 x 460): Modern, Glass,
  Soft, the rack and the collage put the playlist beside the player, as
  the cassette and the brutalist did - the picture was a strip some
  10 - 28 points tall.
- The wave ribbon in the starkest schemes (black on white at 960 x 540: a
  fast-oscillating grating, 40 by review 10's reading) twists and opens far
  slower: 4.
- The docs said "toward a warm white" (a warm grey as light as the colour
  is), "exits 1" (a start still going after two seconds exits 0 and says
  so), ~/.cache on macOS (~/Library/Caches), that the AppImage "just runs"
  (from a terminal it goes to the background), that Glass, Soft and Zen
  share Modern's arrangement; FLASH_SAFETY.md disagreed with itself (three
  or four readings, 12 or 14 cases at six) and lacked the 100 ms results.

### Changed
- **Glass and Soft have layouts of their own**: Glass in two columns (a
  tall frosted pane of what plays beside the picture and the playlist),
  Soft centred (the picture pressed in, a raised card of keys around a
  large play key, the playlist's card).
- **Windows**: a GUI program (no console window beside it); started from
  cmd or PowerShell it writes --help, --version and a failed start to
  that terminal (windows-sys, already in the lock, as a direct
  dependency).
- **Release archives** carry LICENSE, THIRD_PARTY_LICENSES.md (every
  crate's licence and texts, egui's fonts' OFL and Ubuntu Font Licence
  too, made from Cargo.lock by scripts/third_party_licenses.py and kept up
  to date by a test) and the README; on Linux also the desktop entry and
  the icon (the AppImage has the licences in usr/share/doc).
- **The release waits** for the supply-chain gate and the tests (ci.yml,
  called) and attests its files' provenance
  (actions/attest-build-provenance, pinned by commit); every action is
  pinned by its commit.

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
  and Fire's reds, a cover's, a custom one are turned toward a warm grey
  as light as they are (a custom scheme's red, and the colours some faces
  mix from the scheme while they draw, toward a warm white), a red ground
  darkened and turned from red.
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
- Glass and Soft drew Modern's arrangement in their own surfaces (fixed in
  0.14.1: each has a layout of its own).
- "Show on display" with the sound has been tested against a fake pactl
  only, not on real HDMI hardware; on Wayland the compositor places the
  view.
- By a review's stricter 100 ms reading (motion over a field counted too),
  34 of 196 cases at 60 frames a second in Classic and Fire read over 6
  (the wave ribbon most, 41); at 12 frames a second none.
- The flash guard dims the line face in very fast music more often than
  needed (it over-estimates the moving fill); nothing flashes, the picture is
  just darker at times.
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

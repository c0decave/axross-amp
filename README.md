# axross-amp

axross's Media Player as a program of its own - written in Rust.

![The window](docs/window.png)

<sub>The track in the picture is a demo synthesized for it (`docs/make_demo.py`); the
picture is made by `AMP_DEMO_TRACK=… cargo test --test ui -- --ignored`.</sub>

- **Plays** local files (MP3, FLAC, Ogg Vorbis, Opus, AAC / M4A, ALAC, WAV, AIFF, CAF;
  MP4 / MKV containers), folders and M3U / PLS playlists, and **internet radio** by
  its http(s) URL - decoded by [symphonia](https://github.com/pdeljanov/Symphonia)
  (Opus by libopus, built in), played through [cpal](https://github.com/RustAudio/cpal)
  (PulseAudio / PipeWire or ALSA on Linux, CoreAudio on macOS, WASAPI on Windows).
- **Gapless**: the next track is opened before the current one ends and decoded on
  into the same buffer - not a sample of silence between the tracks of an album;
  AAC / ALAC in MP4 are trimmed by their edit list (encoder delay and padding).
- **Sound**: resampled to the output's rate by a band-limited (windowed-sinc)
  resampler; 5.1 / 7.1 downmixed as ITU-R BS.775 has it; **ReplayGain** (track or
  album gain, pre-amp, held below the peak, a gain of its own for untagged tracks;
  Opus R128 gains too); a ten-band **equalizer** with axross's presets; a volume curve
  as axross's; pause, seek, stop and a new track fade (no clicks), and so does a
  dropout's edge. At unity gain the samples go out bit for bit (a master peaking just
  under full scale is not bent); a track ReplayGain makes louder goes through a
  look-ahead true-peak limiter (-1 dBTP, 4 ms ahead, its gain eased in and out)
  instead of clipping. The output has one too, always in the path (4 ms later, bit
  for bit where it holds nothing back), holding the peaks at -0.3 dBTP wherever the
  sound may go over full scale: a track resampled to the output's rate (its peaks
  between samples become samples), two tracks summed in a crossfade, the equalizer
  raising a band. The output's finds the peaks between samples 8x oversampled (a
  64-tap windowed sinc; it reads clipped, bright masters right where BS.1770's 4x
  filter reads them up to 0.8 dB low), aims 0.5 dB under its ceiling, holds from
  0.3 s before a crossfade, and where it holds nothing it neither looks nor scans:
  its cost is a copy. An optional **crossfade** (1 to 12 s, off by default) fades a
  track into the next - its law following how alike the two are (equal power for
  unrelated tracks, equal amplitude for alike ones: the level stays) - never inside
  an album, which stays gapless: tagged, the album (and album artist) decides;
  untagged, two files of one folder numbered one after the other ("01 ...", "02
  ..." or track numbers) are an album's; a flat folder of unrelated files fades. Each
  track keeps its own ReplayGain; the meters count the new track from the fade's
  middle. Masters that themselves go 3-4 dB over full scale between their samples
  are held at the ceiling in a fade at the cost of ~3 dB of level there (a gain
  limiter cannot do otherwise without clipping; outside the fade they play bit for
  bit). Optionally a track chosen while
  another plays fades in over it for a second. The output callback takes no lock and
  allocates nothing (a lock-free ring;
  a test counts its allocations through a real device); files are read and decoded
  seconds ahead on threads of their own, so a slow disk or a stalled network share is
  no dropout. A small output buffer is asked for (1024 frames); the position and the
  pictures follow what the speakers play, not what was decoded.
  An output that goes away (a headset switched off) is opened again; one chosen that is
  not there gives way to the default; without any, nothing pretends to play and the
  window offers Retry.
- **Internet radio**: what the station plays now (ICY `StreamTitle`), its name and
  genre; a radio that breaks off is connected again; a `.pls` / `.m3u` URL from a
  radio directory is resolved (only its http(s) entries); old SHOUTcast servers
  (`ICY 200 OK`) play. The network is read on threads of its own: a server that
  stalls holds up nothing (a read gives up after 20 s; a stream let go closes its
  connection at once). HLS (`.m3u8` with segments) is not supported and says so.
- **The window** is laid out round the picture: what plays on a card tinted with the
  cover's colour (title, artist and album in a type scale of their own; the accent of
  the whole window comes from the cover, kept readable), the picture large - as large
  as a short playlist leaves room for - and below it the playlist (two lines a track,
  the playing one with its cover and bars that move with the sound) or the equalizer
  (a dB scale, each band filled from its 0 dB line). The card's cover is 120 to 200
  px as the window allows (wider than 900 points, a hero of up to 320 px on a
  soft shadow, what comes next beside it); the cover (kept up to 1024 px) is made the size it is
  shown at, in pixels, on a thread of its own by a Lanczos filter and cropped to a
  square (egui's renderer makes no mipmaps: made smaller by the GPU it aliased).
  The "Cover" face makes the cover the picture, glowing with the bass (without a
  cover, a record in the scheme's colours). The pictures' colours come from the
  cover too, when it has some (or a scheme of axross's, below): not its colours
  mixed (that made browns and olive greys) but a ramp designed in OKLCH round its
  main hue - and its second, where it has one - lightness rising evenly, a floor of
  chroma; the pictures are dithered (no bands in slow gradients). Nothing to play: a drop zone with
  Open files, Open folder, Add a URL and the keys. Paused, the picture stands still;
  changing faces fades one into the next.
- **Visualizations**, 49 of them in three groups:
  - *Classic* - all eighteen of axross's, drawn as axross draws them (its geometry,
    colours and constants; [side by side](docs/axross-and-amp.png)): spectrum,
    mirrored, radial and polar spectrum, line spectrum, LED equalizer, particle
    columns, waterfall, ridgelines with their sun, spectrum terrain, 3D ring, bell
    curves, wave ribbon, kaleidoscope, oscilloscope, stereo scope, VU meter,
    starfield. Two differ by default: the waterfall is smooth, at high resolution (256
    log bands of the fine spectrum, as foobar2000's spectrogram; axross's bars are an
    option), and the two scopes find their gain (quiet or near-mono music still fills
    them). Trails and glow for the starfield and the stereo scope are an option.
  - *Analysis* - a spectrum analyzer (8192-point FFT, a held peak curve, the
    strongest peak with its note and cents, the note and level under the pointer),
    a loudness meter after EBU R128 (momentary, short-term, integrated with both
    gates, loudness range, true peak by BS.1770's own filter, the last minute, a
    target of -23, -16 or -14 LUFS), phase and stereo width (correlation, width,
    balance, the last ten seconds), a goniometer with a phosphor's persistence, the
    track's waveform (read in the background; a click goes there), a chromagram with
    the circle of fifths and the whole track's key (with how sure, "?" until then), a
    piano roll (notes found at two resolutions, harmonics told from notes) and a mel
    spectrogram in magma.
  - *Generative* - the cover, liquid (domain-warped noise), fire, metaballs, aurora,
    flow field, synthwave, a tunnel of glowing lines, galaxy, Julia set (smooth escape
    colours, orbit traps), reaction-diffusion, a feedback face in MilkDrop's manner
    (its last frame warped under a new layer; eight looks of its own - rings, a
    scope's line, spiral arms, orbits, a rose, spokes, mirrored in two or four -
    a new one each phrase of 16 beats; it reads no MilkDrop presets), a flyover of a raymarched night landscape the bass lifts, crystals
    (Voronoi cells), a skyline (searchlights, traffic, roofs lit by their bands),
    isometric bars, DNA helix, spirograph, polygon morph, moiré (antialiased), beat
    rings, smoke and the title's words on the beat.

  The loudness, the true peak, the beat and the notes are measured on a thread of
  their own from every sample played - minimised, on another tab or with the picture
  off too. The pixel faces are WGSL shaders run through egui's wgpu renderer. Where
  the renderer is slow (a software rasterizer such as llvmpipe: measured at the start,
  by the CPU time a face costs either way) the window is drawn at most 12 times a
  second, fewer where a frame costs more (a large window: down to 3), so that it
  takes about 0.6 of a core; the cover face 4 times; the shader faces in at most
  about 90,000 pixels, scaled up, or by a thread of the player's own per pixel,
  whichever measured cheaper. At rest nothing is drawn (no frames, no work) and the
  last picture stays. The picture is analysed once a
  frame for all faces, and neither that nor most faces allocate in a frame; the
  pictures move alike at any frame rate. In axross's colour schemes (Classic, Fire,
  Ice, Neon, Mono, Follow the theme, Custom - edited here), with its options (bars,
  smoothing, peak fall, frame rate). A click shows the next face (Shift+V the one
  before), a double-click makes it large (its title kept above it, with "Show the
  playlist" - the list under the picture - and "Normal size"; Escape too), F11 full
  screen, a
  right click its menu with every face at a glance; "Off" draws nothing.
  Escape closes only the innermost thing, one at a time: a track's details,
  the URL field, the warning or a field being typed in before the large view
  or full screen.
- **Themes** for the window (⚙ → Theme), twelve, each a set of design tokens
  (grounds, cards, text, secondary text, the accent with its hover and pressed
  shades, borders, the focus ring, good / warning / error, a slider's rail,
  selection, shadow, corners): *Midnight* (the default), *Paper* (warm white,
  ink-blue), *Frost*, *Dusk* (Solarized at sundown), *Night drive* (violet,
  magenta, a little cyan), *Forest*, *Espresso*, *Ocean*, *Sakura* (light pink,
  plum), *Studio* (greyscale, one orange signal), *Daylight* (light) and *High
  contrast* (black and white, yellow for what is chosen, a 3 px focus ring, 7 : 1
  text). The pointer on one in the menu shows it at once; a click keeps it (in
  axross-amp's own file). Optionally the desktop's light or dark decides, between
  a light and a dark theme of one's choice. The accent comes from the cover (made
  to read in the theme: lightened or darkened until it does) or is the theme's
  own. A test checks every theme's text, secondary, disabled and selected text,
  accent, good / warning / error, the focus ring and the slider against every
  ground they lie on (WCAG AA, 4.5 : 1 and 3 : 1; AAA, 7 : 1, in high contrast).
  A theme changes the window, never the pictures: the faces keep their own
  colour schemes (a theme only suggests one) and so every limit on flashes.
  Switching costs one frame; nothing a frame.
- **The window**: wider than 1240 points the player is a centred column
  1180 wide. Text on the cover's colour (the chosen row, the buttons) is light
  or dark, whichever reads at 4.5 : 1 or more (WCAG 1.4.3) - tested over 144
  cover hues; the cover's colour is lifted until it does against the ground
  and the cards.
- **Playlist**: shuffle (Previous goes back the way it came), repeat (all / one), a
  filter (Ctrl+F), sorting (title, artist, album with its discs and track numbers,
  file name, folder, length, reversed; names in natural order), several rows at once
  (Ctrl- / Shift-click) to remove or drag, reordering by keys, **play next** (from a
  row's menu; queued rows are marked), **undo** of a removal, clearing, sorting or
  reordering (Ctrl+Z), drag and drop of files and folders from the desktop (walked in
  the background; a file already in the list is not added twice), saving as M3U8. Titles,
  lengths, artists and albums are read in the background; a track that cannot be
  played is marked red with why; the details of a track (tags, format, ReplayGain) in
  a window of their own. Two or more tracks of one album in a row (one folder) come
  under a header with the album's cover, artist ("Various artists" where they
  differ), its tracks and length. At most 10,000 tracks - axross's player reads no more.
- **Sleep timer** (15 to 90 minutes with a fade, or at the end of the track).
- **The desktop** (Linux): MPRIS 2 on the session bus - media keys, a headset's
  buttons, the desktop's sound menu and lock screen show and control it, also while
  the window is minimised. One player: a second start hands its files over. A desktop
  entry and an icon are in `packaging/`. The window title says what plays; the window
  opens as large as it was left.
- **The system mixer** (Mixer tab): this player's output device, volume, mute and
  equalizer; the sound server's outputs and inputs (volume, balance kept, mute, port,
  default), the applications playing and recording (volume, mute, which device) and
  the cards' profiles (a Bluetooth headset: A2DP or headset) - through `pactl` of
  PulseAudio or PipeWire, live as they change elsewhere; volumes follow a slider
  while it is dragged. Laid out as cards in sections (this player, outputs,
  inputs, applications, sound cards; two columns in a wide window), the
  player's card and the default device in the cover's accent.
- **Bluetooth** (Bluetooth tab, Linux): the adapter (power, scan for Classic and / or
  BLE devices), the devices with kind, battery, signal and state; Pair, Trust,
  Connect, Disconnect, Forget - grouped as connected, paired and nearby, each with
  its kind's sign - through BlueZ on the system D-Bus
  ([zbus](https://github.com/dbus2/zbus)). A pairing that asks (confirm a passkey,
  type a PIN) asks in a dialog; nothing is accepted without an answer. **Use as audio
  output** connects a speaker or headset and makes its output this player's.
- **Shares its state with axross**: the playlist, the track and its position, the
  volume, the visualization (`~/.config/axross/media_player.json`) and axross's one
  equalizer (`~/.config/axross/audio.json`) - the player inside axross and this
  program pick up where the other left off. Keys they do not know are kept. What
  only this program has (window size, ReplayGain, crossfade, the cover's colours, the face options) is in
  `~/.config/axross/axross-amp.json`, as axross rewrites its own file with its keys.
  Files are written on a thread of their own, merged with what axross's player wrote
  meanwhile (its playlist is not undone by a change of volume here).

![The classic faces](docs/faces-classic.png)

![The analysis faces](docs/faces-analysis.png)

![The generative faces](docs/faces-generative.png)

<sub>From the demo track, played in real time by `cargo test --test ui -- --ignored`
(see above); the sheets are made by `docs/faces_sheet.py`.</sub>

<p><img src="docs/empty.png" alt="Nothing to play: the drop zone" width="45%"> <img src="docs/mixer.png" alt="The Mixer tab" width="45%"></p>

<p><img src="docs/bluetooth.png" alt="The Bluetooth tab" width="45%"></p>

![The themes](docs/themes.png)

<sub>Every theme, playing (`cargo test --test themes -- --ignored`, which also
renders each in the window's other states into a contact sheet).</sub>

## Photosensitivity

**Warning.** The pictures move with the music, and moving, flashing pictures
and stripes can bring on seizures in people with photosensitive epilepsy -
some of whom do not know it. If you or anyone watching is sensitive to
flashing light or patterns, do not watch the pictures: turn them off ("Off"
in the faces' menu) or choose the cover.

What axross-amp does about it:

- **Two readings; the stricter is enforced.** Every rendered frame is read
  as WCAG 2.3.1 / 2.3.2 have it - a field changing over a quarter of its
  area the same way within 34 ms is a transition, a flash a pair of
  opposing ones - and by a stricter reading an independent review applies:
  within 100 ms, and every change counted, each rise and each fall, paired
  or not. A face passes only where both readings count at most six
  transitions in every second (saturated red too) and no pattern hazard;
  `tests/flash.rs` fails otherwise.
- **Calm faces.** Each face is meant to stay within both readings by
  itself, without the guard below. What drives the pictures' brightness
  (the bars, the beat's pulse, the bass, the energy, the middle, the
  treble, the VU meter, the oscilloscope's height, every band of the
  spectrograms) turns no faster than every quarter of a second - in
  *Intense* too, which is livelier by its quicker easing and pace. In the
  starkest schemes (white or pure red on black: measured by their colours'
  contrast with the ground and their share of saturated red) it turns up to
  1.6 times slower still, the LED and particle columns fall slower, and
  the flow field's lines are a shade under full. Pictures with seconds of
  history (the waterfall, the mel and the fine spectrograms, the piano
  roll) show each band as its slow mean and half its change from it,
  eased. Between bars and cells (spectrum, mirror, LED, particles, 3D ring,
  chromagram) lies a shade of their colour 0.075 dimmer, not the dark
  ground: no grating. Dense lines that would form one are faint (the
  polygon morph's outer copies, the helix's rungs, synthwave's fan and far
  rows, a noise-dense oscilloscope trace); the moiré's rings interfere at
  most 0.09 above the ground; crystals are lit evenly; the fire, the smoke,
  the beat rings, the helix, the polygon morph, the terrain and the
  starfield's trails are never saturated red; the onsets
  turn the kaleidoscope no faster than a petal in a third of a second; the
  tunnel's near rings (which sweep past fastest) are faint and the onsets
  twist it slowly; a new word of the typography comes at most every 0.75
  s; the beat zooms and turns the Julia set only a little; the terrain's
  dots lie on every other row, as loud as it is. The Cover face's cover is
  at most 30 % of the picture, its backdrop blurred.
- **A flash guard at the output**, as a backstop. After a face has drawn a
  frame, the guard estimates the picture on a grid of cells (128 across):
  the shapes laid on in the order they are painted (pieces side by side
  adding up to what they cover), a shader face by its own function
  evaluated on the CPU, the cover from its pixels. It follows each cell's
  relative luminance and saturated red (R, G and B linear), and each
  *field* - a third of the **screen** each way (the monitor's size; WCAG's
  10° view), slid over the face - in which a quarter of that field's area
  changes the same way within 100 ms, every change counted, as the strict
  reading does. Where a field has changed seven times in a second it lays
  a veil over the whole face - a slow, blurred average of the picture, 95
  % - staying on while the face goes on near the limit and a second and a
  half after. Seven, not six: the veil's own coming is a change too, and
  at six it would make the seventh. So the guard does not by itself hold a
  face to six; it caps what the faces do not foresee. On ordinary music - pop,
  house, drum and bass - it does not come (a test). It costs about 1.5 - 5.5
  ms of the CPU a frame (`--release`, `tests/frame_cost.rs`, the faces with
  and without it); more for the faces of thousands of pieces: the
  spirograph 7 - 10 ms, the particles 11 - 12.5 ms at 1920 x 1080 and 3840
  x 2160, the skyline 9 ms at 3840 x 2160. The frame rate's cap on a
  software renderer counts it in.
- **A pattern stage in the guard** (ITU-R BT.1702), for stripes no face
  foresaw - a cover of checks, a striped custom scheme: regular stripes
  in the estimate (five light-dark pairs or more in a row or column of
  cells, full periods alike, each step 0.1 of luminance) over a quarter of
  the picture while they move, or 35 % standing, are softened in place
  towards the mean of the cells round them (21 each way), by 95 - 98 %,
  until they no longer are. Stripes finer than two cells (about 1 % of
  the picture's width) are not seen by it. `tests/pattern_stage.rs`: a
  drifting checkerboard cover and black-and-white stripes of 6 px are a
  hazard in 120 of 120 frames without it, in 10 and 5 with it (the first
  tenth of a second, before it has seen them move).
- **Calm by default.** *Intense* (Options) is livelier and only chosen after
  a photosensitivity warning; where the desktop asks for reduced motion
  (GNOME, KDE, GTK's settings, macOS, Windows) it cannot be chosen and the
  pictures are calmer still.
- **Measured.** `tests/support/flashmeter.rs` measures rendered frames:
  per pixel (relative luminance; saturated red with R, G and B linear),
  any field a third of the picture each way (the picture taken as the
  whole screen: the strictest case), both readings above; regular stripes
  (five light-dark pairs or more, full periods alike, uneven gratings too)
  over a quarter of the picture while they move or drift (within a tenth
  of a second), or 40 % standing. `tests/flash.rs` renders every face and
  its options through music made to provoke them - kicks at 120 and 240
  BPM, gated noise, bass and treble in turn, fast sweeps, left and right
  in turn, bursts, silence and full scale - at 60, 30 and 12 frames a
  second; the faces with history through ten seconds each of steady music;
  the default faces through pop, house and drum and bass (the veil must
  stay off); the feedback face's every look, Intense, white on black and
  pure red; the moiré at its most (Intense, white on black); every case an
  earlier sweep found. With `--ignored`, the whole sweep: every face and
  option, Calm and Intense, in Classic, Neon, Mono, Fire and two extreme
  custom schemes - white on black, and pure red (#ff0000, peak #ff3030) on
  black - at 320 x 180: 624 cases, **all pass both readings** (each face last
  measured after its last change). The most by the normal reading: 4
  transitions a second, red 4; by the strict reading: 6, red 6 (12 cases at
  the limit, 11 of them in *Intense*: synthwave, the line spectrum, the bell
  curves, the crystals, the aurora; and the ribbon in pure red, Calm); moving
  stripes in no frame; standing stripes at most 39 % (40 % allowed). The
  faces that run faster as the music gets louder move as fast after 25
  minutes of it as at its start (a test: their clock times the energy had
  leapt further the longer they played).

Before and after, by the strict reading (the independent review's
numbers for 0.12.0; 0.13.0 from the sweep above, 320 x 180, worst music;
transitions a second, normal reading in brackets):

| | 0.12.0 | 0.13.0 |
|---|---|---|
| plasma, Intense, white on black | 9 - 17 | 1 (0) |
| kaleidoscope, Intense, white on black | 7 | 0 (0) |
| fire, Intense, pure red | 8 - 9 red | red 0 (0); 1 (0) |
| LED equalizer, Intense, Classic | moving stripes, 2 - 3 frames | none; 3 (1) |
| plasma, Intense, Neon | moving stripes, 2 - 3 frames | none; 0 (0) |
| moiré at its most (Intense, white on black) | - | 0 (0), no stripes |
| the whole sweep | 414 of 416 pass (normal reading, 4 schemes) | 624 of 624 (both readings, 6 schemes) |
| a drifting checkerboard cover, 6 px stripes (no stage / stage) | no pattern stage | hazard in 120 / 10 and 120 / 5 frames of 120 |

**What does not hold, or is not claimed.** The guard by itself does not
hold a face to six transitions a second by the strict reading: it veils at
seven (at six its own veil would be the seventh); the faces pass by
themselves, the guard is the backstop. Twelve cases sit at six, the limit.
The pattern stage acts after a tenth of a second (up to 10 frames of a
pattern first) and does not see stripes finer than about 1 % of the
picture's width. Measured on our music at 320 x 180 (the sweep), 384 x 216
(the suite) and up to 1920 x 1080 (some faces), on Mesa's lavapipe, not a
real GPU.

What is not claimed: the guard estimates the picture; the test's music and
sizes are ours, not every song or screen. It is a strong precaution, not a
medical guarantee.

## Use

```sh
axross-amp                          # the last playlist
axross-amp ~/Music/album/           # add a folder and play it
axross-amp list.m3u song.flac
axross-amp https://example.org/radio.mp3
```

Keys (on the Player tab, not while typing in a field):

| Key | |
|---|---|
| Space | play / pause |
| ← / → | seek 5 s (Shift: 30 s); Ctrl: previous / next track |
| ↑ / ↓, + / − | volume |
| Home | the track's start |
| M, N, P | mute, next, previous |
| V, Shift+V, F11 | the next / the previous visualization; full screen (Escape leaves) |
| Ctrl+F | the playlist's filter |
| Ctrl+↑ / ↓, Alt+↑ / ↓ | choose a track in the playlist; move it |
| Enter, Delete | play the chosen track; remove it |
| Ctrl+Z | undo the playlist's last change |

The mouse wheel seeks over the position and sets the volume over its slider.

## Build

```sh
cargo build --release               # target/release/axross-amp
cargo test                          # the window test renders it (needs a Vulkan device;
                                    # Mesa's lavapipe works without a GPU)
```

To have it in the desktop's menu and as an "Open with" for audio files (Linux):

```sh
install -Dm755 target/release/axross-amp ~/.local/bin/axross-amp
install -Dm644 packaging/axross-amp.desktop ~/.local/share/applications/axross-amp.desktop
install -Dm644 packaging/axross-amp.svg ~/.local/share/icons/hicolor/scalable/apps/axross-amp.svg
update-desktop-database ~/.local/share/applications   # (where it is installed)
```

One player at a time: a second start (a file opened from the file manager) hands its
files to the one running, over the session D-Bus; `--new-instance` starts another.

On Linux the ALSA headers are needed (`libasound2-dev`); PulseAudio / PipeWire is
spoken natively. libopus is built from its sources, which needs CMake.

## Status

This is the Rust rewrite of the Python `axross-amp` (written out of
[axross](https://github.com/c0decave/axross) by `scripts/export_standalone.py amp`).
The three tabs of the Python version are here, and its player's features but for
those named below; on top of them: gapless playback, ReplayGain, MPRIS, ICY titles and
reconnecting radios, a playlist filter and sorting, background reading of titles and
lengths, track details.

What stays in axross's Bluetooth panel only: the map of devices by distance, the
presence log, HCI capture streams, GATT details, connection tests and driving a USB
stick directly (Bumble). Left out on purpose: playback speed and A-B loops, lyrics
and scrobbling (none of them in axross either);
a visualization detached into a window of its own (full screen and the large view
cover it); axross's check that the sound card plays at its speed; its session's theme and language;
tracks dropped from axross's remote panes (axross's own business). On
macOS and Windows the Bluetooth tab points to the system's settings (BlueZ is
Linux's) and there is no MPRIS (media keys there would need the platform's own APIs);
the Mixer tab needs `pactl` (Linux, or a PulseAudio elsewhere).

Known limits: cpal's PulseAudio back end (cpal 0.18.2, the newest) takes a mutex
after the output callback has returned, to hand the samples over - outside this
player's callback, which itself takes no lock, and out of its hands until cpal
changes it. HLS radio (`.m3u8` segment lists) is not played. On a software renderer
the whole window is drawn by the CPU (llvmpipe: 0.06 to 0.25 s of a core a frame,
more as the window grows): the frame rate is lowered to stay under a core, so a
large window on a software renderer moves at a few frames a second
(`tests/real_window.rs` measures it); at rest it costs about nothing. There was no
GPU of its own to measure on here: the GPU's cost of the faces
(`--ignored` in `tests/faces.rs`) has only been measured on Mesa's lavapipe and
llvmpipe, by the clock.
The beat is found from the onsets; on syncopated music the tempo may read half or
double. The notes below about C2 are not told apart from their neighbours.

Tests: `cargo test` runs everywhere. The window is driven headless (egui_kittest,
wgpu - Mesa's lavapipe works without a GPU); the engine's sound is taken by the
tests themselves sample by sample (gapless joins, streams from local HTTP servers
that stall, break off, carry ICY titles); with `AMP_CORPUS` (a folder made by
`make.py` with PyAV) every format is played through ALSA's file plugin and measured.
The mixer and Bluetooth tests run against axross's fakes when they are given -
`AMP_FAKE_PACTL=axross/tests/fake_pactl.py`, `AMP_FAKE_BLUEZ=axross/tests/fake_bluez.py`
(on a private `dbus-daemon`) and `AMP_PYTHON` (a Python with jeepney); MPRIS runs on
a private `dbus-daemon` of its own. Without them those tests pass, saying so.

The faces are tested from a known signal: every one rendered and measured (how
much it draws, in how many colours, how much it moves), the shaders on the CPU
against the GPU, the cost of a frame at 620 x 300, 1920 x 1080 and 3840 x 2160, the
allocations of a frame counted; the analysis against known signals (a 1 kHz sine at
-18 dBFS reads -18 LUFS, mono correlates +1 and inverted -1, a C major chord lights
C, E and G, clicks at 128 BPM are 128 BPM, an intersample peak is found) and EBU
Tech 3341's cases 1-5, 9-11 and 15-18 and Tech 3342's cases 1-4 synthesised. Not
covered: 3341's case 6 (5.0 - the meter measures what is played, the stereo
downmix), 7, 8, 19-23 and 3342's 5 and 6 (EBU's own recordings, not derivable),
12-14 (their signals could not be checked against the specification here: not
claimed); end to end through the player, the
same integrated loudness shown, hidden, with the picture off and at 3 fps, the meters
starting a new track at its first sound, a chord progression's key and a melody's
notes; the CPU's drawing of the shader faces against the GPU's by SSIM in each colour
channel and by ΔE at the same moment; a hot master bit for bit at unity gain, the
limiter's true peak (measured at 8x: a dense master resampled, two hot masters
crossfaded), a crossfade, a fade when changing track, an album gapless with a
crossfade set (also untagged rips of one folder). In a real window (Xvfb,
`tests/real_window.rs`, also 1200 x 980) the frame rate stays under its cap, on a
software renderer the player takes under a core (`--release`), and at rest under 5 %.
`tests/python_compat.rs` has axross's own Python read and write the shared state file
(`AMP_AXROSS`, `AMP_PYTHON`; it says loudly when it could not).
`docs/axross_faces.py` renders axross's own faces (offscreen Qt) from the same signal
for the comparison sheet. `tests/states.rs` renders the window in each of its states
(also 420 x 420).

## Licence

Apache-2.0, as axross.

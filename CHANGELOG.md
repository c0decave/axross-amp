# Changelog

axross-amp is axross's Media Player as a program of its own, in Rust. The
versions before 0.14.0 were not published; they are listed so the history
reads whole.

## 0.17.1 - 2026-11-27

Titles that move so all of them is read, a cover for every track, the
dedicated GPU of a laptop with two, and an output that comes back - with
what the release review of it found (6.5 of 10: the output's recovery,
the log flood, a cover's seed, the marquee on a slow renderer, a switch
of the window's own to stop it) and its confirmation (7.5 of 10: a log
that cannot be written aborted the player; a recovery by the user left
it let go).

### Added
- **A cover for every track: 100 pictures made for tracks without one**
  (a user's wish: the empty box was what most of a collection without
  embedded art showed). Made in code (src/covers.rs) - no image files, no
  new crates -, in eleven families: gradient meshes, synthwave suns over a
  grid, Bauhaus compositions, watercolour and ink blots, layered waves,
  night skies (a planet and a nebula, the Milky Way, an aurora), mosaics,
  calm op-art, botanical silhouettes, records and their labels, ridged
  landscapes; each preset its family's parameters and one of twenty
  palettes, full-bleed and mid-toned for light and dark themes alike. A
  track always gets the same one: seeded (FNV-1a, the same on every
  machine) from its album in its folder - one album, one cover, as the
  playlist's album heads group it, compilations too - else from its file.
  It shows wherever a cover shows: the card and every design's cover
  slot, the cassette, the collage, the Cover face, the large picture, the
  playlist's album heads, and the desktop's (MPRIS) art. Made once a
  track on a thread of its own, at 256 and 1024 px, kept in memory (four)
  and on disk (`~/.cache/axross-amp/covers`, the newest 64); a frame
  showing one allocates no more than one showing a track's own cover (3
  allocations a frame at most, tested). A change goes through the
  window's own cover change - its turn every half second at most, its
  crossfade - and no picture has the saturated red the red guard slows
  for: a colour that red is made less so as it is drawn, whatever the
  seed (tested over 24 seeds of every preset). A track reached by the
  gapless advance gets its own cover, from what its own file says (not the
  album of the track before it). Options ›
  *Covers for tracks without one*: Generated (the default) or None (as
  before). Tested: the same track the same picture in two windows, an
  album one, others another; 100 presets pairwise unlike (16 x 16 means,
  the nearest two 0.061 apart, none under 0.06), none flat, none red;
  the frames' allocations with a made cover against an own one; an
  untagged track after an album's track by the gapless advance; tracks
  without covers switched every half second in every design and as the
  Cover face, read by both flash meters (`tests/covers.rs`,
  `tests/covers_alloc.rs`, `tests/window_flash.rs`); the design, flash
  and MPRIS tests run with them, as the player does.
  `COVERS_SHEET=1 cargo test --test covers` draws all 100.

### Changed
- **A title too long for its box moves, so all of it is read** (a user's:
  "'Back To The 80's' _ Best of Synthwave And Retro Electro Music Mix for
  2 Hours _ Vol. 5"). Since 0.16.1 it was cut to an ellipsis, its whole in
  a tooltip only - the 90s desktop's marquee had stood cut while paused.
  Now what plays - the title, the artist, the album, the lines of both,
  the "title - artist (format)" line - in every design and in its low and
  narrow forms, the large picture's line, and the name over the display
  view's picture: where it fits it stands as before; where it does not,
  it waits 2 s at its start, moves at a calm pace (about 2.3 times its
  type's size in points a second - some 32 at 14 points; a quarter faster
  in the intense motion) to its end, waits 1.5 s showing it, and comes
  back to its start. By the clock, not the sound: paused or stopped it
  goes on. A new track starts it from its start; the pointer over it holds
  it (its whole is in the tooltip as before); a screen reader is given its
  whole. Cut by its box, never over what is beside it. Each design its own
  way: Modern and Glass smoothly, on round ("   •   " between its end and
  its start again), its cut edges fading out; Zen, Soft, the cassette's
  label, the collage's artist and the display view smoothly there and
  back, fading; Brutalist on round, cut hard; the Hi-fi rack's fluorescent
  display a character cell at a time, on round after blank cells, its
  unlit segments with it; the 90s strip a cell at a time with "***"
  between, as the players of the time had it; the Terminal a cell at a
  time, there and back. The collage's title, cut from other pages word by
  word, is no longer left at its eighth word: its scraps are laid on
  pages, each shown a while and faded into the next. A right-to-left title
  starts at the right and moves the way it is read. Where the desktop asks
  for reduced motion nothing moves: cut to an ellipsis, its whole in the
  tooltip, as before - and so with Options › *Move titles that do not
  fit* off, the program's own switch (WCAG 2.2.2: a desktop that says
  nothing of reduced motion left no way to stop it). A click on a moving
  title stops it, another sets it moving again. The playlist's rows and
  "Up next" keep their ellipsis and tooltip (a list moving in every row is
  noise).
- It costs little: a frame is drawn only while it moves - during its
  waits the window sleeps until the wait ends; moving smoothly 20 to 30
  frames a second (as often as it moves a pixel), by cells about 4 (once
  a cell); a minimised window draws nothing. A frame of the window costs
  0.5 - 1 ms of CPU laid out and tessellated (620 x 860: Modern 0.9, the
  rack 1.0, the 90s strip 0.5), so some 2 % of a core moving smoothly, 0.4
  % by cells, beside the GPU's own. On a slow renderer (no GPU: Mesa's
  llvmpipe draws a frame in 50 - 100 ms of CPU) every design moves it in
  jumps of at most a third of its box, there and back, at the same pace
  on average - a frame every few seconds -, and the collage's pages turn
  without fading: stopped, with the picture off, measured in a real
  window there over 30 s, 0.4 % of a core with a short title and 3.9 %
  (Modern), 7.5 % (the rack), 2.1 % (the 90s strip) with the user's;
  cell by cell it had been 16 - 37 %. A jump is at most 3 s of the pace
  apart, and a wait the clock asked for is its own time (a jump 5.9 s
  away was taken for the window not drawn, and the title stood for
  good). The text is laid out once for each change of it (egui's cache;
  a frame looks it up - a text and its job a frame, as every label of
  egui's: a moving title allocates a handful a frame, tested).
- Tested (`tests/marquee.rs`): the user's title in all ten designs at
  620 x 860 and 420 x 420 - over a round its first letter and its last
  are each seen whole in its box, which runs into no other text; it moves
  while paused, starts again with a new track, holds under the pointer,
  moves to the right read from the right, stands where it fits (and asks
  for no frame), is cut and stands in reduced motion, wakes no minimised
  window; the bright title on the dark display and the 90s strip, at 420 x
  420 and 640 x 480 - and Modern's smooth fading and a slow renderer's
  jumps - read by both flash meters: 0 transitions by every reading. `tests/display_fit.rs` holds the moving boxes to the window and
  to the texts beside them.

### Fixed
- **Sound comes back after the output failed four times in ten seconds**
  (a Bluetooth headset or a car connecting and disconnecting): the player
  gave up for good - "choose one in the Mixer tab" - and stayed silent
  until it was started again. Now it lets the output go, says it is trying
  again, and tries once a round - after 5 s, then 10, 20 and every 30 s -
  until the device is back; a try that fails is let go at once (no three
  more at once); a stream that has played 30 s without an error is well
  again - whoever opened it: the retry, Play, a device chosen in the
  Mixer (the user's recovery left it let go for good, and a single loss
  hours later was 5-30 s of silence and "keeps failing"); time paused
  does not count. A track that played goes on where it was (Android
  stability audit, H4; `tests/engine.rs`, `tests/output_retry.rs`: the
  waits, one try a round, Play and a device chosen, time paused).
- **An output that fails at every write is no longer logged at every
  write**: what the output says - its own errors and alsa-lib's lines,
  which alsa-lib printed for every failed write itself - goes through a
  limiter of the program's, across the streams made anew: each kind of
  line (where in alsa-lib it comes from; the kind of the output's error)
  said once, then at most every 10 s with how many more - alsa-lib's
  flood does not keep the output's own errors unsaid. alsa-lib's lines are
  said with their arguments (which file, which device - formatted in C,
  `src/alsa_errors.c`: its handler is variadic), not the bare format. A
  log that cannot be written - a full disk under it, a closed terminal -
  loses lines and nothing else: every line the program says is written
  so, and nothing unwinds into alsa-lib (its first line aborted the
  player) nor into the sound system's callbacks (CoreAudio calls them
  from C: a panic there is silence and the output made anew). Errors that are not recovered from
  by themselves, 20 within a second, make the output lost; an underrun, a
  route followed or real-time scheduling refused never count (an
  overloaded machine's underruns let the output go). With ALSA's file
  plugin writing to a full disk: 4 lines in 25 s (two of alsa-lib's, two
  of the program's), where there were 26 260 of alsa-lib's and 86 of the
  program's, and 6 streams made, where there
  were some 13 every 5 s; with the log on a full disk, playing on
  (`tests/output_flood.rs`). (A sound file with its
  reader gone wrote 20 GB of the one line into the log in three hours.)
- **A cover made smaller wakes the window when it is done**: while its
  sizes were made the window asked for a frame every 30 ms - on a slow
  renderer some 10 frames a second, the Cover face held to 4, for as long
  as a large cover's sizes took (`tests/real_window.rs`, now with a cover
  made for its track).
- **Paused for five seconds, the output is let go**; Play opens it again
  where the sound left off - nothing lost, nothing heard twice (tested
  frame by frame). Before, the stream ran all the time - also paused,
  stopped or with nothing loaded - rendering silence a hundred times a
  second: a sound server's stream kept, the device held, a laptop (or
  phone) kept awake (Android stability audit, H3). While it is let go the
  player has no stream in the sound server's mixer (the system's mixer
  shows none for it).
- **The dedicated GPU of a Linux laptop with two (Optimus, PRIME) is
  used when the setting wants it** - High performance, or Auto on a
  machine without a battery. With an integrated GPU drawing the desktop
  and no Vulkan loader installed (wgpu then draws with OpenGL, which sees
  only the integrated GPU), High performance had nothing to choose: the
  owner's Iris Xe and NVIDIA laptop drew with the Iris. The machine is
  read from `/sys/class/drm` (each GPU's vendor, which drew the boot
  screen, its driver; an AMD APU beside an AMD card told by its memory),
  and the dedicated GPU asked for the way its driver wants, at the very
  start, before any thread or graphics: NVIDIA's own driver by PRIME
  render offload (`__NV_PRIME_RENDER_OFFLOAD=1`,
  `__GLX_VENDOR_LIBRARY_NAME=nvidia`, `__VK_LAYER_NV_optimus=NVIDIA_only`,
  NVIDIA's EGL vendor file), a Mesa driver (nouveau, amdgpu, radeon) by
  `DRI_PRIME=1`; a discrete card that draws the screen already, one GPU or
  a virtual machine's: nothing. Variables the user set stand. A window the
  dedicated GPU cannot give is made again at once, in the same process's
  place, on the integrated one - from a terminal by the copy in the
  background too, which knows the variables as the program's by a mark of
  its own (as in axross-files-matrix, its review 6's M3); an update's
  first start stays watched across it. A panic on the dedicated GPU
  before the window's first frame is shown (wgpu's validation, a device
  lost) is made good the same way; one after it is the program's, a panic
  as any other (not put on the GPU); an error of another kind (no
  display) is not. The start again knows itself so
  and takes the word out of its environment: a "Restart now" for an
  update starts a program that decides afresh and whose first start is
  watched. Helpers (gsettings, xrandr, pactl, a new release's self-test)
  get none of the program's variables. Not offloaded: a desktop whose
  display is on the card while the BIOS made the processor's GPU the
  first, or a card held by another driver than a graphics one (vfio-pci,
  for a virtual machine). The tests' fixture root is read in test builds
  only. Options › Graphics says what was done; `axross-amp --adapters`
  lists every adapter by API, the one the window would draw with and that
  note. Tested on fixture trees - Intel + NVIDIA's driver (the owner's),
  Intel + nouveau, AMD + AMD, one GPU, a virtual machine, a display on
  the card, vfio-pci -, the variables it sets, the start again without
  them, and the copy in the background keeping it (`tests/detach.rs`).

## 0.17.0 - 2026-11-24

Twenty new faces, and what independent reviews 18, 19 and 20 found in them
and around them.

### Added
- **Twenty new faces**, after the 49 there were (axross's eighteen keep
  their keys; each new one is saved in the shared state file as the face
  of axross's it is most like):
  - *Generative*: Orbits, Plasma globe, Fractal tree, Water surface (a
    shader), Constellation, Strange attractor (a shader), Bokeh, Cymatics (a
    shader), Contour map (a shader), Torus knot, Sunflower.
  - *Spectral*, a group of its own: Circular waveform, Bouncing balls,
    Spectrum globe, Honeycomb equalizer, Pitch spiral, Spectral vinyl,
    Radar, Pendulum wave, Spectrum rain.
  Every one is driven by the music - beat, bands, stereo, loudness - within
  the frame-cost limits, allocates no more in a frame than the others, and is
  measured by the flash tests like the others; the shader faces are drawn
  by the CPU too where there is no GPU.
- **Graphics: Auto, Power saving, High performance** (Options): which
  adapter draws the window. With an integrated and a dedicated GPU, Auto
  takes the integrated one on a machine with a battery - a player sits on
  one for hours; egui's own choice was the high-performance one - and the
  dedicated one on a machine known to have none (a desktop's monitor is on
  that card; a list of power supplies that is empty or cannot be read
  says nothing: the integrated one);
  Power saving and High performance say which outright. A software adapter
  (llvmpipe, WARP) never while there is a real GPU. Options names the one
  in use ("Vulkan – Intel(R) UHD Graphics 620") and says so where it is a
  software one: the shader faces are then drawn by the CPU. Taken at the
  next start, which the status line says when one is chosen.
  `WGPU_ADAPTER_NAME` (a part of an adapter's name), `WGPU_POWER_PREF`
  (`low`, `high`) and `WGPU_BACKEND` decide instead of the setting, and a
  start whose graphics cannot be set up names them (it said
  `Error: Wgpu(CreateSurfaceError(…))` and no more).
- **Titles in Chinese, Japanese, Korean, Arabic, Hebrew, Thai and
  Devanagari** are written with the system's fonts - they were rows of
  boxes wherever eframe's own search found nothing, and that search ran on
  the window's thread at the first such letter. The fonts are looked for
  in the background (the system's own matching, then well-known files) and
  `AXROSS_AMP_FONTS` names more files or folders - listed on that thread
  too, each file's letters read once. A right-to-left title reads from the
  right, its numbers and Latin words in place, a pair of brackets of one
  direction (UAX #9's rule N0: "Habaytak (حبيتك) 1970" keeps its year
  outside the brackets), brackets turned (it was laid word by word from
  the left).
- **No font file can end the program.** Every font - the bold, serif and
  monospace faces of the start as well as one found for a script - is
  gone through before egui is given it: every table within the file (half
  a font, a copy that stopped, lists them all), `head` with a size of the
  em that is one, each glyph's place within `glyf`, and every glyph drawn
  once, into nothing, with a panic of the font's reader caught - on the
  fonts' threads. Of 1,064 damaged files (a zeroed or 0xff block at every
  offset, each table zeroed or garbage, eight cuts, of six fonts) none
  panics as a face of the start or as a named font; 143 of 878 did
  ("index out of bounds" in the reader, "Bad px_scale_factor: inf"). And
  should a font panic in a frame all the same, the frame's panic is
  caught: the fonts found for other scripts are let go, at a second time
  the faces of the start, and the next frame is drawn with what is left.
- **Bluetooth: a double click on a device connects it** - a paired one is
  connected, one not paired yet is paired; nothing while it is busy or
  connected, or with the adapter off - and the row says "Double-click to
  connect." only where it would.
- **The library's dependencies are set up for Android** (eframe there
  without AccessKit, the clipboard and the desktop's window systems), to be
  built through the app, axross-amp-android, a repository of its own: the
  library alone does not build for Android (the activity is the app's to
  choose), and this version has not been built there - the app pins an
  earlier commit. Nothing changes for Linux, macOS and Windows: the same
  lock file, the same features of every crate.

### Changed
- **The menu of the faces** (⚙, a right click on the picture) for 69 of
  them: rows 24 points tall (16 - 18 before); three columns, two or one
  where the window is narrower than three - at the least window, 420 x 420,
  the third column lay past its edge in nine of the ten designs; the grid
  scrolls both ways and to the row that has the focus; its last row wraps
  (Brutalist's "Options" was off the window). The grid is as tall as the
  window lets it be (at the default window ten rows were below the fold
  with 190 points unused under the menu) and its scroll bar stays in
  sight; the row that has the focus wears a ring; Left and Right go across
  its columns (Left dropped the focus out of the grid).
- **Every submenu of the menu opens beside its button or above or below
  it, and scrolls** where the window is too low or too narrow for it: see
  *Fixed*.
- **Bouncing balls** (was Bouncing caps): a row of balls on pads, each
  kicked as high as its band is loud, falling back, squashed as it lands,
  with its trail and its reflection - it was the Spectrum's bars again, with
  caps thrown to the picture's top whatever the bar's height.
- **Water surface, Cymatics, Contour map**: the water with the light's net
  on its floor, a sheen on its rings and deeper colour (gentler in the
  starkest schemes and on light grounds, where the flash rule binds it) -
  on a light ground the net is a net of ink and the rings' rims a little
  darker (Paper was a pale sheet, all but blank: contrast 8.8 of 255, now
  18; rings in ink as strong as the net read ten transitions a second);
  the plate's sand in the colour of the band that sounds there, a new
  figure sooner, the bands' light spilling round the plate, and the plate
  jumps on the beat - its sand thrown off its lines and settling back (it
  moved 2.5 times as much with a song as in silence; 4.7 times now). The
  plate stays a square in the picture's middle: its figure is regular
  lines, and those may cover two fifths of a picture at most - as wide as
  the picture they covered 65 % of it. The map on a light ground a middle
  tone tinted by its height, pale lowlands to dark peaks, each line a
  shade darker than its land (lines in ink on a pale ground were a pattern
  by BT.1702); black on white its low land is nearly the ground (a tenth
  of the picture was paler than a middle grey; now near half).
- **The Contour map slows in Calm and in Reduced motion**: its hills
  followed the bands at once whatever the setting (0.99 and 0.82 of
  Intense's motion; now 0.64 and 0.31 - each hill eased over a sixth and a
  half of a second).
- **The Circular waveform** on a dark ground finds its gain by the line it
  draws, not by the samples: in a loud, dense passage the ring was nearly
  a plain circle. It is a little larger there, and its line heavier the
  louder it is. (On a light ground it is as it was: livelier, in black on
  white at 240 x 135 it read six transitions a second.)
- **The Radar** sends a ring out from its middle on the beat, and its beam
  goes round a little faster the louder it is: it swept the same with
  music and without.
- **The Flow field in a small picture** draws every second of its hairs,
  every third on a light ground: all of them, as close as they lie there,
  were rows of lines, and the guard's pattern stage softened the face in
  85 - 90 % of the frames in Paper at 240 x 135 (now in none).
- **Synthwave and Kinetic typography on light grounds**: the sun swells
  and the rows come half as fast with the bass, a word every 1.1 s (0.75):
  with drum and bass the veil lay over Synthwave a sixth of the track in
  Paper at 240 x 135, over the typography a sixth in a cream scheme.
  Synthwave is as calm in a small picture on a dark ground, and the
  Spirograph has no glow there: at 240 x 135 in Midnight the veil lay over
  Synthwave a fifth of an ordinary song and over the Spirograph a fifth of
  drum and bass (found by the same test, which holds Midnight too now).
- **On light grounds**: the Spectrum globe has a body and inked marks, the
  Radar a deeper face and inked echoes (pale on pale before); the
  Constellation in a small picture shows fewer stars with a shorter reach
  (a tangle at 240 x 135).
- **The Honeycomb equalizer** eases its cells over a seventh of a second
  and has larger cells in a small picture: it read 4 - 5 transitions a
  second on ordinary music at 240 x 135 and leant on the guard's veil in
  drum and bass.

### Fixed
- **A click on "Options ▸" in a low window changed the update mode**:
  with its Graphics block Options was taller than a window under about
  700 points; egui, finding no place beside the button, laid it over the
  button, and the click that opened it went on to the entry under the
  pointer. Its last entries lay off the window. Each of the menu's five
  submenus is now as tall and as wide as there is room beside, above or
  below its button, and scrolls (its bar in sight), in all ten designs
  down to the least window; `tests/face_menu.rs` opens every one by a
  click in every design at five window sizes and compares both settings
  files before and after.
- **A face chosen in ⚙'s menu was thrown away**, in all ten designs, by
  the pointer and by the keys: the top row wrote the face it had before
  the menu back over the choice. The menu from a right click worked; no
  test chose a face through ⚙ (`tests/face_menu.rs` does, in every design).
- **The flash guard's estimate held a dark wedge no face draws** on every
  light ground: the slivers of a rounded ground's fan of triangles were
  laid evenly along their length - half the ground at the corner across.
  The veil, a blur of the estimate, showed it as a streak; a piece is laid
  as wide as it is at each place now. With it, what brought the veil over
  pictures that do not flash, on light grounds in small pictures: the
  Circular waveform (a third of an ordinary song at 240 x 135), the stereo
  scope's wide figure, Kinetic typography's and the Spirograph's light
  added to a light ground. The veil staying off where nothing flashes is
  tested for every face through four kinds of music - two songs, drum and
  bass, speech - in Paper, in black on white and in Midnight at 240 x 135,
  in Sakura at 200 x 112 and in a cream scheme at 300 x 170
  (`tests/veil_light.rs`;
  four classic faces in Classic with one signal before). One veil more
  than is needed is left, and named there: the line in black on white
  through drum and bass.
- **The nine Spectral faces ignored Calm and Reduced motion**: they moved
  as fast where the desktop asks for less motion as in Intense. What moves
  by itself - a pendulum's swing, the rain's fall, the radar's sweep, a
  throw, a turn - goes by the pictures' pace now (0.7 in Calm, 0.45 in
  Reduced), and a line follows the sound more slowly there; the levels keep
  the music's own time.
- **A seek in a long Ogg that is sought by decoding** (pages on which no
  packet ends: 0.16.4) decoded the link from 0:00 - a second and a half of
  silence for a seek to 9:30 of ten minutes, 8.65 s to 57:00 of an hour,
  each seek of a drag in turn. The page scan keeps a place to decode from
  every four seconds; a seek reads from the last one a second before it
  (0.04 s whatever the place), and of the seeks waiting only the last is
  taken.
- **The Strange attractor fell into a dozen blobs in a loud passage** and
  all but vanished for seconds between phrases, with any music: its shape
  was drifted from one set of numbers to the next, and most of what lies
  between two strange attractors is a cycle of a few points. It draws
  eight shapes, each proven strange by a test (Lyapunov exponent, and how
  much of the plane the orbit covers), and goes from one to the next by
  drawing both - the old one's points fewer, the new one's more, over four
  seconds; the sound bends a shape only as far as it stays strange, and
  the picture is scaled to the cells brighter than all but one in two
  hundred. Through five kinds of music, 17 s each, the least it inks is
  0.12 - 0.22 of the picture against a median of 0.30 (0.01 - 0.05 before).
  A shape bent by the sound, or begun where the orbit happened to be, is
  drawn only where it covers seven tenths of what the shape covers by
  itself: the eighth, bent, was for seconds a thin drawing of a dozen
  curves in techno and metal.
- **The Plasma globe's filaments jumped** - up to 40 degrees, all of them -
  whenever their number changed: each has its place now, and the last one
  grows out and draws back.
- **A shut menu's still picture** let clicks through to what lies under
  it and showed the old choice: the second click of a slow double click on
  a face's row went to the picture under it and stepped the face on, one
  on a row lower down pressed Play or opened ⚙ again. The whole still
  picture takes the pointer while it is seen (half a second from the
  choice; every tap in that time) - a right click there opens the menu
  again where it is, a dialog that comes up is not under it - and it shows
  the face chosen. A button pressed on it in its last frames and let go
  after it would be gone is still its click: the right button 0.40 s
  after a choice opened nothing.
- **A tap on "Theme" or "Colours" shut the whole menu** at the default
  width: a click that arrives with the pointer - pressed in the frame the
  submenu opens or the next. egui shuts every menu at a click that began
  while a submenu was open, wherever it was; the submenus' clicks are told
  by the player now: one beside the submenu and its menu shuts the menu
  (and one within it where it did before: in ⚙'s), one on its button does
  not.
- **The submenus' scroll bar** is to be seen without the pointer on it:
  five points wide, its handle in the text's colour (egui's was all but
  unseen until the pointer came to it).
- **A cut-short AIFF file was refused whole** ("the file is empty or cut
  short") where a WAV file cut at the same place plays: it plays what is
  there. **An Ogg file cut in the middle of a page** lost that page's
  sound - 0.7 s of a 3 s file, all of a short one that is a single page:
  the packets that are whole in it are played (Vorbis and Opus). **The
  length shown for a file cut short** was its header's - 20 s shown, 10 s
  played: a WAV or AIFF file shows what is there from the start, any
  other once its end is read.
- **A start whose graphics cannot be set up** names the variables that
  choose another adapter or backend (see *Added*).
- **The Bluetooth row said "Double-click to connect."** also while the
  device was busy and with the adapter off, when a double click does
  nothing.
- **0.16.4's notes** said a face chosen in a menu or in the list waits its
  turn; it does not, and never did - it changes at once. That entry is
  corrected below.
- **Tests**: `seek15`'s "half a second of playing" was by the clock - on a
  busy machine little sound - and failed two times in eight under load; it
  is by the sound taken now.
- **The Pitch spiral**: its lowest name ("F#") was cut by the picture's
  edge; its names laid out anew each frame were 40 allocations a frame.

## 0.16.4 - 2026-11-23

What independent review 17 found in 0.16.3.

### Fixed
- **An update's first start, watched in the process that opens the
  window**: started from a terminal (which hands the player to a detached
  copy of itself) or handing its files to a player already running, the
  update was taken for one that did not start, the old program put back
  and the new one refused for good - "did not start here before", daily.
  The start is marked as watched only where the window opens.
- **V held** stepped the face at the key's repeat - 15 faces a second, the
  picture flashing 8 - 13 times a second in the default window. A held V
  (Shift+V) steps once; the face - by V or a click on the picture - changes
  at the window's shared turn, twice a second at most, in the picture's own
  window and on another display alike (one chosen in a menu or in the list
  changes at once: a click a choice).
- **The Cover face's layout** (Modern leaves its card out for it) changed
  outside that turn: tapped in and out it flashed 7 - 10 times a second.
  It follows the face at the turn.
- **Typing in the playlist's filter** resized the picture at each letter
  (the list's matching rows gave it room): 7 - 10 transitions a second.
  The picture keeps its size.
- **N or P held** moved the playing row's highlight at the key's repeat
  (12 by both meters at 640 x 480): twice a second at most now.
- **Controls off the window**: Brutalist at 580 - 630 points wide (the
  default window is 620) had Next, Clear and the playlist past its right
  edge; Modern at 540 - 550 the volume and the list; Glass from 780 on laid
  Next over the playlist's filter. Every design is checked at 620 x 860 and
  every width from 400 to 900, 10 apart (`tests/designs.rs`,
  `tests/display_fit.rs`).
- **Ogg**: a file of more than 64 links (a concatenation of recordings) had
  its links after the 8th timed by a page's capacity - 70 recordings of
  459 s said 796 s; every link is counted now (Opus by its packets' TOC,
  without decoding). An Ogg whose first page of sound completes no packet
  (granule -1: packets longer than a page - Opus at 510 kb/s on small
  pages) was taken for a recording joined 0.1 s in, and every seek in it
  played from the start (symphonia's seek is led astray by such pages):
  its length is right and it is sought by decoding from the link's start.
- **Popups opened and shut at a click's pace** (review 16): a popup shut -
  a menu, a list's choices - leaves what it showed a quarter second, then
  fades out over another; ⚙'s menu or the picture's, opened and shut 5
  times a second, read 10 transitions a second in Paper, now 0 - 1.
- **A second player** (`--new-instance`) in Install mode, its program
  updated by another, downloaded the release in full every day and failed
  to install it; it now checks nothing more and says to restart.
- **The flash guard's cost**: its estimate lays a rectangle whole and
  passes over the pieces it leaves out of a very large mesh at once, and
  its veil's colours are worked out once a cell, not once a corner.
  Unoptimised, a spectrum frame 3.0 -> 1.6 ms, particles at 1920 x 1080
  14.4 -> 12.1 ms (4.4 without the guard). A rectangle's cells are covered
  exactly, not by four samples a row: the line's fill through drum and
  bass, veiled 48 % of the time where nothing flashes, now 36 %.
- **The frame-cost test** holds the 95th percentile of 40 frames to the
  limit too, not only the median (a face slowed by 20 ms every sixth frame
  passed by its median).
- **The supply-chain gate's minimum age** is 14 days again (10 since
  0.14.2); url 2.5.9 passes as the reviewed security fix it is.

## 0.16.3 - 2026-11-22

- The frame-cost test measures by the machine it runs on - median CPU
  time, calibrated, one retry; 0.16.2's release did not build on GitHub's
  slower runner (particles at 1920 x 1080 18.7 ms of 16, the spirograph
  12.0 - 12.3 of 12; here, in 0.16.0 and 0.16.2 alike, 13 - 13.6 and 8.1
  - 8.7). No change to the player itself.
- 0.16.3 carries everything of 0.16.1 (the Hi-fi rack's display, what
  plays in every design, the cassette) and of 0.16.2 (url 2.5.9,
  RUSTSEC-2026-0592), whose releases were not published.

## 0.16.2 - 2026-11-22

- security: url 2.5.9, RUSTSEC-2026-0592 (a stack overflow in
  `Url::origin()` on nested `blob:` URLs; axross-amp does not call it -
  url comes in only through the window toolkit's link opener). url 2.5.9
  needs form_urlencoded 1.2.3. Both diffed against the versions reviewed
  before and accepted into the supply-chain baseline though younger than
  its ten days. 0.16.1's fixes - the Hi-fi rack's display, what plays in
  every design, the cassette - are released with it.

## 0.16.1 - 2026-11-22

What three users saw in 0.16.0's designs.

### Fixed
- **The Hi-fi rack's display**: from the second track on, the length and
  the track's name were cut off the readout - the line "02  0:00 / 0:40"
  was laid in a type chosen for nine of its heights, and the track's
  number took the room the length and the name needed. The time, position
  and length, is laid whole in the largest type that holds it; the track's
  number beside it where there is room, smaller, else not at all; the
  name under it cut to an ellipsis where it does not fit, its whole in a
  tooltip; the readout of a low window a little taller, the name in it
  too.
- **What plays, in every design, fits its box and runs into nothing**: in
  Modern (the default) a long title ran under the "Up next" column; in
  the 90s desktop a long track's time ran out of its well and the length
  was shown nowhere in a large window (it is beside the time now), and
  the words' marquee stood cut while paused (cut to an ellipsis now, its
  whole in a tooltip); the Terminal's first line ran into its box's name
  and its position's bar pushed the time out; the Collage's title scraps
  ran into the artist's line; a long album's name in the playlist's album
  rows ran into its track count. A name cut to an ellipsis has its whole
  in a tooltip wrapped within the window (egui's own was one line, wider
  than a small window). The face's name shown after a change no longer
  lies over "Visualization off", nor out of a small picture. A new test
  holds every design to it at eight sizes from 420 x 420 to 3000 x 1600,
  for the 1st of 1 to the 999th of 999, names short and long, in CJK,
  right to left, emoji and a word of 300 letters, tracks of 0:59, 59:59
  and 10:00:00: no text of what plays cut, none in the window over
  another.
- **The Cassette deck's cassette** reads as a compact cassette (IEC
  60094): a shell of 100.4 x 63.8 mm's proportions with rounded corners,
  the label round the window, two hubs of six teeth 42 mm apart inside
  the window, the tape packs - the left one shrinking as the side plays,
  the right one growing, the tape's length the same -, the screws, the
  write-protect tabs on the top edge, the trapezoid on the bottom edge
  with the head's and the pinch rollers' openings. It was a box with a
  label and two large reels at its window's ends, over its edges.

## 0.16.0 - 2026-11-20

What independent reviews 13 (7.5 of 10), 14 (8.0), 15 (7.5) and 16 (7.5)
asked for, and updates by themselves. The equalizer sounds as axross's now, a
theme or design is shown once clicked, the shared files keep what the
other program wrote, and the player looks for updates by itself: behaviour
changes, hence 0.16.

### Added
- **Updates by themselves**: at the start and then once a day, on a thread
  of its own and never on an offline or metered network (NetworkManager),
  the player looks for a newer release. A setting (Options): *Off*, *Say
  when one is out* (the default) or *Download and install by themselves* -
  every check of the update by hand holds (https to GitHub only, the
  signature, the checksum, the size, no older version, the self-test before
  it is put in place, the old program kept); the player is never restarted
  and what plays goes on: the new version runs from the next start or by
  *Restart now*. 0.15's *Check for updates at start* gives way to it: a
  player of 0.15 keeps its choice - off unless it was ticked (review 16: it
  was taken for no choice, and every 0.15 user asked GitHub daily); a new
  installation says when one is out. Review 16 besides: the first start
  of a program put in place is watched wherever it comes (one that never
  draws its window is replaced by the old program at the next start); a
  release that failed its self-test or was put back is never put in place
  by itself again; where the program's folder cannot be written it is not
  fetched at every start - said, and a file fetched by hand is kept in the
  cache; two players never both put a program in place (the second kept
  the new one as `.previous`); *Off* chosen while a check runs stops it; a
  `.previous` that is a link is replaced, not followed; the self-test keeps
  its 20 s with a child holding its output; the checks run while the window
  is minimised, the day counted by the wall clock.

### Fixed
- **The stereo scope** (one of axross's faces, review 16) under a chord
  panned hard left and right two or three times a second, in the default
  scheme and motion: 9 - 20 transitions a second by the review's meter. A
  figure of one channel alone goes to and fro along its diagonal, and egui
  joins a line turning right back by its normals' mean - nothing but
  rounding, made a unit: a wide band crossed the whole picture at a new
  angle almost every frame. Every face's lines are cut where they turn
  back. The flash guard now catches such bands too - a band two cells wide
  is laid on its cells as the large shape it is (laid as small pieces, it
  was never counted whole): wide bands at random every frame read 21 - 29
  without the guard, 1 - 5 with it.
- **The picture's colours in turn** (review 16): its schemes clicked in
  turn in its menu (it stays open) flipped its ground at the clicks' pace,
  7 - 10 transitions a second in Paper; a scheme, and the cover's colours'
  box, are shown at the window's turn. The layout, the theme and design,
  the picture's colours and the cover share one turn - each had its own,
  and three in turn could change the window six times a second. Held Space
  or Enter clicks nothing in the picture's own window and the view on
  another display either (they have their own keys). F11 again to leave
  full screen, then Escape at once, left it on (both cancelled).
- **Recordings of Ogg stations joined a few seconds into a song**
  (review 16): taken for files of their own since review 15's fix (a page's
  capacity - 30 s of Opus, 6 s of Vorbis - was the bound): no length, no
  seek, or a length too long and every seek early. Where a link's time
  begins is now its first page's granule less that page's own samples,
  counted packet by packet - quiet starts and recordings alike, exact to
  the sample. An Ogg file without its end (a download cut short) has a
  length and is sought.
- **The shared playlist, a reorder** (review 16): a reorder in one program
  was undone by an edit in the other (the order of whoever wrote last).
  The order is part of the merge: the one that reordered is kept, what the
  other added put by its neighbours; each copy of a track is an entry of
  its own. axross keeps a track that is not there at its start (a drive
  not mounted) - greyed, said, played once it is back - where it took it
  for its own removal and the shared list lost it for good.
- A playlist saved through the user's own link: a link leading nowhere (a
  NAS not mounted) or round in a loop is kept and the save refused (it was
  replaced by a plain file); chains of up to 40 links are followed, each
  folder on the way the user's or written by its owner only.
- **A crash** (review 15): the picture resized while paused (F11, the
  window, the playlist, the picture in its own window) after the flash
  guard's pattern stage had softened it - the stage's cells kept the old
  grid's size and the player aborted (spectrum, mirror, waterfall). They
  are laid on the new grid with the others; every face is resized again and
  again, paused and playing, in the tests.
- **Space or Enter held on a focused switch** (review 15): egui clicked it
  at each key repeat - the playlist and the equalizer, the large picture's
  playlist flipped 31 times a second. A held key's repeats reach no widget.
- **The window's layout changes in turn** (review 15): the tabs, the
  playlist and equalizer switch, the playlist's box and the large
  picture's playlist went through no turn (7 - 14 transitions a second at
  quick clicks, in Paper); every change of the layout - also full screen
  from the menu and *Normal size* - comes at most twice a second, and a
  change asked within the half second is made at its turn, not dropped
  (Escape then D, F11 then Escape); two of the same toggle waiting are none.
  The Player tab's first frame drew a black band where its toolbar comes
  (the top bar's panel kept its old height for a frame: the frame is done
  again), and the picture's menu stays open as its playlist box is ticked
  (it closed and was opened again at each click).
- **Vorbis beginning quietly** (review 15): libogg packs a quiet start into
  one page of seconds, and such a file was taken for a station's recording
  (6.24 s said for 8, every seek 1.76 s late). A recording is a first page
  whose granule is beyond what any page of the codec can hold.
- **The shared playlist, both programs editing it** (review 15): merged
  three ways by the tracks' paths on both sides - a track added here and
  removed in axross stays removed, an edit in axross keeps a track added
  here, both additions are kept (axross 2ee53ef0); tested with axross's
  own code.
- The oscilloscope through a square wave gated at 3 Hz (3 pattern frames:
  a low tone's first frame showed two and a half periods); an Ogg file
  damaged in its first page of sound plays its whole pages after it; a
  playlist saved through links follows them one at a time, each and the
  file at the end the user's own; the albums' covers are let go with the
  albums (kept for every album ever shown).
- **F11, D held** (review 14): a held key toggled full screen, or the view
  on another display, at its repeat - 30 transitions a second over the
  whole window by both meters. What toggles (F11, D, Escape, Space, M)
  takes a key's first press only, and the window's layout (full screen,
  the large picture, the display view, the detached window's full screen,
  a double click on the picture) changes at most twice a second. Designs
  clicked quickly read 7 - 8 by the strict reading: a design's fonts are
  swapped at its turn with it, not at the click.
- **The oscilloscope** (one of axross's faces) through a two-note melody
  in the default scheme and motion showed a pattern (8 - 14 frames): a
  steady tone is shown as two of its periods, the time base following its
  pitch (a mix and noise as before).
- **A recording of an Ogg station** (its granule positions hours in, where
  it joined) said it lasted hours, and a seek ended it or missed its later
  songs: each link is timed from where its own granules begin.
- **Shared state** (review 14): axross's every save wrote the list it had
  loaded, dropping a track added in axross-amp, and axross-amp then took
  that list over. axross merges its list three ways now (axross); and
  axross-amp keeps a track it has just added when the other program saves
  an older list without it.
- An Ogg file damaged in its middle (a page failing its checksum) was
  refused at its opening: what comes before the damage plays. An equalizer
  of cuts alone can raise a frequency by its fit's ripple: the output's
  limiter holds it then (a master at full scale was clipped). A playlist
  saved through the user's own link updates the list it names (the link was
  replaced). The playlist's lengths before a track plays are measured as
  when it plays (a VBR MP3 without a Xing header said symphonia's estimate).
- **Chained Ogg** (Vorbis, Opus, FLAC - a new link at each song, as an
  Icecast station sends it): playback stopped after the first link, and an
  Ogg radio went silent after its first song. Each link plays with its own
  decoder (another rate resampled), a radio says each song's title, a
  chained file's length is that of all its links and a seek goes into the
  link it falls in.
- **A VBR MP3 without a Xing header** was cut at symphonia's estimated
  length (half of a 6 s file lost), or said to be ten times as long: its
  frames are counted, it plays whole, its length is theirs and a seek goes
  to its frame.
- **A NaN or an infinite sample** in a file kept the equalizer at NaN to
  the end of the track (and through gapless joins): it is silence now, and
  the output takes no NaN.
- **A new output while the next track was queued** (a device chosen, lost
  and found again) played 96 ms of the next track twice; at the same rate
  nothing decoded ahead is taken back now (a crossfade under way neither).
- **The Theme and Design pickers** showed each theme or design the pointer
  (or the keyboard's focus) passed over, at once - down and up the list the
  whole window flashed 12 times a second. A click shows and keeps one, and
  the window takes another at most twice a second however fast they are
  clicked.
- **Covers in saturated red**: skipping between a red cover and a dark one
  gave a saturated red flash a second (the cover card, the Glass haze, the
  accent), outside the flash guard. A change of cover to or from one with
  much saturated red comes at most every two seconds, faded over one, and
  the haze and the cover's accent are never saturated red.
- **The equalizer sounds as axross's**: axross's curve (the gains
  interpolated between the bands, flat beyond them), by shelves and peaks
  fitted to it - within half a decibel for every preset, where Bass boost
  lowered everything above 1 kHz by 8 dB (the fixed headroom), Treble boost
  gave -7.4 dB at 20 kHz and all bands at +6 dB rippled 1.5 dB. No level is
  taken off for a boost: the output's limiter holds its peaks (on a radio
  too). The filters are the same at every setting: a band turned on or off
  no longer moves the others' state (a click).
- **Shared state with axross**:
  - a track axross added was lost when only the track playing here changed,
    or a track's tags were read: the playlist is merged by its paths, the
    track playing here kept current in axross's list;
  - axross-amp's output device is kept in its own file (axross rewrote
    audio.json with its own keys and the device was lost - read from the old
    place while the new one has none);
  - a face axross does not have was set back to the spectrum each time
    axross ran: axross-amp keeps its face in its own file, the shared file
    says the nearest of axross's 18;
  - opening Options rewrote axross's Smoothing and Peak fall (its ranges are
    wider): a value outside a slider's range stays until the slider is moved.
  axross's own side keeps axross-amp's keys and values too (axross
  `core/audio_settings.py`, `media_player.py`).
- **The updater** runs the new release once (`--self-test`) before it is put
  in place - one that cannot start on this system never replaces the
  player; keeps the old one (`.previous`) until the new one has run a minute
  (not only drawn a frame); *Restart now* watches the new one's start and
  puts the old one back should it end before its window is up; and the
  program it starts waits for the player's name on the session bus and
  takes it when the old one ends (it took a name of its own, and the next
  file opened from the file manager started a second player). What a cut
  off download left beside the program is removed at the next start.
- **Saving a playlist** followed a link planted at its predictable
  temporary name (overwriting another file and making it mode 0666): the
  temporary file is made anew (O_EXCL) under a random name, its mode set on
  the open file, and a link at the playlist's own name is replaced, never
  written through.
- **On a software renderer** a 2560 x 1440 window took 1.1 to 1.6 cores (a
  frame costs 0.3 s and more whatever it shows, and the frame rate went no
  lower than 3): it goes down to a frame every three seconds now, the clock
  with the pictures off and the heartbeat as seldom, and there the shader
  faces are drawn by the CPU's thread (a software rasterizer's "GPU" cost
  twice the frame) - 0.33 to 0.40 of a core at 2560 x 1440 on lavapipe now
  for the spectrum, the particles, the liquid and Off. The picture falls to
  rest at the clock's pace however few the frames.
- **The piano roll** was a pattern hazard by the project's own detector
  (a chord in octaves: 28 to 133 frames of stripes): its keys and notes lie
  within a shade of the ground, its keyboard drawn as a piano's (seven white
  keys an octave). **The feedback face** read 7 by the strict reading
  through a chord gated at 2 to 5 Hz: the bass pulls its zoom half as hard,
  the beat turns it half as much and its waves follow the bands eased over
  0.3 s - 0 to 5 now, in every look.
- A seek to the start landed 331 to 1204 frames late in an MP3 (its encoder
  delay dropped twice) and played an Opus file's pre-skip; damage in the
  middle of a file failed the whole track (up to 3 s of good sound dropped)
  where it ends the track now; a new ReplayGain did not reach the next track
  already opened; a build in a cargo target folder not named `target` was
  offered an update; Ctrl + / Ctrl - changed the volume as they zoomed; with
  the cover's colours on, a colour scheme chosen did nothing (choosing one
  now unticks them); a name could not hold U+10FE00 - U+10FE7F as itself (they
  stood for ASCII bytes: "../"); a hostile MP4 atom size overflowed.

### Changed
- Tests: `previous_on_a_radio_goes_to_the_track_before` and the album test
  wait by the sound, not the clock (they failed on a busy machine); the
  window's flashes (`tests/window_flash.rs`), chained Ogg, VBR MP3s without a
  Xing header, NaN, a new output during a gapless join, the equalizer's curve
  against axross's, the shared files both ways (Rust and axross's pytest).

## 0.15.1 - 2026-11-20

0.15.0 was tagged but not released (its Linux tests failed in CI: no
program files were made); its changes below come with this one.

### Fixed
- **A window library missing on Linux** is said, not a panic: without
  libxkbcommon-x11 (and the X libraries the window's toolkit loads by name)
  the program stopped with "Library libxkbcommon-x11.so could not be
  loaded." (exit status 101); now it says which library is missing and the
  package that has it (Debian and Ubuntu, Fedora, Arch) and exits with
  status 1. Under Wayland it asks for libwayland-client and libxkbcommon.

### Changed
- CI: Ubuntu's runners have no libxkbcommon-x11: it is installed with
  Xvfb for the real-window test, which there must run (`AMP_REQUIRE_X`);
  when the window draws nothing, the test says what the program wrote.

## 0.15.0 - 2026-11-20

### Added
- **Updates from the GitHub releases** (Options › Check for updates…, and
  *Check for updates at start* - both off until used). A newer release's
  file for this program - the Linux binary, the AppImage, the program in
  axross-amp.app, the Windows .exe - is fetched over https from GitHub's
  hosts only, taken only when `SHA256SUMS` carries a valid Ed25519
  signature by the release key pinned in the program and the file has the
  signed SHA-256 and the stated size, and put in place with one rename (the
  old one kept until the new one has started). Releases carry the program
  alone for each system and `SHA256SUMS.sig`; CI makes a draft that the
  maintainer signs and publishes (docs/RELEASING.md).
- A player before 0.15.0 has no update: it is updated by hand once, to
  0.15.0; from there on the player updates itself. A build from the source
  tree or from `cargo install` is never replaced. What is not tested yet
  (the replacement on Windows and macOS) is in docs/LIMITATIONS.md.

### Fixed
- **The flash guard lets no saturated red flash through**, however slow:
  a whole picture flickering in saturated red at 2 Hz (two red flashes a
  second, within the general budget of three) was not veiled at all. A
  saturated red change over a field now calls the veil in the frame it
  comes (review 12).
- **Zen and the Cassette deck use a very large window** (2200 points and
  more): Zen's column is two fifths of it with a picture a fifth of its
  height (it kept a column of 720 points and a strip of a picture), the
  Cassette deck grows up to 640 points tall (review 12, low).
- **Windows: file:// URLs** are `file:///C:/Music/...` both ways - a
  playlist's name starting with '#' or a space was saved as a URL no reader
  took for a local file and lost on reading, and the `file:///C:/...` of
  other programs' playlists was skipped.
- **Windows**: the shared state file and saved playlists are put in
  place with a retry while a virus scanner holds the new file a moment.

### Changed
- The faces, the analysis and the flash guard take their time step from
  egui's clock (on a screen the wall clock's as before): in the tests a
  busy machine's slow frames no longer count as seconds on the screen
  (the cover test read 16 transitions a second on a loaded machine).
- CI: a failing test is an annotation of its own (its name, where it
  panicked, the message), every test binary runs past a failing one, and
  the output is kept; the unit tests compare paths as Windows has them.

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

# Changelog

axross-amp is axross's Media Player as a program of its own, in Rust. The
versions before 0.14.0 were not published; they are listed so the history
reads whole.

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

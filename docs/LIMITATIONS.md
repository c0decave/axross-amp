# Known limitations

Back to the [README](../README.md) - everything in detail: the [extended README](README-extended.md).

- **Updates** (Options › Check for updates…, from 0.15.0): tried against a
  fake GitHub only (its API, its redirects, signed and badly signed
  releases) and on Linux; the Windows replacement (the running program
  renamed to `.previous`) and the macOS one (the program inside
  `axross-amp.app`, whose Info.plist keeps naming the old version) are not
  tested on those systems yet. A release before 0.15.0 has no signature
  and is never offered. An offline or metered network is known only where
  NetworkManager says so (Linux); elsewhere the daily check goes ahead (one
  small request; a download only when installing by themselves is chosen).
- The flash guard costs the CPU about 1.5 - 2 ms a frame, and 5 - 9 ms on the
  particles, the spirograph and the skyline at 4K: **60 frames a second is not
  supported for those faces** on a laptop's CPU (see
  [Photosensitivity](FLASH_SAFETY.md)).
- **Show on display** with the sound has been tested against a fake sound
  server only - not on real HDMI or DisplayPort hardware; on Wayland the
  compositor decides where the full-screen view goes; macOS and Windows list
  no monitors.
- By an independent review's stricter 100 ms reading (a field's pixels
  newly changing one way, summed over a tenth of a second - it counts a
  curve or small shapes moving over a field quickly too), 34 of 196 cases at
  60 frames a second in Classic and Fire, Calm and Intense, read more than 6
  a second - the wave ribbon most (Classic 41, Fire 24), then the galaxy, morph,
  isometric, milkdrop, ridges, typography, terrain, particles, beat rings
  (10 - 30), a few spectra (7 - 9); none of them more than before the reds
  were taken out. At 12 frames a second all read at most 5, and every
  other reading (one frame, 34 ms, the field's mean, red) at most 6
  everywhere.
- Through an independent review's harshest music (chords gated at 6 - 10
  Hz, fast sweeps), by the strict 100 ms reading, the spectrum in pure red
  and the Julia set in black on white, *Intense*, read 8 transitions a
  second (by WCAG's reading 1); the flow field in pure red and the
  reaction-diffusion in black on white, *Intense*, show a pattern for a
  frame. In *Calm* they keep within.
- The flow field in the built-in Classic scheme, *Intense*, shows a pattern
  (regular stripes, by the project's own detector) for 5 frames through an
  independent review's music and for 6 through fast chirps eight a second
  (review 12, which found it in *Intense* only); the guard's pattern stage
  softens them as they come.
- The flow field also shows its pattern for 5 - 7 frames through a chord in
  octaves gated or swelling and through a run of notes (review 14's music),
  *Intense*, Classic.
- **Shared state, a face chosen in axross** that is the nearest of axross's
  faces to the one shown here (the Starfield while axross-amp shows the
  Galaxy, the Spectrum while it shows Off, the Cover, the Analyzer or the
  Typography) cannot be told from "unchanged": axross-amp keeps its own.
- Also as they were: a seek that fails reports the position asked for;
  `iTunSMPB` (old iTunes' gapless) is not read; a 5.1 downmix is quieter than
  stereo; the state file is read twice at the start; a progressive JPEG cover
  of 8192 x 8192 takes about 400 MB while it is decoded; a PLS's titles are
  not counted; `file://` URLs that are no UTF-8 are skipped; Next at the end
  of the list wraps where axross's stops.
- Ordinary UI, by an independent review's clicks at 6.7 a second (review
  16): the large popups (⚙, the faces' list, More, Sort) opened and shut
  read 7 transitions a second in Paper (at 5 clicks a second 6), and Next
  moving the playing row's highlight in a 640 x 480 window 7 by the review's
  meter, 11 by the project's; the oscilloscope through a DC step at 3.5 Hz
  under a tone in white on black and the tunnel through a full-scale 4 Hz
  square in black on white, *Intense*, read 7 - 9 by the project's strict
  reading only (every other reading at most 3). The albums' covers are not
  capped while a long list is scrolled (a reply arriving after its album
  was let go is kept until the list changes).
- The first start of an update put in place is watched by the program
  itself: one that crashes before its window is up is replaced by the
  program before it at the *next* start (the crash is not caught as it
  happens), and only where the program's folder can be written.
- No CJK, Arabic or Hebrew font is bundled: such names are drawn as boxes
  (they take their room, cut and in tooltips as any other), and a
  right-to-left name is laid left to right.
- The flash guard dims the line face in very fast music more often than
  needed (it over-estimates the moving fill); nothing flashes, the picture is
  just darker at times.
- Not signed or notarised on macOS and Windows.

## Known limits

cpal's PulseAudio back end (cpal 0.18.2, the newest) takes a mutex
after the output callback has returned, to hand the samples over - outside this
player's callback, which itself takes no lock, and out of its hands until cpal
changes it. HLS radio (`.m3u8` segment lists) is not played. On a software renderer
the whole window is drawn by the CPU (llvmpipe: 0.06 to 0.25 s of a core a frame,
0.3 s and more at 2560 x 1440 whatever it shows): the frame rate is lowered to keep
the window within about a third of a core, down to a frame every three seconds (the
clock with the pictures off as seldom), so a large window on a software renderer
moves at a few frames a second or fewer (`tests/real_window.rs` measures it); at
rest it costs about nothing. There was no
GPU of its own to measure on here: the GPU's cost of the faces
(`--ignored` in `tests/faces.rs`) has only been measured on Mesa's lavapipe and
llvmpipe, by the clock.
The beat is found from the onsets; on syncopated music the tempo may read half or
double. The notes below about C2 are not told apart from their neighbours.

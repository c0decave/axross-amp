# Known limitations

Back to the [README](../README.md) - everything in detail: the [extended README](README-extended.md).

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
more as the window grows): the frame rate is lowered to stay under a core, so a
large window on a software renderer moves at a few frames a second
(`tests/real_window.rs` measures it); at rest it costs about nothing. There was no
GPU of its own to measure on here: the GPU's cost of the faces
(`--ignored` in `tests/faces.rs`) has only been measured on Mesa's lavapipe and
llvmpipe, by the clock.
The beat is found from the onsets; on syncopated music the tempo may read half or
double. The notes below about C2 are not told apart from their neighbours.

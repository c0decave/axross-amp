# Photosensitivity

Back to the [README](../README.md) - everything in detail: the [extended README](README-extended.md).

**Warning.** The pictures move with the music, and moving, flashing pictures
and stripes can bring on seizures in people with photosensitive epilepsy -
some of whom do not know it. If you or anyone watching is sensitive to
flashing light or patterns, do not watch the pictures: turn them off ("Off"
in the faces' menu) or choose the cover.

What axross-amp does about it:

- **Four readings; all are enforced.** Every rendered frame is read as
  WCAG 2.3.1 / 2.3.2 have it - a field changing over a quarter of its area
  the same way within 34 ms is a transition, a flash a pair of opposing
  ones -; by a stricter reading an independent review applies - within
  100 ms, and every change counted, each rise and each fall, paired or
  not -; and either way at once - the area newly transiting in a frame,
  brightening and darkening alike, where most of a pixel's neighbourhood
  transits with it - which sees flicker in antiphase (a checker whose
  tiles each change the other way from their neighbours never has a
  quarter of a field changing the same way: the first two readings saw 2
  of it at 15 Hz, this one 30) - three readings of light, each of
  saturated red too; and a fourth, saturated red from the gamma-encoded
  values (see *Measured*). A face passes only where all four count at most
  six transitions in every second and no pattern hazard, through the
  project's own test music; `tests/flash.rs` fails otherwise. (Through an
  independent review's harsher music - see *What does not hold* - some
  faces read more by the strict reading; by WCAG's reading none does.) (Thin lines moving are
  motion, not flicker: at no moment is a quarter of a field changing - the
  third reading counts only what transits together, and a test holds it
  to that.)
- **Calm faces.** Each face is meant to stay within the readings by
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
  reading does - and either way at once, as the third: half a field of
  large shapes transiting in one frame (a checker in antiphase), whatever
  their phase. Where a field has changed seven times in a second it lays
  a veil over the whole face - a slow, blurred average of the picture, 95
  % - staying on while the face goes on near the limit and a second and a
  half after. Seven, not six: the veil's own coming is a change too, and
  at six it would make the seventh. So the guard does not by itself hold a
  face to six; it caps what the faces do not foresee. On ordinary music - pop,
  house, drum and bass - it does not come (a test). It costs the CPU
  about 1.5 - 2 ms a frame on most faces at 1920 x 1080 (`--release`, an
  independent review's bench, the face with and without it: the spectrum
  1.7, the plasma 1.5, the cover 2.0), and more on the faces of thousands of
  pieces: the spirograph 5.1 ms, the particles 7.1 ms at 1920 x 1080 and 9.1
  at 3840 x 2160, the skyline 5.6 at 3840 x 2160. **Not met**: an independent review asked for about 2 ms on
  the shape-heavy faces. The guard lays every piece the face drew on its
  grid (about 0.6 µs a piece, and 0.5 ms whatever the face); laying only one
  piece in so many was tried and undone - one dot of the particles in two,
  counted twice, made a grating of the estimate and the pattern stage
  softened the face. A coarse picture of the face rendered on the GPU and
  read back would be the way; it is not done. So the particles, the
  spirograph and the skyline (at 3840 x 2160) do not hold 60 frames a
  second on a laptop's CPU with the guard on: 60 frames a second is not
  supported for them. With a GPU a frame of theirs simply takes as long as
  the guard does; on a software renderer the frame-rate governor lowers
  their rate (it counts the guard in). The guard is never dropped.
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
  per pixel (relative luminance; saturated red with R, G and B linear -
  and, as a fourth reading, saturated red from the gamma-encoded values as
  sent to the screen, the way a review found a red ground darkened to
  #330000 still red: 64 by it, 10 linear),
  any field a third of the picture each way (the picture taken as the
  whole screen: the strictest case), the readings above; regular stripes
  (five light-dark pairs or more, full periods alike, uneven gratings too)
  over a quarter of the picture while they move or drift (within a tenth
  of a second), or 40 % standing. `tests/flash.rs` renders every face and
  its options through music made to provoke them - kicks at 120 and 240
  BPM, gated noise, bass and treble in turn, fast sweeps, left and right
  in turn, bursts, silence and full scale; since 0.14.2 also stereo unlike
  left and right - noise different in each channel, a chord in antiphase
  at 8 Hz, 30 ms bursts nine times a second - at 60, 30 and 12 frames a
  second; the faces with history through ten seconds each of steady music;
  the default faces through pop, house and drum and bass (the veil must
  stay off); the feedback face's every look, Intense, white on black and
  pure red; the moiré at its most (Intense, white on black); every case an
  earlier sweep found; the Options at their extremes (no smoothing, 128
  bars, peaks falling at 300 % a second); faces moved by the energy after
  25 minutes of play. With `--ignored`, the whole sweep: every face and
  option, Calm and Intense, in Classic, Neon, Mono, Fire and five extreme
  custom schemes - white on black, pure red (#ff0000, peak #ff3030) on
  black, black on white, black on a pure red ground, and the colour
  editor's own defaults - at 320 x 180: 936 cases, **all pass all four
  readings** (the whole sweep run again with the gamma reading in it; the
  ring, changed after it, swept again in all nine schemes, Calm and Intense: 18 cases, all
  pass - at most 5 by the strict reading, 3 by the either-way one; Classic
  and Fire swept again once their reds were taken out: 208 cases, all pass,
  not one red transition). The
  most by the normal reading: 4 transitions a second, red 4; by the strict
  reading: 6, red 6 (14 cases at the limit, 13 of them in *Intense*); by
  the either-way reading: 4, red 4; red by the gamma-encoded reading 6
  (the Julia set, Intense, Fire); moving stripes in no frame; standing
  stripes at most 35 % (40 % allowed). At 12 frames a second (a slow
  renderer's) the whole sweep again - every face and option, both motions,
  all nine schemes, 936 cases - **all pass all four readings, and not one
  red transition by any reading in any case** (the most 4, strict 5,
  either-way 6; standing stripes at most 36 %): at that rate the pictures
  step at most about half the way a frame (the ring's near bars, the line
  in a pale scheme came to 7 by the either-way reading before), the flow
  field's hairs a shade fainter. `AMP_FLASH_OPTIONS=extreme` runs it
  with the Options at their extremes (Smoothing 0 asked, its
  floor given - 85 % at 128 bars; 128 bars; peaks falling at 300 % a second asked, 100 %
  given): in Classic, Neon, white on black, black on white and the colour
  editor's defaults, every face and option, Calm and Intense (552 cases,
  the three faces changed for it - terrain, beat rings, the starfield's
  trails - in all nine schemes), **all pass the three readings of light and
  red it was run with** (it was run before the gamma reading): the most
  by the normal reading 6 (synthwave, Intense, Neon), by the strict reading
  6 (6 cases), by the either-way reading 6, red 4, moving stripes in no
  frame, standing stripes at most 30 %. It first found the terrain at 8 to
  11 by the strict reading (its mesh as fine as the bars: now at most 48
  columns), the beat rings at 7 (their 256 spokes, now at most 48 a side,
  eased and dimmer) and the starfield's trails at 7 (a long trail now
  fainter, as a photograph's blur spreads a star's light; its speed eased);
  `the_options_at_their_extremes_keep_the_faces_calm` keeps those cases.

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
seven (at six its own veil would be the seventh) - or, a field flickering
eight times a second or more, at its fourth transition (within the first
three flashes); the faces pass by themselves, the guard is the backstop.
Through independent review 11's music (chords gated at 6 - 10 Hz, stereo
noise, sweeps; `tests/support/music11.rs`) the faces it found over the
project's limits now pass (`review_11s_cases_keep_within_the_limits`),
except, in *Intense*: by the strict reading the spectrum in pure red
reads 8 (a sweep's peak passing its high bars, each rising in turn) and
the Julia set in black on white 8 (by WCAG's reading both 1); the flow
field in pure red and the reaction-diffusion in black on white show a
pattern for one frame. In *Calm* all of them keep within (4, 4, 0, 0). In the sweep 14 cases sit at six by
the strict reading, the limit (13 of them in *Intense*).
The pattern stage acts in the frame it sees a pattern (it eased in over
0.05 s: up to six frames first) and does not see stripes finer than
about 1 % of the picture's width. Measured on our music at 320 x 180 (the sweep), 384 x 216
(the suite) and up to 1920 x 1080 (some faces), on Mesa's lavapipe, not a
real GPU.

**By a 100 ms reading of motion.** Independent review 9 also reads a
field's pixels newly changing one way, summed over a tenth of a second
since that field's last event - a reading that counts thin lines and
small shapes moving over a field quickly, not only areas flashing; the
project's meter has it as well (the swept reading). Over every face in
Classic and Fire, Calm and Intense, at 320 x 180 (review 9's meter, 392
cases): at 12 frames a second all read at most 5; at 60 frames a second
34 of 196 read over six - the wave ribbon most (Classic 41, Fire 24), the
galaxy (21 - 30), morph (10 - 22), isometric (21), milkdrop (12 - 20), the
ridges in Classic (15 - 16), typography (8 - 14), terrain and particles
(10 - 12), the beat rings (11 - 12), a few spectra (7 - 9). Reviews 8 and
9 judged these motion, not flashing (blurred and field-mean meters read
them within the rule), and none rose when the reds were taken out; the
review's other readings (one frame, 34 ms, the field's mean, red) read at
most 6 everywhere. The line spectrum (7 - 10 before 0.14.1) reads 3 - 6,
and the wave ribbon in black on white at 960 x 540 (review 10's reading:
40) 4; `the_line_sweeps_no_field_too_often` and
`the_ribbon_in_black_on_white_is_no_grating` hold them.

What is not claimed: the guard estimates the picture; the test's music and
sizes are ours, not every song or screen. It is a strong precaution, not a
medical guarantee.

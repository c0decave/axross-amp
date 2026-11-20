# axross-amp

axross's Media Player as a program of its own - written in Rust.

![The window](docs/window.png)

- **Plays almost anything** - MP3, FLAC, Ogg Vorbis, Opus, AAC / M4A, ALAC, WAV, AIFF, CAF, folders, M3U / PLS and internet radio
- **Truly gapless** - not a sample of silence between an album's tracks; an optional crossfade that never cuts into an album
- **Bit for bit at unity gain** - ReplayGain, a ten-band equalizer, true-peak limiters, a band-limited resampler
- **49 visualizations** - all eighteen of axross's classic faces, eight analysis tools, 23 generative ones
- **Guarded against flashes** - every face measured by WCAG 2.3.1 / 2.3.2 and ITU-R BT.1702, a flash guard at the output
- **Ten designs** - Modern, Zen, Hi-fi rack, 90s desktop, Cassette deck, Terminal, Brutalist, Glass, Soft, Collage
- **Twelve themes** - design tokens, contrast tested to WCAG AA (AAA in High contrast), follows the desktop's light or dark
- **Show on display** - the pictures full screen on a TV or projector, the sound moved to its HDMI or DisplayPort output
- **Mixer and Bluetooth built in** - the sound server's devices and applications, pairing and connecting headsets
- **At home on the desktop** - MPRIS media keys (Linux), one instance, the state shared with axross, and updates from the signed GitHub releases (Options › Check for updates…)

## Contents

- **Everything in detail: [Extended README](docs/README-extended.md)**
- [Screenshots](#screenshots)
- [Install](#install)
- [Quick start](#quick-start)
- [Documentation](#documentation)
- [Building](#building)
- [Licence](#licence)

> **Photosensitivity warning.** The visualizations move with the music. They
> are guarded against flashes, red flashes and patterns, but the pictures are
> not guaranteed safe for everyone. If you or anyone watching has epilepsy or
> feels unwell, turn the pictures off (⚙ → Off). *Calm* motion is the default;
> *Intense* asks first. What is measured and what is not claimed:
> [Photosensitivity](docs/FLASH_SAFETY.md).

## Screenshots

<table>
  <tr>
    <td><img src="docs/designs.png" alt="The ten designs"></td>
    <td><img src="docs/themes.png" alt="The twelve themes"></td>
  </tr>
  <tr>
    <td><img src="docs/faces-classic.png" alt="The classic faces"></td>
    <td><img src="docs/faces-generative.png" alt="The generative faces"></td>
  </tr>
  <tr>
    <td><img src="docs/faces-analysis.png" alt="The analysis faces"></td>
    <td><img src="docs/mixer.png" alt="The Mixer tab"></td>
  </tr>
</table>

<sub>The track in the pictures is a demo synthesized for them (`docs/make_demo.py`);
how each picture is made: [Extended README](docs/README-extended.md),
[Faces](docs/FACES.md#the-face-sheets), [Themes](docs/THEMES.md), [Designs](docs/DESIGNS.md).</sub>

## Install

Each [release](https://github.com/c0decave/axross-amp/releases) has the files
and their `SHA256SUMS`:

- **Linux (x86_64)** - `axross-amp-VERSION-x86_64.AppImage` (`chmod +x` it and run it; from a terminal it goes to the background, `--foreground` keeps it there) or the `linux-x86_64.tar.gz`
- **macOS** - `axross-amp-VERSION-macos-universal.dmg` (the app, Apple silicon and Intel) or the `.tar.gz` (not notarised: the first time, System Settings → Privacy & Security → Open Anyway - on macOS 15 and later the right-click way is gone - or `xattr -d com.apple.quarantine` on it once)
- **Windows (x64)** - `axross-amp-VERSION-windows-x64.zip` (not signed: SmartScreen asks once)
- **From source** - see [Building](#building)

Each archive carries `LICENSE`, `THIRD_PARTY_LICENSES.md` and (Linux) the
desktop entry and icon. Requirements and details: [Install](docs/README-extended.md#install). What changed:
[CHANGELOG](CHANGELOG.md).

**Updates** (from 0.15.0): *Options › Check for updates…* fetches a newer
release's file for this program from GitHub and puts it in place - only
when its `SHA256SUMS` is signed by the release key pinned in the program
and the file has the signed SHA-256. Off until used; a player before 0.15.0
is updated by hand once. Details: [Updates](docs/README-extended.md#updates).

## Quick start

```sh
axross-amp                          # the last playlist
axross-amp ~/Music/album/           # add a folder and play it
axross-amp https://example.org/radio.mp3
```

Space plays and pauses, V changes the face, D shows the pictures on another
display, F11 is full screen. All keys and options: [Use](docs/README-extended.md#use).

## Documentation

| Page | What it covers |
|---|---|
| [Extended README](docs/README-extended.md) | everything in detail: features, install, use and keys, build, status, tests |
| [Faces](docs/FACES.md) | the 49 visualizations, their colour schemes and options, the face sheets |
| [Designs](docs/DESIGNS.md) | the ten window designs and how they are tested |
| [Themes](docs/THEMES.md) | the twelve window themes and their contrast checks |
| [Show on display](docs/DISPLAY.md) | the pictures on a monitor of their own, with the sound |
| [Photosensitivity](docs/FLASH_SAFETY.md) | the flash guard, the readings, the measurements, what is not claimed |
| [Known limitations](docs/LIMITATIONS.md) | what does not hold or is not supported yet |
| [Changelog](CHANGELOG.md) | what changed in each version |

## Building

```sh
cargo build --release               # target/release/axross-amp
cargo test                          # needs a Vulkan device (Mesa's lavapipe works without a GPU)
```

On Linux the ALSA headers are needed (`libasound2-dev`); libopus is built from
its sources, which needs CMake. Desktop entry, icon and more:
[Build](docs/README-extended.md#build).

## Licence

Apache-2.0, as axross.

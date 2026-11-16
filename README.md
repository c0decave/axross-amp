# axross-amp

axross's Media Player as a program of its own - written in Rust.

![The window](docs/window.png)

- **Plays** local files (MP3, FLAC, Ogg Vorbis, Opus, AAC / M4A, ALAC, WAV, AIFF, CAF;
  MP4 / MKV containers), folders and M3U / PLS playlists, and **internet radio** by
  its http(s) URL - decoded by [symphonia](https://github.com/pdeljanov/Symphonia)
  (Opus by libopus, built in; AAC / ALAC in MP4 trimmed by its edit list: gapless),
  played through [cpal](https://github.com/RustAudio/cpal) (PulseAudio / PipeWire or
  ALSA on Linux, CoreAudio on macOS, WASAPI on Windows), resampled to the output's rate.
- **A ten-band equalizer** with axross's presets (Flat, Bass boost, Treble boost,
  Vocal, Loudness, Rock), headroom so a boost does not clip.
- **Visualizations** drawn on the GPU (egui): spectrum, mirrored, radial and polar
  spectrum, line spectrum, LED equalizer, waterfall, ridgelines, oscilloscope, stereo
  scope, VU meters, starfield - in axross's colour schemes (Classic, Fire, Ice, Neon,
  Mono, Custom), with its options (bars, smoothing, peak fall, frame rate). A click on
  the picture shows the next one, a double-click makes it large.
- **Playlist** with shuffle, repeat (all / one), drag and drop (also of files from
  the desktop), saving as M3U; cover art and tags (title, artist, album).
- **The system mixer** (Mixer tab): this player's output device, volume, mute and
  equalizer; the sound server's outputs and inputs (volume, balance kept, mute, port,
  default), the applications playing and recording (volume, mute, which device) and
  the cards' profiles (a Bluetooth headset: A2DP or headset) - through `pactl` of
  PulseAudio or PipeWire, live as they change elsewhere.
- **Bluetooth** (Bluetooth tab, Linux): the adapter (power, scan for Classic and / or
  BLE devices), the devices with kind, battery, signal and state; Pair, Trust,
  Connect, Disconnect, Forget - through BlueZ on the system D-Bus
  ([zbus](https://github.com/dbus2/zbus)). A pairing that asks (confirm a passkey,
  type a PIN) asks in a dialog; nothing is accepted without an answer. **Use as audio
  output** connects a speaker or headset and makes its output this player's.
- **Shares its state with axross**: the playlist, the track and its position, the
  volume, the visualization (`~/.config/axross/media_player.json`) and axross's one
  equalizer (`~/.config/axross/audio.json`) - the player inside axross and this
  program pick up where the other left off. Keys they do not know are kept.

![The visualizations](docs/visualizations.png)

<p><img src="docs/mixer.png" alt="The Mixer tab" width="45%"> <img src="docs/bluetooth.png" alt="The Bluetooth tab" width="45%"></p>

## Use

```sh
axross-amp                          # the last playlist
axross-amp ~/Music/album/           # add a folder and play it
axross-amp list.m3u song.flac
axross-amp https://example.org/radio.mp3
```

Keys: Space play / pause, Left / Right seek 5 s, Up / Down volume, M mute, N next,
P previous, V next visualization, Enter play the selected track, Delete remove it.

## Build

```sh
cargo build --release               # target/release/axross-amp
cargo test                          # the window test renders it (needs a Vulkan device;
                                    # Mesa's lavapipe works without a GPU)
```

On Linux the ALSA headers are needed (`libasound2-dev`); PulseAudio / PipeWire is
spoken natively. libopus is built from its sources, which needs CMake.

## Status

This is the Rust rewrite of the Python `axross-amp` (written out of
[axross](https://github.com/c0decave/axross) by `scripts/export_standalone.py amp`),
done step by step: **the player**, **the mixer** and **Bluetooth** are here, so the
three tabs of the Python version are. What stays in axross's Bluetooth panel only:
the map of devices by distance, the presence log, HCI capture streams, GATT details,
connection tests and driving a USB stick directly (Bumble). On macOS and Windows the
Bluetooth tab points to the system's settings (BlueZ is Linux's); the Mixer tab
needs `pactl` (Linux, or a PulseAudio elsewhere).

Tests: `cargo test` runs everywhere; the mixer and Bluetooth tests run against
axross's fakes when they are given - `AMP_FAKE_PACTL=axross/tests/fake_pactl.py`,
`AMP_FAKE_BLUEZ=axross/tests/fake_bluez.py` (on a private `dbus-daemon`) and
`AMP_PYTHON` (a Python with jeepney) - and pass without them.

## Licence

Apache-2.0, as axross.

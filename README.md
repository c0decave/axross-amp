# axross-amp

axross's Media Player, Audio Mixer and Bluetooth connector as one program: playlists, a 10-band equalizer, visualizations, internet radio, a mixer per source and Bluetooth headphones and speakers.

Three tabs in one window:

- **Media Player** - local files, folders and M3U playlists, and internet radio by its
  http / https URL. A 10-band equalizer with presets, animated visualizations (in a
  window of their own or full screen), shuffle, repeat and a sleep timer; the list, the
  track and its position come back at the next start.
- **Audio Mixer** - a volume, mute and output device per source, the shared equalizer
  and the system's sound server (PipeWire / PulseAudio). The player's sound menu
  (*Audio Mixer...*) opens this tab.
- **Bluetooth** - find, pair, trust and connect headphones, speakers and other devices
  through BlueZ (or a USB Bluetooth stick driven directly, with the optional `bumble`),
  battery levels, a presence log of devices in range, the audio profile per device.

```sh
axross-amp                      # the last list
axross-amp ~/Music/album/       # add a folder and play it
axross-amp https://example.org/radio.mp3
```

Keys in the player: Space play / pause, Left / Right seek, Up / Down volume, M mute.
Bluetooth needs BlueZ on the system bus (Linux) or a supported USB stick.

It is the same code as in [axross](https://github.com/c0decave/axross), written out by
axross's `scripts/export_standalone.py amp`; its settings are axross's
(`~/.config/axross`), so both share the language, the theme and the tool's state.

## Install and run

```sh
pip install axross-amp      # or, from this folder: pip install .
axross-amp
```

## Tests

```sh
python -m pytest tests
```

## Licence

Apache-2.0, as axross.

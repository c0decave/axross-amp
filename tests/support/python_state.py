"""axross's own player on the state file it shares with axross-amp:
`write PATH` writes a state as axross's PlayerPanel saves it (with a key
only axross knows), `check PATH` reads what axross-amp wrote as
PlayerPanel.restore_state would and says what is wrong (exit 1).

    python tests/support/python_state.py AXROSS_DIR write|check PATH
"""

import importlib.util
import sys
from pathlib import Path


def load(axross: Path):
    sys.path.insert(0, str(axross))
    spec = importlib.util.spec_from_file_location(
        "axross_media_player", axross / "resources/extensions/media_player.py")
    mp = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = mp
    spec.loader.exec_module(mp)
    return mp


def main() -> int:
    axross, mode, path = Path(sys.argv[1]), sys.argv[2], sys.argv[3]
    mp = load(axross)
    if mode == "write":
        state = {
            "version": mp.STATE_VERSION, "volume": 37, "muted": True, "shuffle": True,
            "repeat": "one",
            "tracks": [
                {"path": "/music/a.flac", "title": "First", "duration": 61000},
                {"path": "/music/b.mp3", "title": "", "duration": 0},
                {"path": "/music/c.ogg", "title": "Third", "duration": 2000},
                {"path": "https://radio.example/live.mp3", "title": "Radio", "duration": 0},
            ],
            "current": 2, "position_ms": 900, "visualizer": "radial",
            "equalizer_open": True, "visual_scheme": "neon",
            "visual_custom": dict(mp.CUSTOM_COLOURS),
            "visual_options": {**{k: v[2] for k, v in mp.VISUAL_OPTIONS.items()},
                               "hide_cursor": True},
            "playlist_open": True,
            "only_axross_knows": {"kept": [1, 2, 3]},
        }
        mp.write_state(path, state)
        return 0
    state, usable = mp.read_state(path)
    problems = []

    def want(key, value):
        if state.get(key) != value:
            problems.append(f"{key}: {state.get(key)!r}, not {value!r}")

    if not usable:
        problems.append("the file is not usable for axross")
    want("volume", 64)
    want("muted", False)
    want("repeat", "all")
    want("current", 1)
    want("position_ms", 1500)
    want("visualizer", "waterfall")
    want("equalizer_open", False)
    want("only_axross_knows", {"kept": [1, 2, 3]})
    if state.get("visualizer") not in mp.Visualizer.MODES:
        problems.append("axross has no such face")
    tracks = state.get("tracks") or []
    if len(tracks) != 5:
        problems.append(f"{len(tracks)} tracks")
    else:
        if tracks[0].get("title") != "Renamed by Rust":
            problems.append(f"track 0: {tracks[0]!r}")
        # (durations in seconds, as axross keeps them: 3000 ms is 3)
        if tracks[4].get("path") != "/music/tone.m4a" or tracks[4].get("duration") != 3:
            problems.append(f"track 4: {tracks[4]!r}")
        for t in tracks:
            # as restore_state takes them: a printable path, an int duration
            if not isinstance(t.get("path"), str) or not t["path"].isprintable():
                problems.append(f"a path axross skips: {t!r}")
            if not isinstance(t.get("duration", 0), int):
                problems.append(f"a duration axross ignores: {t!r}")
    if mp.encode_state(state) is None:
        problems.append("axross cannot save it again")
    for p in problems:
        print(p)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())

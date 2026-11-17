"""Render axross's own visualizer faces (its media_player.py, offscreen)
from the same signal the Rust tests use (tests/faces.rs: a chord over the
whole range, a beat, a little noise; 48 kHz stereo), at 30 frames a second,
into PNGs - to put them side by side with axross-amp's.

    QT_QPA_PLATFORM=offscreen python docs/axross_faces.py AXROSS_DIR OUT_DIR [W H]
"""

import importlib.util
import math
import sys
from pathlib import Path

import numpy as np

RATE = 48_000
FPS = 30
TICKS = 75  # 2.5 s: the histories fill, the sparks rise


def signal(seconds: float) -> np.ndarray:
    """(frames, 2) float32 - the same numbers as tests/faces.rs."""
    frames = int(RATE * seconds)
    t = np.arange(frames, dtype=np.float64) / RATE
    seed = np.empty(frames, np.uint64)
    s = 7
    for i in range(frames):  # the LCG of the Rust tests
        s = (s * 1_664_525 + 1_013_904_223) & 0xFFFFFFFF
        seed[i] = s
    noise = (seed >> 9).astype(np.float64) / (1 << 23) - 0.5
    beat = np.exp(-np.mod(t * 2.0, 1.0) * 8.0)
    base = np.zeros(frames)
    for k, hz in enumerate([55.0, 110.0, 220.0, 440.0, 880.0, 1760.0, 3520.0, 7040.0]):
        base += np.sin(t * hz * 2 * math.pi) * 0.22 / math.sqrt(k + 1)
    left = base * (0.6 + 0.4 * beat) + noise * 0.04
    right = base * 0.8 + np.cos(t * 330.0 * 2 * math.pi) * 0.15 + noise * 0.04
    return np.clip(np.stack([left, right], axis=1), -1, 1).astype(np.float32)


def main() -> None:
    axross, out = Path(sys.argv[1]), Path(sys.argv[2])
    width, height = (int(sys.argv[3]), int(sys.argv[4])) if len(sys.argv) > 4 else (620, 300)
    out.mkdir(parents=True, exist_ok=True)
    sys.path.insert(0, str(axross))
    from PyQt6.QtCore import QObject
    from PyQt6.QtWidgets import QApplication

    app = QApplication.instance() or QApplication([])
    spec = importlib.util.spec_from_file_location(
        "axross_media_player", axross / "resources/extensions/media_player.py")
    mp = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = mp
    spec.loader.exec_module(mp)
    sound = signal(TICKS / FPS + 0.2)

    class Engine(QObject):
        """What the visualizer asks of an engine, at the frame `now`."""
        now = 0

        def window(self):
            end = int(RATE * 0.2) + int(self.now * RATE / FPS)
            return sound[max(0, end - 2048):end]

        def current_bars(self, n_bars=32):
            return mp.spectrum_bars(self.window(), RATE, n_bars)

        def current_samples(self):
            return self.window()

    for mode in mp.Visualizer.MODES:
        engine = Engine()
        vis = mp.Visualizer(engine)
        vis.resize(width, height)
        vis.set_mode(mode, announce=False)
        vis.reset_stars(1)
        vis._rng = np.random.default_rng(1)
        for frame in range(TICKS):
            engine.now = frame
            vis.tick()
        vis.grab().save(str(out / f"{mode}.png"))
    app.quit()


if __name__ == "__main__":
    main()

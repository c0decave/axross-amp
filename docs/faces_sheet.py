"""Sheets of the faces' pictures, labelled.

    # axross's faces (docs/axross_faces.py) beside axross-amp's (tests/faces.rs)
    python docs/faces_sheet.py compare PYTHON_DIR RUST_DIR OUT.png
    # a group of axross-amp's faces, for the README
    python docs/faces_sheet.py gallery RUST_DIR OUT.png KEY ...

Needs PyQt6 (QT_QPA_PLATFORM=offscreen without a display).
"""

import sys
from pathlib import Path

from PyQt6.QtCore import QRect, Qt
from PyQt6.QtGui import QColor, QFont, QImage, QPainter
from PyQt6.QtWidgets import QApplication

AXROSS = [
    "spectrum", "mirror", "radial", "line", "led", "particles", "polar", "waterfall",
    "ridges", "terrain", "ring", "bells", "ribbon", "kaleidoscope", "scope", "lissajous",
    "vu", "starfield",
]
BACKGROUND = QColor("#0d1117")
TEXT = QColor("#e6edf3")
LABEL = 22


def scaled(path: Path, width: int) -> QImage:
    image = QImage(str(path))
    if image.isNull():
        image = QImage(width, width // 2, QImage.Format.Format_RGB32)
        image.fill(QColor("#300"))
    return image.scaledToWidth(width, Qt.TransformationMode.SmoothTransformation)


def sheet(cells, columns: int, width: int, out: Path, title: str = "") -> None:
    """cells: (label, image) in rows of `columns`."""
    height = max(img.height() for _, img in cells)
    rows = (len(cells) + columns - 1) // columns
    top = 34 if title else 0
    gap = 8
    page = QImage(columns * (width + gap) + gap, top + rows * (height + LABEL + gap) + gap,
                  QImage.Format.Format_RGB32)
    page.fill(BACKGROUND)
    p = QPainter(page)
    p.setRenderHint(QPainter.RenderHint.Antialiasing)
    font = QFont()
    font.setPixelSize(13)
    p.setFont(font)
    if title:
        big = QFont(font)
        big.setPixelSize(17)
        big.setBold(True)
        p.setFont(big)
        p.setPen(TEXT)
        p.drawText(QRect(gap, 6, page.width(), 26), Qt.AlignmentFlag.AlignLeft, title)
        p.setFont(font)
    for i, (label, image) in enumerate(cells):
        x = gap + (i % columns) * (width + gap)
        y = top + gap + (i // columns) * (height + LABEL + gap)
        p.setPen(TEXT)
        p.drawText(QRect(x, y, width, LABEL - 4), Qt.AlignmentFlag.AlignLeft | Qt.AlignmentFlag.AlignVCenter, label)
        p.drawImage(x, y + LABEL, image)
    p.end()
    page.save(str(out))


def main() -> None:
    app = QApplication.instance() or QApplication([])
    mode = sys.argv[1]
    if mode == "compare":
        python, rust, out = Path(sys.argv[2]), Path(sys.argv[3]), Path(sys.argv[4])
        width = int(sys.argv[5]) if len(sys.argv) > 5 else 400
        cells = []
        for key in AXROSS:
            cells.append((f"{key} - axross (Python, Qt)", scaled(python / f"{key}.png", width)))
            cells.append((f"{key} - axross-amp (Rust, egui)", scaled(rust / f"{key}.png", width)))
        sheet(cells, 4, width, out, "The same signal, the same scheme: axross's faces and axross-amp's")
    else:
        rust, out = Path(sys.argv[2]), Path(sys.argv[3])
        keys = sys.argv[4:]
        width = 300
        cells = [(key, scaled(rust / f"{key}.png", width)) for key in keys]
        sheet(cells, 4, width, out)
    app.quit()


if __name__ == "__main__":
    main()

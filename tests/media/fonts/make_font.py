"""The tests' font: tests/media/fonts/amp-fixture.ttf.

A few characters of scripts egui's own fonts cannot write - Japanese,
Chinese, Korean, Hebrew, Arabic - each a plain box, every one of its own
width (tests/fonts.rs tells them apart by it), and the Latin letters and
the digits, boxes too (a face of the start, where the machine has no bold
one to damage: tests/fonts.rs). Drawn here from nothing:
no other font's outlines are in it. Made with fontTools:

    python tests/media/fonts/make_font.py
"""

from pathlib import Path

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen

CHARS = "日本語の音楽中文한국어אבגדبتث"
LATIN = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"
UPM = 1000


def box(width: int):
    pen = TTGlyphPen(None)
    pen.moveTo((60, 0))
    pen.lineTo((60, 700))
    pen.lineTo((width - 60, 700))
    pen.lineTo((width - 60, 0))
    pen.closePath()
    # (a hole: a frame, not a slab)
    pen.moveTo((140, 80))
    pen.lineTo((width - 140, 80))
    pen.lineTo((width - 140, 620))
    pen.lineTo((140, 620))
    pen.closePath()
    return pen.glyph()


def main() -> None:
    names = [".notdef"] + [f"uni{ord(c):04X}" for c in CHARS + LATIN]
    widths = {".notdef": 500}
    glyphs = {".notdef": box(500)}
    for k, c in enumerate(CHARS):
        name = f"uni{ord(c):04X}"
        widths[name] = 400 + 40 * k
        glyphs[name] = box(widths[name])
    for c in LATIN:
        name = f"uni{ord(c):04X}"
        widths[name] = 560
        glyphs[name] = box(560)
    fb = FontBuilder(UPM, isTTF=True)
    fb.setupGlyphOrder(names)
    fb.setupCharacterMap({ord(c): f"uni{ord(c):04X}" for c in CHARS + LATIN})
    fb.setupGlyf(glyphs)
    fb.setupHorizontalMetrics({n: (w, 60) for n, w in widths.items()})
    fb.setupHorizontalHeader(ascent=800, descent=-200)
    fb.setupNameTable({"familyName": "Amp Fixture", "styleName": "Regular"})
    fb.setupOS2(sTypoAscender=800, sTypoDescender=-200, usWinAscent=800, usWinDescent=200)
    fb.setupPost()
    out = Path(__file__).with_name("amp-fixture.ttf")
    fb.save(out)
    print(out, out.stat().st_size, "bytes")


if __name__ == "__main__":
    main()

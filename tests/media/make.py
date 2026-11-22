"""The test files of tests/media (made with PyAV: python make.py tests/media).

- chained.ogg: a chained Ogg Vorbis file - two links of 1.5 s, each a
  physical stream of its own with its own serial, the first at 44.1 kHz
  (440 Hz), the second at 48 kHz (660 Hz) - as an Icecast station sends a
  new link at each song.
- chained.opus: the same with two Opus links (48 kHz, 440 and 660 Hz).
- vbr-noxing-short.mp3 / vbr-noxing-long.mp3: VBR MP3s of 4 s without a
  Xing or VBRI header, loud noise then a quiet tone (the length estimated
  from the first frames is too short) and the other way round (too long).
- capture1.ogg, capture.ogg: a recording of an Ogg radio - a link of 2 s
  (440 Hz) whose granule positions start 3 hours in (the station's count
  where the recording joined), alone and followed by a link of 2 s (660 Hz).
- damaged.ogg, damaged.opus: 2 s of a tone, 2 KiB of noise written over
  it at 40 % and at 60 % of the file (pages that fail their CRC).
- lo-*.ogg (independent review 15's): Vorbis files paged as libogg
  (oggenc, libvorbis) pages them - a page flushed past 4096 bytes or at 255
  segments, so a quiet start's first page holds seconds - made by
  oggpage.py repage from PyAV's files: lo-sil3-2tone.ogg (3 s of silence,
  2.5 s at 440 Hz, 2.5 s at 660 Hz), lo-sil2-2tone.ogg (2 s of silence),
  lo-sil3.ogg (3 s, then 5 s at 440 Hz), lo-sil6.ogg, lo-plain.ogg;
  cap-2tone.opus a recording (granules shifted 3 h: oggpage.py shift).
- corrupt-first-page.opus (independent review 13's): damage in the first
  and second pages of sound (at 129 and 20 252 of 40 302), its last page
  whole; damaged-first.ogg / .opus: 200 bytes overwritten in the first page
  of sound (pages of 0.1 s), the pages after it whole.
- chirp.mp3 (LAME: its header says the encoder's delay) and chirp.opus
  (a pre-skip): 1 s of a chirp, for a seek to 0.
"""
import av, numpy as np, os, sys

out = sys.argv[1] if len(sys.argv) > 1 else os.path.dirname(os.path.abspath(__file__))


def tone(rate, secs, hz):
    t = np.arange(int(rate * secs)) / rate
    v = (0.3 * np.sin(2 * np.pi * hz * t)).astype(np.float32)
    return np.array([v, v])


def encode(container, codec, rate, data, fmt="fltp", options=None, qscale=None, title=None):
    os_path = os.path.join(out, ".part")
    buf = av.open(os_path, "w", format=container, options=options or {})
    if title:
        buf.metadata["title"] = title
    s = buf.add_stream(codec, rate=rate, layout="stereo")
    if qscale is not None:
        s.codec_context.qscale = qscale
    if codec == "vorbis":
        s.codec_context.options = {"strict": "experimental"}
    fs = s.codec_context.frame_size or 1024
    if fmt.startswith("s16"):
        data = (data * 32767).astype(np.int16)
    planar = fmt.endswith("p")
    pts = 0
    for i in range(0, data.shape[1], fs):
        chunk = data[:, i:i + fs]
        arr = chunk if planar else chunk.T.reshape(1, -1)
        frame = av.AudioFrame.from_ndarray(np.ascontiguousarray(arr), format=fmt, layout="stereo")
        frame.sample_rate = rate
        frame.pts = pts
        pts += chunk.shape[1]
        for p in s.encode(frame):
            buf.mux(p)
    for p in s.encode():
        buf.mux(p)
    buf.close()
    data = open(os_path, "rb").read()
    os.remove(os_path)
    return data


def serial(ogg: bytes, new: int, granule_add: int = 0) -> bytes:
    """Every page of *ogg* given the serial *new* (and its CRC again); its
    granule positions *granule_add* later (as a station's are where a
    recording joins it)."""
    def crc(page):
        r = 0
        for b in page:
            r ^= b << 24
            for _ in range(8):
                r = ((r << 1) ^ 0x04C11DB7) & 0xFFFFFFFF if r & 0x80000000 else (r << 1) & 0xFFFFFFFF
        return r
    out, at = bytearray(), 0
    while at < len(ogg):
        assert ogg[at:at + 4] == b"OggS"
        n = ogg[at + 26]
        size = 27 + n + sum(ogg[at + 27:at + 27 + n])
        page = bytearray(ogg[at:at + size])
        page[14:18] = new.to_bytes(4, "little")
        granule = int.from_bytes(page[6:14], "little", signed=True)
        if granule_add and granule > 0:
            page[6:14] = (granule + granule_add).to_bytes(8, "little", signed=True)
        page[22:26] = b"\0\0\0\0"
        page[22:26] = crc(page).to_bytes(4, "little")
        out += page
        at += size
    return bytes(out)


vorbis = "libvorbis" if "libvorbis" in av.codecs_available else "vorbis"
a = encode("ogg", vorbis, 44100, tone(44100, 1.5, 440), title="First")
b = encode("ogg", vorbis, 48000, tone(48000, 1.5, 660), title="Second")
open(os.path.join(out, "chained.ogg"), "wb").write(serial(a, 0x1111) + serial(b, 0x2222))
a = encode("ogg", "libopus", 48000, tone(48000, 1.5, 440), fmt="s16", title="First")
b = encode("ogg", "libopus", 48000, tone(48000, 1.5, 660), fmt="s16", title="Second")
open(os.path.join(out, "chained.opus"), "wb").write(serial(a, 0x3333) + serial(b, 0x4444))

rng = np.random.default_rng(2)
noise = (0.3 * rng.standard_normal((2, 44100))).astype(np.float32)
quiet = tone(44100, 3.0, 440) * 0.5
for name, data in [("short", np.concatenate([noise, quiet], axis=1)),
                   ("long", np.concatenate([quiet, noise], axis=1))]:
    mp3 = encode("mp3", "libmp3lame", 44100, data, fmt="s16p", options={"write_xing": "0"}, qscale=4)
    open(os.path.join(out, f"vbr-noxing-{name}.mp3"), "wb").write(mp3)

one = encode("ogg", vorbis, 44100, tone(44100, 2.0, 440), title="One")
two = encode("ogg", vorbis, 44100, tone(44100, 2.0, 660), title="Two")
late = serial(one, 0x5555, granule_add=44100 * 3600 * 3)
open(os.path.join(out, "capture1.ogg"), "wb").write(late)
open(os.path.join(out, "capture.ogg"), "wb").write(late + serial(two, 0x6666))

rng = np.random.default_rng(3)
small = {"page_duration": "100000"}  # (pages of 0.1 s: the damage hits a few)
for name, data in [("damaged.ogg", encode("ogg", vorbis, 44100, tone(44100, 2.0, 440), options=small)),
                   ("damaged.opus", encode("ogg", "libopus", 48000, tone(48000, 2.0, 440), fmt="s16",
                                           options=small))]:
    b = bytearray(data)
    for frac in (0.4, 0.6):
        at = int(len(b) * frac)
        b[at:at + 2048] = rng.integers(0, 256, 2048, dtype=np.uint8).tobytes()
    open(os.path.join(out, name), "wb").write(bytes(b))

# damage in the first page of sound (the third page: after the headers'),
# the pages after it whole
for name, data in [("damaged-first.ogg", encode("ogg", vorbis, 44100, tone(44100, 2.0, 440), options=small)),
                   ("damaged-first.opus", encode("ogg", "libopus", 48000, tone(48000, 2.0, 440), fmt="s16",
                                                 options=small))]:
    b = bytearray(data)
    at, pages = 0, []
    while at < len(b):
        n = b[at + 26]
        pages.append(at)
        at += 27 + n + sum(b[at + 27:at + 27 + n])
    first_sound = pages[3] if name.endswith(".ogg") else pages[2]
    b[first_sound + 40:first_sound + 240] = bytes(200)
    open(os.path.join(out, name), "wb").write(bytes(b))

# a LAME MP3 (its Xing / LAME header says the encoder's delay and padding)
# and an Opus file of a chirp: a seek to 0 lands where playing from the start does
t = np.arange(int(44100 * 1.0)) / 44100
chirp = (0.3 * np.sin(2 * np.pi * (300 * t + 400 * t * t))).astype(np.float32)
open(os.path.join(out, "chirp.mp3"), "wb").write(
    encode("mp3", "libmp3lame", 44100, np.array([chirp, chirp]), fmt="s16p"))
t = np.arange(48000) / 48000
chirp = (0.3 * np.sin(2 * np.pi * (300 * t + 400 * t * t))).astype(np.float32)
open(os.path.join(out, "chirp.opus"), "wb").write(
    encode("ogg", "libopus", 48000, np.array([chirp, chirp]), fmt="s16"))
print("ok")

"""Independent review 17's Ogg cases (kept here as it wrote them, made
smaller for the repository: a source of 4 s for the large packets, 70
links of 3 s at 12 kb/s; tests/media/r17 is its output). Its cases, written with gen_ogg.py's helpers (imported by
path) and a pager of fixed-size pages that cuts packets across pages (as
ffmpeg's muxer and others do), so captures can begin with a continued
packet or a page on which no packet ends (granule -1).

Usage: python -I gen_ogg17.py tests/media/gen_ogg.py OUTDIR
"""
import importlib.util, json, os, struct, sys
import numpy as np, av, io

spec = importlib.util.spec_from_file_location("g", sys.argv[1])
g = importlib.util.module_from_spec(spec)
spec.loader.exec_module(g)
out = sys.argv[2]
os.makedirs(out, exist_ok=True)
man = {}


def encode(x, rate, channels, codec, bitrate=None, frame_ms=None):
    buf = io.BytesIO()
    c = av.open(buf, "w", format="ogg", options={"page_duration": "1"})
    s = c.add_stream(codec, rate=rate)
    s.layout = "mono" if channels == 1 else "stereo"
    if bitrate:
        s.bit_rate = bitrate
    cc = s.codec_context
    opts = {}
    if codec == "vorbis":
        opts["strict"] = "experimental"
    if frame_ms:
        opts["frame_duration"] = str(frame_ms)
    cc.options = opts
    fmt = cc.format.name
    data = np.tile(x[:, None], (1, channels)).T.copy()
    step = 960 * 6
    for i in range(0, x.shape[0], step):
        chunk = data[:, i:i + step]
        if fmt == "s16":
            arr = (np.clip(chunk, -1, 1) * 32767).astype(np.int16).T.reshape(1, -1)
            f = av.AudioFrame.from_ndarray(arr, format="s16", layout=s.layout.name)
        else:
            f = av.AudioFrame.from_ndarray(np.ascontiguousarray(chunk), format="fltp", layout=s.layout.name)
        f.sample_rate = rate
        f.pts = i
        for p in s.encode(f):
            c.mux(p)
    for p in s.encode(None):
        c.mux(p)
    c.close()
    pages = g.parse_pages(buf.getvalue())
    return g.packets_of(pages)


def fixed_pages(pkts, codec, serial=0x5EED, size=3000, shift=0):
    """Headers as libogg flushes them, then pages of `size` bytes cut
    anywhere in the packets' lacing (a packet across pages)."""
    o = g.LibOgg(serial)
    nhead = 3 if codec == "vorbis" else 2
    o.packetin(pkts[0][0], 0); o.flush()
    for pk, _ in pkts[1:nhead]:
        o.packetin(pk, 0)
    o.flush()
    nh = len(o.pages)
    seq = o.seq
    pages = list(o.pages)
    lacing = []
    body = b""
    for pk, gr in pkts[nhead:]:
        n = len(pk)
        segs = [255] * (n // 255) + [n % 255]
        for i, s_ in enumerate(segs):
            lacing.append((s_, gr + shift if i == len(segs) - 1 else None))
        body += pk
    continued = False
    while lacing:
        acc = 0
        vals = 0
        granule = -1
        while vals < len(lacing) and vals < 255 and acc < size:
            acc += lacing[vals][0]
            if lacing[vals][0] < 255:
                granule = lacing[vals][1]
            vals += 1
        lac = [v for v, _ in lacing[:vals]]
        sz = sum(lac)
        flags = 1 if continued else 0
        if vals == len(lacing):
            flags |= 4
        pages.append(g.make_page(flags, granule, serial, seq, lac, body[:sz]))
        seq += 1
        continued = lac[-1] == 255
        lacing = lacing[vals:]
        body = body[sz:]
    return pages, nh


def write(name, pages, **info):
    with open(os.path.join(out, name), "wb") as f:
        f.write(b"".join(pages))
    info["name"] = name
    info["pages"] = len(pages)
    man[name] = info


def flags_of(p):
    return p[5]


cases = [
    # (stem, codec, rate, ch, bitrate, frame_ms, page size, joins)
    ("obig", "libopus", 48000, 2, 510000, 120, 2500, [2]),
]
for stem, codec, rate, ch, br, fm, size, joins in cases:
    x = g.signal(rate, 4)
    pk = encode(x, rate, ch, codec, br, fm)
    grate = 48000 if codec == "libopus" else rate
    pages, nh = fixed_pages(pk, codec, size=size)
    ext = "opus" if codec == "libopus" else "ogg"
    # the plain file (cut in pieces): not a capture
    write(f"r17-{stem}-plain.{ext}", pages, codec=codec, rate=rate, channels=ch, secs=4, intro=0,
          kind="silence", capture=False)
    for j in joins:
        # the first page at or after j s of granule (the previous page's
        # granule may be -1: search back to the last known one)
        def gr(i):
            return g.granule_of(pages[i])
        k = next(i for i in range(nh + 1, len(pages)) if gr(i - 1) >= j * grate)
        cap = list(pages[:nh]) + list(pages[k:len(pages) - 2])
        # the time the capture's first complete sound comes from: the
        # earliest packet that starts on page k (continuations: later)
        prev = gr(k - 1)
        last = g.granule_of(cap[-1])
        cont = bool(pages[k][5] & 1)
        write(f"r17-{stem}-cap{j}.{ext}", cap, codec=codec, rate=rate, channels=ch, capture=True,
              shift=0, t0=prev / grate, approx_secs=(last - prev) / grate, continued=cont,
              g1=gr(k))
        # and one starting on a page where no packet ends (granule -1)
        km = next((i for i in range(k, len(pages) - 4) if gr(i) == -1), None)
        if km is not None:
            back = km - 1
            while gr(back) < 0:
                back -= 1
            cap2 = list(pages[:nh]) + list(pages[km:len(pages) - 2])
            write(f"r17-{stem}-capneg{j}.{ext}", cap2, codec=codec, rate=rate, channels=ch,
                  capture=True, shift=0, t0=gr(back) / grate,
                  approx_secs=(g.granule_of(cap2[-1]) - gr(back)) / grate, continued=True, g1=-1)

# a concatenation of 70 recordings (links), each joined 1 s into a 3 s
# song
x = g.signal(48000, 3)
pk = encode(x, 48000, 1, "libopus", 12000, 20)
allp = []
for n in range(70):
    pages, nh = g.libogg_file(pk, "libopus", serial=0x1000 + n)
    k = next(i for i in range(nh + 1, len(pages)) if g.granule_of(pages[i - 1]) >= 1 * 48000)
    allp += list(pages[:nh]) + list(pages[k:])
    if n == 0:
        prev = g.granule_of(pages[k - 1])
        last = g.granule_of(pages[-1])
        link1 = (last - prev - 312) / 48000
write("r17-chain70caps.opus", allp, codec="libopus", rate=48000, channels=1, capture=True, shift=0,
      t0=prev / 48000, approx_secs=link1 * 70, link1=link1, chained=True, repeat=True)

with open(os.path.join(out, "manifest.json"), "w") as f:
    json.dump(man, f, indent=1)
for n, i in man.items():
    print(n, {k: (round(v, 3) if isinstance(v, float) else v) for k, v in i.items()
              if k in ("pages", "g1", "t0", "approx_secs", "continued")})

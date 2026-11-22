"""Independent review 16's generator of Ogg test media (kept here as it
wrote it, its set made smaller for the repository: Opus mono at 24 kb/s,
sources of 30 s - `main`). Ogg Vorbis/Opus paged as libogg pages
(ogg_stream_pageout: a page closed once its body passes 4096 bytes with at
least 4 packets just done, or at 255 segments), with quiet intros, quiet
middles, odd rates, and station captures (pages dropped from the front,
granules kept or shifted). Independent of the project's oggpage.py: the
packets and their granules are taken from ffmpeg's own Ogg muxer run with
one packet per page.

Signal: a chirp f(t) = F0 + K*t Hz, t the time from the source's start, so
a frequency heard tells the time it was taken from: t = (f - F0) / K.

Usage: python -I gen_ogg.py OUTDIR   (writes the files and manifest.json;
tests/media/r16 is its output)
"""
import av, io, json, struct, sys, os
import numpy as np

F0, K = 200.0, 40.0

# ---------------------------------------------------------------- Ogg bits
def _crc_table():
    t = []
    for i in range(256):
        r = i << 24
        for _ in range(8):
            r = ((r << 1) ^ 0x04C11DB7) if r & 0x80000000 else (r << 1)
        t.append(r & 0xFFFFFFFF)
    return t
_T = _crc_table()
def crc(b):
    c = 0
    for x in b:
        c = ((c << 8) & 0xFFFFFFFF) ^ _T[((c >> 24) ^ x) & 0xFF]
    return c

def make_page(flags, granule, serial, seq, lacing, body):
    h = bytearray(b"OggS" + bytes([0, flags]) + struct.pack("<qIII", granule, serial, seq, 0)
                  + bytes([len(lacing)]) + bytes(lacing))
    p = h + body
    struct.pack_into("<I", p, 22, crc(p))
    return bytes(p)

def parse_pages(data):
    at, out = 0, []
    while at + 27 <= len(data):
        assert data[at:at+4] == b"OggS", at
        n = data[at+26]
        lac = list(data[at+27:at+27+n])
        body = data[at+27+n:at+27+n+sum(lac)]
        flags = data[at+5]
        g, serial, seq = struct.unpack_from("<qII", data, at+6)
        out.append(dict(flags=flags, granule=g, serial=serial, seq=seq, lacing=lac, body=body, at=at))
        at += 27 + n + sum(lac)
    return out

def packets_of(pages):
    """(packet bytes, granule of the page where it ends) - from pages that
    end one packet each (asserted where a granule matters)."""
    out, cur = [], b""
    for p in pages:
        off = 0
        ends = 0
        for i, l in enumerate(p["lacing"]):
            cur += p["body"][off:off+l]; off += l
            if l < 255:
                ends += 1
                out.append([cur, None]); cur = b""
        if ends:
            assert ends == 1 or p["granule"] == 0, ("several packets on a page", p["granule"], ends)
            out[-1][1] = p["granule"]
    return out

class LibOgg:
    """libogg's ogg_stream_packetin / pageout / flush."""
    def __init__(self, serial):
        self.serial, self.seq = serial, 0
        self.lacing = []   # (value, granule or None when not a packet's end)
        self.body = b""
        self.bos = False
        self.pages = []
        self.continued = False
    def packetin(self, pkt, granule):
        n = len(pkt)
        segs = [255] * (n // 255) + [n % 255]
        for i, s in enumerate(segs):
            self.lacing.append((s, granule if i == len(segs) - 1 else None))
        self.body += pkt
    def _flush(self, force, nfill=4096):
        if not self.lacing:
            return False
        maxvals = min(len(self.lacing), 255)
        acc = 0; granule = -1
        if not self.bos:
            vals = 0
            for vals in range(maxvals):
                if self.lacing[vals][0] < 255:
                    vals += 1; break
            else:
                vals = maxvals
            granule = 0
        else:
            done = just = 0
            vals = 0
            while vals < maxvals:
                if acc > nfill and just >= 4:
                    force = True; break
                acc += self.lacing[vals][0]
                if self.lacing[vals][0] < 255:
                    granule = self.lacing[vals][1]
                    done += 1; just = done
                else:
                    just = 0
                vals += 1
            if vals == 255:
                force = True
        if not force:
            return False
        lac = [v for v, _ in self.lacing[:vals]]
        size = sum(lac)
        flags = (0 if self.bos else 2) | (1 if self.continued else 0)
        self.pages.append(make_page(flags, granule, self.serial, self.seq, lac, self.body[:size]))
        self.seq += 1
        self.bos = True
        self.continued = lac[-1] == 255
        self.lacing = self.lacing[vals:]
        self.body = self.body[size:]
        return True
    def pageout(self):
        while self._flush(False):
            pass
    def flush(self):
        while self._flush(True):
            pass
    def end(self):
        # (the last page: e_o_s)
        self.flush()
        last = bytearray(self.pages[-1])
        last[5] |= 4
        struct.pack_into("<I", last, 22, 0)
        struct.pack_into("<I", last, 22, crc(bytes(last)))
        self.pages[-1] = bytes(last)

# ---------------------------------------------------------------- signals
def chirp(t):
    return np.sin(2 * np.pi * (F0 * t + K / 2 * t * t))

def signal(rate, secs, intro=0.0, kind="silence", quiet=None, amp=0.5, seed=1):
    n = int(round(rate * secs))
    t = np.arange(n) / rate
    x = amp * chirp(t)
    rng = np.random.default_rng(seed)
    m = t < intro
    if kind == "silence":
        x[m] = 0.0
    elif kind == "noise60":
        x[m] = rng.standard_normal(m.sum()) * 10 ** (-60 / 20)
    elif kind == "fade":
        db = -90 + 90 * (t[m] / max(intro, 1e-9))
        x[m] *= 10 ** (db / 20)
    if quiet:
        a, b = quiet
        q = (t >= a) & (t < b)
        x[q] = 0.0
    return x.astype(np.float32)

# ---------------------------------------------------------------- encode
def encode(x, rate, channels, codec):
    """ffmpeg's Ogg of x, one packet a page: [(packet, granule)], headers."""
    buf = io.BytesIO()
    c = av.open(buf, "w", format="ogg", options={"page_duration": "1"})
    s = c.add_stream(codec, rate=rate)
    s.layout = "mono" if channels == 1 else "stereo"
    if codec == "libopus":
        s.bit_rate = 24000
    cc = s.codec_context
    if codec == "vorbis":
        cc.options = {"strict": "experimental"}
    fmt = cc.format.name
    data = np.tile(x[:, None], (1, channels)).T.copy()  # planar
    step = 1024
    for i in range(0, x.shape[0], step):
        chunk = data[:, i:i+step]
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
    pages = parse_pages(buf.getvalue())
    return packets_of(pages)

def libogg_file(pkts, codec, serial=0x5EED, shift=0):
    o = LibOgg(serial)
    nhead = 3 if codec == "vorbis" else 2
    o.packetin(pkts[0][0], 0); o.flush()       # (the BOS page alone)
    for pk, _ in pkts[1:nhead]:
        o.packetin(pk, 0)
    o.flush()                                   # (oggenc flushes its headers)
    nheadpages = len(o.pages)
    for pk, g in pkts[nhead:]:
        if g is None:
            raise SystemExit("a packet without a granule")
        o.packetin(pk, g + shift)
        o.pageout()
    o.end()
    return o.pages, nheadpages

def capture(pages, nhead, first, drop_tail=2):
    """A station's capture: its headers, then its pages from `first` on (the
    stream's own sequence numbers and granules), cut before its end."""
    out = list(pages[:nhead]) + list(pages[first:len(pages) - drop_tail])
    # (no EOS: it was cut)
    return out

def granule_of(page):
    return struct.unpack_from("<q", page, 6)[0]

def main(outdir):
    os.makedirs(outdir, exist_ok=True)
    man = {}
    def write(name, pages, **info):
        with open(os.path.join(outdir, name), "wb") as f:
            f.write(b"".join(pages))
        info["name"] = name
        info["pages"] = len(pages)
        man[name] = info
    # plain files with quiet intros (not captures)
    plain = [
        ("v-sil3.ogg", "vorbis", 44100, 2, dict(intro=3, kind="silence", secs=11)),
        ("v-sil10.ogg", "vorbis", 44100, 2, dict(intro=10, kind="silence", secs=18)),
        ("v8k-sil30.ogg", "vorbis", 8000, 2, dict(intro=30, kind="silence", secs=38)),
        ("o-sil10.opus", "libopus", 48000, 1, dict(intro=10, kind="silence", secs=18)),
    ]
    for name, codec, rate, ch, p in plain:
        secs = p.pop("secs")
        x = signal(rate, secs, **p)
        pkts = encode(x, rate, ch, codec)
        pages, nh = libogg_file(pkts, codec)
        g1 = granule_of(pages[nh])
        write(name, pages, codec=codec, rate=rate, channels=ch, secs=secs, intro=p.get("intro", 0),
              kind=p.get("kind"), quiet=p.get("quiet"), capture=False, g1=g1, first_page_secs=g1 / (48000 if codec == "libopus" else rate))
    # station captures: pages dropped from the front
    sources = [
        ("cap-v", "vorbis", 44100, 2, dict(secs=30), [3], [0]),
        ("cap-v8k", "vorbis", 8000, 2, dict(secs=30), [5], [0]),
        ("cap-o", "libopus", 48000, 1, dict(secs=30), [5, 20], [0, 3 * 3600]),
        # joined in a quiet passage (12 to 20 s of silence)
        ("cap-vq", "vorbis", 44100, 2, dict(secs=30, quiet=(12, 20)), [14], [0]),
    ]
    for stem, codec, rate, ch, p, joins, shifts in sources:
        secs = p.pop("secs")
        x = signal(rate, secs, **p)
        pkts = encode(x, rate, ch, codec)
        grate = 48000 if codec == "libopus" else rate
        for sh in shifts:
            pages, nh = libogg_file(pkts, codec, shift=sh * grate)
            for j in joins:
                # the first page whose previous page ends at or after j s
                k = next(i for i in range(nh + 1, len(pages))
                         if granule_of(pages[i - 1]) - sh * grate >= j * grate and granule_of(pages[i - 1]) > 0)
                cap = capture(pages, nh, k)
                prev = granule_of(pages[k - 1]) - sh * grate
                last = granule_of(cap[-1]) - sh * grate
                ext = "opus" if codec == "libopus" else "ogg"
                name = f"{stem}{j}{'-h' if sh else ''}.{ext}"
                write(name, cap, codec=codec, rate=rate, channels=ch, capture=True, shift=sh,
                      t0=prev / grate, t_end=last / grate, approx_secs=(last - prev) / grate,
                      g1=granule_of(pages[k]), first_page_secs=(granule_of(pages[k]) - granule_of(pages[k - 1])) / grate)
    # a capture that ends where the station's song ended (its EOS page
    # kept), the same chained into the next song (a new link from 0), and
    # plain files cut short (no EOS: a download or recording stopped)
    for stem, codec, rate, j in [("cap-v", "vorbis", 44100, 3), ("cap-o", "libopus", 48000, 10)]:
        ch = 2 if codec == "vorbis" else 1
        x = signal(rate, 30)
        pkts = encode(x, rate, ch, codec)
        grate = 48000 if codec == "libopus" else rate
        pages, nh = libogg_file(pkts, codec)
        k = next(i for i in range(nh + 1, len(pages)) if granule_of(pages[i - 1]) >= j * grate)
        cap = capture(pages, nh, k, drop_tail=0)
        prev = granule_of(pages[k - 1])
        last = granule_of(cap[-1])
        ext = "opus" if codec == "libopus" else "ogg"
        write(f"{stem}{j}-eos.{ext}", cap, codec=codec, rate=rate, channels=ch, capture=True, shift=0,
              t0=prev / grate, approx_secs=(last - prev) / grate, g1=granule_of(pages[k]), first_page_secs=0.0)
        # the next song: 10 s from 0, another serial
        y = signal(rate, 10)
        pk2 = encode(y, rate, ch, codec)
        p2, _ = libogg_file(pk2, codec, serial=0xB0B)
        write(f"chaincap-{stem[4:]}{j}.{ext}", cap + p2, codec=codec, rate=rate, channels=ch, capture=True,
              shift=0, t0=prev / grate, approx_secs=(last - prev) / grate + 10.0, chained=True,
              link1=(last - prev) / grate, g1=granule_of(pages[k]), first_page_secs=0.0)
    for name in ["v-sil3.ogg"]:
        data = open(os.path.join(outdir, name), "rb").read()
        pg = parse_pages(data)
        cut = pg[-3]["at"]
        info = dict(man[name])
        trunc = name.replace(".", "-trunc.", 1)
        with open(os.path.join(outdir, trunc), "wb") as f:
            f.write(data[:cut])
        grate = 48000 if info["codec"] == "libopus" else info["rate"]
        info.update(name=trunc, secs=pg[-4]["granule"] / grate - (312 / 48000 if info["codec"] == "libopus" else 0))
        man[trunc] = info
    with open(os.path.join(outdir, "manifest.json"), "w") as f:
        json.dump(man, f, indent=1)
    for n, i in man.items():
        print(n, {k: (round(v, 3) if isinstance(v, float) else v) for k, v in i.items() if k in ("pages", "g1", "first_page_secs", "t0", "approx_secs")})

if __name__ == "__main__":
    main(sys.argv[1])

"""Ogg tools: re-paginate a Vorbis/Opus stream as libogg does (a page
flushed past 4096 bytes or at 255 segments), shift granules (a station's
capture), CRC. Usage:
  oggpage.py repage IN OUT        (libogg's paging)
  oggpage.py shift IN OUT SAMPLES (granules shifted: a capture)"""
import struct, sys, av

def crc_table():
    t = []
    for i in range(256):
        r = i << 24
        for _ in range(8):
            r = ((r << 1) ^ 0x04C11DB7) if r & 0x80000000 else (r << 1)
        t.append(r & 0xFFFFFFFF)
    return t
T = crc_table()
def crc(b):
    c = 0
    for x in b:
        c = ((c << 8) & 0xFFFFFFFF) ^ T[((c >> 24) ^ x) & 0xFF]
    return c

def page(serial, seq, granule, flags, packets_segments, body):
    hdr = b"OggS" + bytes([0, flags]) + struct.pack("<qIII", granule, serial, seq, 0) + bytes([len(packets_segments)]) + bytes(packets_segments)
    p = bytearray(hdr + body)
    struct.pack_into("<I", p, 22, crc(p))
    return bytes(p)

def lacing(n):
    segs = [255] * (n // 255) + [n % 255]
    return segs

def read_pages(data):
    at = 0
    while at + 27 <= len(data) and data[at:at+4] == b"OggS":
        nseg = data[at+26]
        segs = data[at+27:at+27+nseg]
        body = sum(segs)
        yield at, data[at:at+27+nseg+body]
        at += 27 + nseg + body

def packets_with_granules(path):
    d = av.open(path)
    st = d.streams.audio[0]
    extr = st.codec_context.extradata
    out = []
    for p in d.demux(st):
        if p.size == 0: continue
        out.append((bytes(p), p.pts + p.duration))
    return st.codec_context.name, extr, out

def xiph_headers(extr):
    n = extr[0] + 1
    i = 1; sizes = []
    for _ in range(n - 1):
        s = 0
        while extr[i] == 255: s += 255; i += 1
        s += extr[i]; i += 1
        sizes.append(s)
    hs = []
    for s in sizes:
        hs.append(extr[i:i+s]); i += s
    hs.append(extr[i:])
    return hs

def repage(src, dst, serial=0x1234):
    codec, extr, pkts = packets_with_granules(src)
    out = []
    seq = 0
    if codec == "vorbis":
        hs = xiph_headers(extr)
        out.append(page(serial, seq, 0, 2, lacing(len(hs[0])), hs[0])); seq += 1
        segs = lacing(len(hs[1])) + lacing(len(hs[2]))
        out.append(page(serial, seq, 0, 0, segs, hs[1] + hs[2])); seq += 1
    else:
        raise SystemExit("vorbis only")
    segs, body, gran = [], b"", -1
    for i, (pk, g) in enumerate(pkts):
        ls = lacing(len(pk))
        if len(segs) + len(ls) > 255 or len(body) > 4096:
            out.append(page(serial, seq, gran, 0, segs, body)); seq += 1
            segs, body = [], b""
        segs += ls; body += pk; gran = g
    out.append(page(serial, seq, gran, 4, segs, body))
    open(dst, "wb").write(b"".join(out))
    return out

def shift(src, dst, by):
    data = open(src, "rb").read()
    out = []
    for at, p in read_pages(data):
        p = bytearray(p)
        g = struct.unpack_from("<q", p, 6)[0]
        if g > 0:
            struct.pack_into("<q", p, 6, g + by)
        struct.pack_into("<I", p, 22, 0)
        struct.pack_into("<I", p, 22, crc(p))
        out.append(bytes(p))
    open(dst, "wb").write(b"".join(out))

def granules(path):
    data = open(path, "rb").read()
    return [(at, struct.unpack_from("<q", p, 6)[0]) for at, p in read_pages(data)]

if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "repage":
        repage(sys.argv[2], sys.argv[3])
    elif cmd == "shift":
        shift(sys.argv[2], sys.argv[3], int(sys.argv[4]))
    elif cmd == "granules":
        print(granules(sys.argv[2]))

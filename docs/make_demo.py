"""A demo track for the README's picture: synthesized here (chords, bass,
an arpeggio, a soft beat), tagged as what it is, with a cover drawn here."""
import av, numpy as np, io, sys, os, struct
from PIL import Image, ImageDraw
out = sys.argv[1]
rate, secs = 44100, 40.0
t = np.arange(int(rate * secs)) / rate
def note(f, start, length, amp, shape="sine"):
    s = np.zeros_like(t); i0, i1 = int(start*rate), int(min(secs, start+length)*rate)
    tt = t[i0:i1] - start
    env = np.minimum(1, tt/0.02) * np.exp(-tt * 1.8)
    w = np.sin(2*np.pi*f*tt) + 0.3*np.sin(4*np.pi*f*tt) + 0.15*np.sin(6*np.pi*f*tt)
    s[i0:i1] = amp * env * w
    return s
mid = lambda n: 440 * 2 ** ((n - 69) / 12)
prog = [(57, [57, 60, 64]), (53, [53, 57, 60]), (48, [55, 60, 64]), (55, [55, 59, 62])]
L = np.zeros_like(t); R = np.zeros_like(t)
bar = 2.0
for b in range(int(secs / bar)):
    root, chord = prog[b % 4]
    for n in chord:
        pad = note(mid(n), b*bar, bar*1.1, 0.06)
        L += pad; R += pad * 0.9
    bass = note(mid(root - 12), b*bar, bar, 0.22)
    L += bass; R += bass
    for k in range(8):
        n = chord[k % 3] + 12 * (1 + (k // 3) % 2)
        a = note(mid(n), b*bar + k*bar/8, 0.4, 0.07)
        L += a * (0.6 + 0.4*(k % 2)); R += a * (1.0 - 0.4*(k % 2))
    for k in range(4):
        i0 = int((b*bar + k*bar/4) * rate); n = int(0.08 * rate)
        kick = np.sin(2*np.pi*55*np.arange(n)/rate) * np.exp(-np.arange(n)/rate*40) * 0.35
        L[i0:i0+n] += kick; R[i0:i0+n] += kick
        h0 = int((b*bar + k*bar/4 + bar/8) * rate); m = int(0.03 * rate)
        hat = np.random.default_rng(b*8+k).standard_normal(m) * np.exp(-np.arange(m)/rate*120) * 0.05
        L[h0:h0+m] += hat; R[h0:h0+m] += hat * 0.8
fade = np.minimum(1, np.minimum(t / 1.5, (secs - t) / 3))
data = (np.vstack([L, R]) * fade / max(np.abs(L).max(), np.abs(R).max()) * 0.8).astype(np.float32)
path = os.path.join(out, "plain.mp3")
c = av.open(path, "w", format="mp3")
s = c.add_stream("libmp3lame", rate=rate, layout="stereo"); s.bit_rate = 192000
fs = s.codec_context.frame_size or 1152; pts = 0
d16 = (data * 32767).astype(np.int16)
for i in range(0, d16.shape[1], fs):
    chunk = d16[:, i:i+fs]
    fr = av.AudioFrame.from_ndarray(np.ascontiguousarray(chunk), format="s16p", layout="stereo")
    fr.sample_rate = rate; fr.pts = pts; pts += chunk.shape[1]
    for p in s.encode(fr): c.mux(p)
for p in s.encode(): c.mux(p)
c.close()
# the cover: a dusk gradient with a sun, 1200 px (drawn at twice that and
# made smaller: smooth edges)
S = 2400
img = Image.new("RGB", (S, S))
dr = ImageDraw.Draw(img)
for y in range(S):
    k = y / (S - 1)
    dr.line([(0, y), (S, y)], fill=(int(40 + 200*k), int(30 + 80*k*k), int(90 + 40*(1-k))))
dr.ellipse((S/3, S*0.467, S*2/3, S*0.8), fill=(255, 196, 92))
for y in range(int(S*2/3), S, S // 25):
    dr.rectangle((0, y, S, y + S // 60), fill=(40, 24, 64))
img = img.resize((1200, 1200), Image.LANCZOS)
buf = io.BytesIO(); img.save(buf, "PNG"); png = buf.getvalue()
def frame(fid, body):
    return fid + struct.pack(">I", len(body)) + b"\0\0" + body
text = lambda s: b"\x00" + s.encode("latin-1")
frames = frame(b"TIT2", text("Afterglow")) + frame(b"TPE1", text("axross-amp demo")) + \
    frame(b"TALB", text("Synthesized for the screenshot")) + frame(b"TRCK", text("3")) + \
    frame(b"APIC", b"\x00image/png\x00\x03\x00" + png)
size = len(frames)
syncsafe = bytes([(size >> 21) & 0x7f, (size >> 14) & 0x7f, (size >> 7) & 0x7f, size & 0x7f])
open(os.path.join(out, "Afterglow.mp3"), "wb").write(b"ID3\x03\x00\x00" + syncsafe + frames + open(path, "rb").read())
os.remove(path)
print("ok")

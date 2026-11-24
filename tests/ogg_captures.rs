//! Ogg lengths and seeks (independent review 16's test): libogg-paged
//! Vorbis and Opus with quiet intros, odd rates, cut short, and recordings
//! of stations joined a few seconds into a song - the media of
//! tests/media/r16, made by tests/media/gen_ogg.py (R16_OGG: another
//! folder of its files). Prints a table; fails listing every file misjudged
//! (review 16, M6: a recording joined less than a page's capacity in - 30 s
//! of Opus, 6 s of Vorbis - was taken for a file of its own: no length, no
//! seek, or a length too long and every seek early).
//!
//! Signal: a chirp f(t) = 200 + 40 t Hz (t: the source's time), so a
//! frequency heard tells the time it came from.

use axross_amp::engine::{Command, Engine, RenderState};
use std::time::{Duration, Instant};

const RATE: u32 = 48_000;

/// The folders of media: review 16's, and review 17's (tests/media/r17, by
/// tests/media/gen_ogg17.py: an Opus whose first page of sound ends no
/// packet, granule -1, and a recording of 70 songs - more than 64 links).
fn dirs() -> Vec<String> {
    match std::env::var("R16_OGG") {
        Ok(dir) => vec![dir],
        Err(_) => ["r16", "r17"]
            .map(|d| format!("{}/tests/media/{d}", env!("CARGO_MANIFEST_DIR")))
            .to_vec(),
    }
}

#[derive(Debug, Clone)]
struct Entry {
    name: String,
    capture: bool,
    intro: f64,
    kind: String,
    secs: f64,
    t0: f64,
    approx: f64,
    /// A chained capture: the first link's length (the second's chirp from 0).
    link1: f64,
    /// A chain of the same recording again and again (from `t0` each).
    repeat: bool,
    dir: String,
}

/// manifest.json without a JSON crate: one object per file.
fn manifest(dir: &str) -> Vec<Entry> {
    let text = std::fs::read_to_string(format!("{dir}/manifest.json")).unwrap();
    let mut out = Vec::new();
    for chunk in text.split("\"name\": \"").skip(1) {
        let name = chunk.split('"').next().unwrap().to_owned();
        // the object around it: from its opening brace to its closing one
        let at = text.find(&format!("\"name\": \"{name}\"")).unwrap();
        let start = text[..at].rfind('{').unwrap();
        let end = at + text[at..].find('}').unwrap();
        let obj = &text[start..end];
        let num = |k: &str| -> f64 {
            obj.split(&format!("\"{k}\": "))
                .nth(1)
                .and_then(|s| {
                    s.split([',', '\n', '}'])
                        .next()
                        .and_then(|v| v.trim().parse().ok())
                })
                .unwrap_or(f64::NAN)
        };
        let s = |k: &str| -> String {
            obj.split(&format!("\"{k}\": \""))
                .nth(1)
                .map(|s| s.split('"').next().unwrap().to_owned())
                .unwrap_or_default()
        };
        out.push(Entry {
            capture: obj.contains("\"capture\": true"),
            intro: num("intro"),
            kind: s("kind"),
            secs: num("secs"),
            t0: num("t0"),
            approx: num("approx_secs"),
            link1: num("link1"),
            repeat: obj.contains("\"repeat\": true"),
            dir: dir.to_owned(),
            name,
        });
    }
    out
}

fn frequency(left: &[f32]) -> f64 {
    let c = left
        .windows(2)
        .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
        .count();
    c as f64 / 2.0 / (left.len() as f64 / RATE as f64)
}

fn src_time(f: f64) -> f64 {
    (f - 200.0) / 40.0
}

/// Load at `start` s and render `secs` (as fast as the decoder goes): the
/// left channel heard, the position said after.
fn heard(path: &str, start: f64, secs: f64) -> (Vec<f32>, f64, Option<f64>) {
    let engine = Engine::start_headless(|| {});
    engine.send(Command::Load {
        path: path.to_owned(),
        start_ms: (start * 1000.0) as u64,
        play: true,
        generation: 1,
    });
    let mut state = RenderState::default();
    let mut buf = vec![0.0f32; 480 * 2];
    let mut left = Vec::new();
    let mut duration = None;
    // (wait for it to load)
    let wait = Instant::now() + Duration::from_secs(10);
    while !engine.shared.ready(480) && Instant::now() < wait {
        std::thread::sleep(Duration::from_millis(2));
    }
    for _ in 0..(secs * 100.0) as usize {
        let wait = Instant::now() + Duration::from_secs(5);
        while engine
            .shared
            .playing
            .load(std::sync::atomic::Ordering::Relaxed)
            && !engine.shared.ready(480)
            && Instant::now() < wait
        {
            std::thread::sleep(Duration::from_millis(1));
        }
        engine.shared.render(&mut buf, 2, &mut state);
        left.extend(buf.chunks(2).map(|f| f[0]));
        while let Ok(e) = engine.events.try_recv() {
            if let axross_amp::engine::Event::Loaded { info, .. } = e {
                duration = info.duration;
            }
        }
    }
    (left, engine.shared.position(), duration)
}

/// Source time heard just after the first sound: (time, at frame).
fn first_heard(left: &[f32]) -> Option<(f64, usize)> {
    let first = left.iter().position(|s| s.abs() > 0.02)?;
    let a = first + RATE as usize / 20;
    let b = a + RATE as usize / 8;
    (b <= left.len()).then(|| (src_time(frequency(&left[a..b])) - 0.05 - 1.0 / 16.0, first))
}

#[test]
fn oggs_and_their_recordings_are_timed_and_sought_as_they_are() {
    let only = std::env::var("R16_ONLY").ok();
    let mut bad = Vec::new();
    for e in dirs().iter().flat_map(|d| manifest(d)) {
        if only.as_deref().is_some_and(|o| !e.name.contains(o)) {
            continue;
        }
        let path = format!("{}/{}", e.dir, e.name);
        let links = axross_amp::seektable::ogg_links(std::path::Path::new(&path));
        let probed = axross_amp::engine::probe(&path).map(|i| i.duration);
        let (frames, _) = axross_amp::engine::decode_file(&path, RATE).unwrap();
        let decoded = frames as f64 / RATE as f64;
        let said = probed.clone().ok().flatten().unwrap_or(f64::NAN);
        let base = links.as_ref().map(|l| l[0].base).unwrap_or(0.0);
        let start_ts = symphonia_start(&path);
        let mut line = format!(
            "{:22} decoded {decoded:7.3} said {said:7.3} base {base:9.3} sym.start {start_ts:9.3} links {}",
            e.name,
            links.as_ref().map_or(0, |l| l.len())
        );
        if (said - decoded).abs() > 0.1 {
            bad.push(format!(
                "{}: said {said:.3} s, decoded {decoded:.3} s",
                e.name
            ));
        }
        // seeks: where in the source each lands (the capture's time 0 is
        // the source's t0)
        let origin = if e.capture { e.t0 } else { 0.0 };
        let length = if e.capture { e.approx } else { e.secs };
        let quiet_end = if e.capture { 0.0 } else { e.intro.max(0.0) };
        let mut targets: Vec<f64> = vec![0.0];
        for t in [quiet_end + 1.0, quiet_end + 4.0, length * 0.5, length - 3.0] {
            if t > 0.5 && t < length - 1.0 && !targets.iter().any(|x| (x - t).abs() < 0.5) {
                targets.push(t);
            }
        }
        for t in targets {
            // in silence (an intro of silence, a capture's quiet passage):
            // the sound heard after it tells nothing of where it landed
            let (left, pos, _) = heard(&path, t, if t == 0.0 { quiet_end + 1.0 } else { 0.6 });
            let Some((src, at)) = first_heard(&left) else {
                line += &format!(" | {t:.1}: nothing");
                continue;
            };
            let in_second = e.link1.is_finite() && t >= e.link1;
            // (a chain of one recording: where the link it landed in begins
            // as the player times it - its length is checked above)
            let link = links
                .as_ref()
                .and_then(|l| l.iter().rev().find(|l| l.start <= t + 0.2));
            let landed = if let (true, Some(link)) = (e.repeat, link) {
                link.start + src - origin - at as f64 / RATE as f64
            } else if in_second {
                src + e.link1 - at as f64 / RATE as f64
            } else {
                src - origin - at as f64 / RATE as f64
            };
            // (a fade's first sound above the threshold is later than the
            // fade's start: only its chirp tells where it landed)
            line += &format!(" | {t:.1}->{landed:.2} (pos {pos:.2})");
            let quiet = e.kind == "silence" || e.capture;
            if (landed - t).abs() > 0.4 && (quiet || t >= quiet_end) {
                bad.push(format!(
                    "{}: a seek to {t:.2} landed at {landed:.2}",
                    e.name
                ));
            }
            // where it says it is: the time at which it landed and played on
            let expect_pos = t + left.len() as f64 / RATE as f64;
            if (pos - expect_pos).abs() > 0.4 {
                bad.push(format!(
                    "{}: after a seek to {t:.2} and {:.2} s, said {pos:.2}",
                    e.name,
                    left.len() as f64 / RATE as f64
                ));
            }
        }
        println!("{line}");
    }
    assert!(bad.is_empty(), "misjudged:\n{}", bad.join("\n"));
}

/// symphonia's own start of the track (s): its Ogg reader parses the first
/// page's packets (Vorbis modes, Opus TOC) for it.
fn symphonia_start(path: &str) -> f64 {
    use symphonia::core::formats::probe::Hint;
    use symphonia::core::formats::{FormatOptions, TrackType};
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    let file = std::fs::File::open(path).unwrap();
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let Ok(format) = symphonia::default::get_probe().probe(
        &Hint::new(),
        mss,
        FormatOptions::default(),
        MetadataOptions::default(),
    ) else {
        return f64::NAN;
    };
    let Some(track) = format.default_track(TrackType::Audio) else {
        return f64::NAN;
    };
    let rate = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .and_then(|a| a.sample_rate)
        .unwrap_or(48_000) as f64;
    let tb_rate = track.time_base.map(|_| rate).unwrap_or(rate);
    track.start_ts.get() as f64 / tb_rate
}

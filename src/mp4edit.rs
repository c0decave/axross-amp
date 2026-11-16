//! The edit list of an MP4 / M4A file (`moov/trak/edts/elst`): where the
//! sound of the audio track begins and how long it is - the encoder delay
//! and padding of AAC (about 2112 frames at the start, some at the end).
//! symphonia does not apply edits yet, so the player trims by them itself:
//! no gap of silence before a track, gapless albums.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// (frames to drop at the start, frames that sound) in the media's own
/// time scale - which, for audio, is the sample rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edit {
    pub skip: u64,
    pub length: Option<u64>,
}

const MAX_ATOMS: usize = 10_000;

struct Atom {
    kind: [u8; 4],
    body: u64,
    end: u64,
}

fn atoms(file: &mut File, start: u64, end: u64) -> Vec<Atom> {
    let mut out = Vec::new();
    let mut pos = start;
    while pos + 8 <= end && out.len() < MAX_ATOMS {
        let mut head = [0u8; 16];
        if file.seek(SeekFrom::Start(pos)).is_err() || file.read_exact(&mut head[..8]).is_err() {
            break;
        }
        let mut size = u32::from_be_bytes(head[0..4].try_into().unwrap()) as u64;
        let kind: [u8; 4] = head[4..8].try_into().unwrap();
        let mut header = 8;
        if size == 1 {
            if file.read_exact(&mut head[8..16]).is_err() {
                break;
            }
            size = u64::from_be_bytes(head[8..16].try_into().unwrap());
            header = 16;
        } else if size == 0 {
            size = end - pos;
        }
        if size < header || pos + size > end {
            break;
        }
        out.push(Atom {
            kind,
            body: pos + header,
            end: pos + size,
        });
        pos += size;
    }
    out
}

fn read(file: &mut File, at: u64, len: usize) -> Option<Vec<u8>> {
    let mut buf = vec![0u8; len];
    file.seek(SeekFrom::Start(at)).ok()?;
    file.read_exact(&mut buf).ok()?;
    Some(buf)
}

/// The time scale of an `mvhd` or `mdhd` (version 0 or 1).
fn timescale(file: &mut File, atom: &Atom) -> Option<u32> {
    let head = read(file, atom.body, 24)?;
    let at = if head[0] == 1 { 20 } else { 12 };
    let bytes = read(file, atom.body + at, 4)?;
    Some(u32::from_be_bytes(bytes.try_into().ok()?)).filter(|t| *t > 0)
}

/// The edit of the first audio track of `path`, if it has one that trims.
pub fn read_edit(path: &Path) -> Option<Edit> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let moov = atoms(&mut file, 0, len)
        .into_iter()
        .find(|a| &a.kind == b"moov")?;
    let inside = atoms(&mut file, moov.body, moov.end);
    let movie_scale = inside
        .iter()
        .find(|a| &a.kind == b"mvhd")
        .and_then(|a| timescale(&mut file, a))?;
    for trak in inside.iter().filter(|a| &a.kind == b"trak") {
        let parts = atoms(&mut file, trak.body, trak.end);
        let Some(mdia) = parts.iter().find(|a| &a.kind == b"mdia") else {
            continue;
        };
        let media = atoms(&mut file, mdia.body, mdia.end);
        let is_audio = media
            .iter()
            .find(|a| &a.kind == b"hdlr")
            .and_then(|h| read(&mut file, h.body + 8, 4))
            .is_some_and(|t| t == b"soun");
        if !is_audio {
            continue;
        }
        let media_scale = media
            .iter()
            .find(|a| &a.kind == b"mdhd")
            .and_then(|a| timescale(&mut file, a))?;
        let edts = parts.iter().find(|a| &a.kind == b"edts")?;
        let elst = atoms(&mut file, edts.body, edts.end)
            .into_iter()
            .find(|a| &a.kind == b"elst")?;
        let head = read(&mut file, elst.body, 8)?;
        let version = head[0];
        let count = u32::from_be_bytes(head[4..8].try_into().ok()?);
        let entry = if version == 1 { 20 } else { 12 };
        // the first edit that plays media (an empty edit, media time -1, is a pause)
        for i in 0..count.min(16) as u64 {
            let e = read(&mut file, elst.body + 8 + i * entry, entry as usize)?;
            let (duration, media_time) = if version == 1 {
                (
                    u64::from_be_bytes(e[0..8].try_into().ok()?),
                    i64::from_be_bytes(e[8..16].try_into().ok()?),
                )
            } else {
                (
                    u32::from_be_bytes(e[0..4].try_into().ok()?) as u64,
                    i32::from_be_bytes(e[4..8].try_into().ok()?) as i64,
                )
            };
            if media_time < 0 {
                continue;
            }
            let length = (duration > 0)
                .then(|| (duration as u128 * media_scale as u128 / movie_scale as u128) as u64);
            if media_time == 0 && length.is_none() {
                return None;
            }
            return Some(Edit {
                skip: media_time as u64,
                length,
            });
        }
        return None;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atom(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn an_edit_list_gives_the_delay_and_the_length() {
        let mut mvhd = vec![0u8; 100];
        mvhd[12..16].copy_from_slice(&1000u32.to_be_bytes()); // movie: ms
        let mut mdhd = vec![0u8; 24];
        mdhd[12..16].copy_from_slice(&44_100u32.to_be_bytes());
        let mut hdlr = vec![0u8; 24];
        hdlr[8..12].copy_from_slice(b"soun");
        let mut elst = vec![0, 0, 0, 0, 0, 0, 0, 1];
        elst.extend_from_slice(&3000u32.to_be_bytes()); // 3 s
        elst.extend_from_slice(&2112i32.to_be_bytes());
        elst.extend_from_slice(&[0, 1, 0, 0]);
        let mdia = atom(
            b"mdia",
            &[atom(b"mdhd", &mdhd), atom(b"hdlr", &hdlr)].concat(),
        );
        let trak = atom(
            b"trak",
            &[atom(b"edts", &atom(b"elst", &elst)), mdia].concat(),
        );
        let moov = atom(b"moov", &[atom(b"mvhd", &mvhd), trak].concat());
        let file = [atom(b"ftyp", b"M4A \0\0\0\0"), moov].concat();
        let path = std::env::temp_dir().join(format!("amp-elst-{}.m4a", std::process::id()));
        std::fs::write(&path, file).unwrap();
        assert_eq!(
            read_edit(&path),
            Some(Edit {
                skip: 2112,
                length: Some(132_300)
            })
        );
        std::fs::write(&path, b"not an mp4 at all").unwrap();
        assert_eq!(read_edit(&path), None);
    }
}

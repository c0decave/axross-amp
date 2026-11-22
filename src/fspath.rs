//! File names as the playlist keeps them: text, without losing a name the
//! system holds in bytes that are no UTF-8 (`caf\xE9.flac`, a Latin-1
//! name on a Linux disk). Each such byte is kept as a character of its own
//! from Unicode's private use (U+10FE00 + the byte) and turned back into
//! that byte when the file is opened. Names in UTF-8 - nearly all - are
//! kept as they are. (Converted lossily, a name became another that did
//! not exist: the track stayed Stopped; given on the command line, it
//! panicked the program.)

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The first of the 256 characters that stand for a byte.
const BASE: u32 = 0x10FE00;

/// A path as text, losslessly.
pub fn to_string(path: &Path) -> String {
    os_to_string(path.as_os_str())
}

/// Text from the system (a name, an argument), losslessly.
pub fn os_to_string(s: &OsStr) -> String {
    if let Some(text) = s.to_str() {
        return text.to_owned();
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let mut out = String::new();
        for chunk in s.as_bytes().utf8_chunks() {
            out.push_str(chunk.valid());
            for b in chunk.invalid() {
                out.push(char::from_u32(BASE + *b as u32).unwrap_or('\u{fffd}'));
            }
        }
        out
    }
    #[cfg(not(unix))]
    {
        s.to_string_lossy().into_owned()
    }
}

/// The path a text stands for (as [`to_string`] made it).
pub fn to_path(text: &str) -> PathBuf {
    if !text.chars().any(is_byte) {
        return PathBuf::from(text);
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let mut bytes = Vec::with_capacity(text.len());
        for c in text.chars() {
            if is_byte(c) {
                bytes.push((c as u32 - BASE) as u8);
            } else {
                let mut buf = [0u8; 4];
                bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
        PathBuf::from(std::ffi::OsString::from_vec(bytes))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(text)
    }
}

/// `from` renamed over `to`. On Windows a file another program holds open
/// for a moment without sharing its deletion (a virus scanner reading the
/// new file, say) refuses the rename with "access denied": it is tried
/// again for up to half a second.
pub fn rename_over(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut tries = 0;
    loop {
        match std::fs::rename(from, to) {
            Err(e)
                if cfg!(windows)
                    && e.kind() == std::io::ErrorKind::PermissionDenied
                    && tries < 25 =>
            {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            done => return done,
        }
    }
}

/// One of this module's characters for a byte that is no UTF-8 - 0x80 to
/// 0xFF only: those for the bytes below stood for ASCII, so U+10FE2E
/// twice and U+10FE2F made "../" of a name that has none (review 13).
fn is_byte(c: char) -> bool {
    (BASE + 0x80..BASE + 256).contains(&(c as u32))
}

/// JSON as Python's `json.dumps` writes it (axross's media_player.json),
/// for a reader of standard JSON. Python holds a name that is no UTF-8 with
/// a lone surrogate for each such byte ("caf\xE9" is "caf\udce9", its
/// surrogate escape) and writes it as that escape; serde_json refuses a
/// lone surrogate, and the whole file was dropped (review 12). Each
/// `\udc80` - `\udcff` alone becomes this module's character for that
/// byte; any other lone surrogate U+FFFD; a byte that is no UTF-8 (a file
/// written raw) this module's character too. Pairs and all else as they
/// are.
pub fn json_from_python(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        text.push_str(chunk.valid());
        for b in chunk.invalid() {
            text.push(char::from_u32(BASE + *b as u32).unwrap_or('\u{fffd}'));
        }
    }
    let b = text.as_bytes();
    let hex = |at: usize| -> Option<u32> {
        let digits = std::str::from_utf8(b.get(at..at + 4)?).ok()?;
        u32::from_str_radix(digits, 16).ok()
    };
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut copied = 0;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => {
                in_string = !in_string;
                i += 1;
            }
            b'\\' if in_string && b.get(i + 1) == Some(&b'u') => {
                let Some(u) = hex(i + 2) else {
                    i += 2;
                    continue;
                };
                let paired = (0xD800..0xDC00).contains(&u)
                    && b.get(i + 6..i + 8) == Some(b"\\u")
                    && hex(i + 8).is_some_and(|l| (0xDC00..0xE000).contains(&l));
                if paired {
                    i += 12;
                    continue;
                }
                let instead = if (0xDC80..0xDD00).contains(&u) {
                    // (the byte's character, U+10FE00 + byte, as its pair)
                    let c = BASE + (u - 0xDC00) - 0x10000;
                    Some(format!(
                        "\\u{:04x}\\u{:04x}",
                        0xD800 + (c >> 10),
                        0xDC00 + (c & 0x3FF)
                    ))
                } else if (0xD800..0xE000).contains(&u) {
                    Some("\\ufffd".to_owned())
                } else {
                    None
                };
                if let Some(instead) = instead {
                    out.push_str(&text[copied..i]);
                    out.push_str(&instead);
                    copied = i + 6;
                }
                i += 6;
            }
            b'\\' if in_string => i += 2,
            _ => i += 1,
        }
    }
    out.push_str(&text[copied.min(text.len())..]);
    out
}

/// JSON for Python's `json.loads` (as [`json_from_python`] reads it back):
/// this module's character for a byte that is no UTF-8 written as the lone
/// surrogate Python's own surrogate escape uses (`\udce9`), so axross gets
/// the very name it wrote - the same bytes on the disk. (Written as the
/// character, axross could not open the file.)
pub fn json_for_python(text: &str) -> String {
    if !text.chars().any(is_byte) {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len() + 16);
    for c in text.chars() {
        match (c as u32).checked_sub(BASE) {
            Some(byte @ 0x80..=0xFF) => out.push_str(&format!("\\u{:04x}", 0xDC00 + byte)),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;

    #[test]
    fn a_name_in_latin_1_survives_the_round() {
        let raw = OsStr::from_bytes(b"/m/caf\xe9 \xff.flac");
        let text = to_string(Path::new(raw));
        assert!(text.starts_with("/m/caf"));
        assert_eq!(to_path(&text).as_os_str(), raw);
        // UTF-8 names untouched
        assert_eq!(to_string(Path::new("/m/Ça va.flac")), "/m/Ça va.flac");
        assert_eq!(to_path("/m/Ça va.flac"), PathBuf::from("/m/Ça va.flac"));
    }

    /// Python's surrogate escapes in JSON read as the bytes they stand for;
    /// written back as Python wrote them; nothing else touched.
    #[test]
    fn pythons_surrogate_escapes_go_both_ways() {
        let python = r#"{"a": "/m/caf\udce9 \udcff.flac", "b": "\ud83c\udfb5 \\udce9",
            "c": "\ud800 x \udc41", "d": "Ça \u00e9"}"#;
        let read: serde_json::Value =
            serde_json::from_str(&json_from_python(python.as_bytes())).unwrap();
        let a = read["a"].as_str().unwrap();
        assert_eq!(
            to_path(a).as_os_str(),
            OsStr::from_bytes(b"/m/caf\xe9 \xff.flac")
        );
        // (a pair is one character; an escaped backslash is no escape)
        assert_eq!(read["b"], "\u{1f3b5} \\udce9");
        // (a lone surrogate that no byte stands for: U+FFFD, not the file lost)
        assert_eq!(read["c"], "\u{fffd} x \u{fffd}");
        assert_eq!(read["d"], "Ça é");
        // written back: the escapes Python wrote, the rest as it is
        let back = json_for_python(&serde_json::to_string(&read).unwrap());
        assert!(back.contains(r"/m/caf\udce9 \udcff.flac"), "{back}");
        assert!(back.contains("Ça é"), "{back}");
        // a file written raw, bytes that are no UTF-8 in a string: the same
        let raw = b"{\"a\": \"/m/caf\xe9.flac\"}";
        let read: serde_json::Value = serde_json::from_str(&json_from_python(raw)).unwrap();
        assert_eq!(
            to_path(read["a"].as_str().unwrap()).as_os_str(),
            OsStr::from_bytes(b"/m/caf\xe9.flac")
        );
    }

    /// Only the characters for the bytes that are no UTF-8 (0x80 - 0xFF)
    /// become bytes: those for ASCII made "../" of a name (review 13, L13).
    #[test]
    fn no_character_becomes_a_byte_of_ascii() {
        let text = "/m/\u{10FE2E}\u{10FE2E}\u{10FE2F}x.flac";
        let path = to_path(text);
        assert_eq!(path, PathBuf::from(text));
        assert!(!path.as_os_str().as_bytes().windows(3).any(|w| w == b"../"));
    }
}

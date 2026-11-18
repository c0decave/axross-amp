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

fn is_byte(c: char) -> bool {
    (BASE..BASE + 256).contains(&(c as u32))
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
}

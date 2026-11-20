//! Updates from axross-amp's GitHub releases, checked before anything runs.
//!
//! Off by default: nothing goes to the network unless *Options › Check for
//! updates…* is clicked or *Check for updates at start* is ticked
//! (`AXROSS_AMP_NO_UPDATE_CHECK=1` keeps the start check off whatever the
//! setting says). A check is one request to
//! `api.github.com/repos/c0decave/axross-amp/releases/latest`.
//!
//! Which file is fetched ([`Install`], [`asset_name`]):
//!
//! | how it runs                    | the release's file                           |
//! |--------------------------------|----------------------------------------------|
//! | the Linux binary (x86-64)      | `axross-amp-X.Y.Z-linux-x86_64`              |
//! | the AppImage                   | `axross-amp-X.Y.Z-x86_64.AppImage` (replaces the AppImage file itself) |
//! | macOS (the .app or the binary) | `axross-amp-X.Y.Z-macos-universal` (the program inside the .app) |
//! | Windows x64                    | `axross-amp-X.Y.Z-windows-x64.exe`           |
//! | `cargo install`, a source tree | none: `cargo install axross-amp`, or the checkout |
//!
//! Checks, in this order, before a byte of the program is fetched or
//! anything replaced:
//!
//! * every request and every redirect goes over https to one of
//!   [`ALLOWED_HOSTS`] only, redirects are followed one by one here (at most
//!   [`MAX_REDIRECTS`]), every answer is bounded ([`MAX_API_BYTES`],
//!   [`MAX_SUMS_BYTES`], [`SIG_BYTES`], [`MAX_ASSET_BYTES`]);
//! * the release's version is strictly newer than this program's;
//! * a file's URL is the release's own download URL, its name plain;
//! * `SHA256SUMS.sig` is the raw 64-byte Ed25519 signature of the exact
//!   bytes of `SHA256SUMS`, made with a key pinned in [`TRUSTED_KEYS`] - no
//!   signature, a wrong size or a key not pinned: refused, before any line
//!   of `SHA256SUMS` is read;
//! * the program's file has an entry in the signed `SHA256SUMS`; its size is
//!   the one the release states (and below the cap), its SHA-256 the
//!   entry's (and GitHub's digest, where the API gives one).
//!
//! Replacing ([`install`]): the file is downloaded next to the program (the
//! same file system), gets its mode, the old program stays as
//! `<name>.previous` until the new one has started ([`cleanup_previous`]),
//! then one rename puts the new one in place. Windows lets a running program
//! be renamed, not overwritten: the old one is renamed to `.previous` first.
//! A folder that cannot be written keeps the program as it is; the checked
//! file is left in the cache folder, and the window says where.

use ring::digest::{Context, SHA256};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const REPOSITORY: &str = "c0decave/axross-amp";
pub const LATEST_URL: &str = "https://api.github.com/repos/c0decave/axross-amp/releases/latest";
pub const RELEASES_PAGE: &str = "https://github.com/c0decave/axross-amp/releases";
const DOWNLOAD_PREFIX: &str = "https://github.com/c0decave/axross-amp/releases/download/";
/// The only hosts a request or a redirect may reach (https only).
pub const ALLOWED_HOSTS: [&str; 4] = [
    "api.github.com",
    "github.com",
    "release-assets.githubusercontent.com",
    "objects.githubusercontent.com",
];
pub const SUMS_NAME: &str = "SHA256SUMS";
pub const SIG_NAME: &str = "SHA256SUMS.sig";
/// An Ed25519 signature is exactly this long; it is also its size bound.
pub const SIG_BYTES: u64 = 64;
pub const MAX_API_BYTES: u64 = 1 << 20;
pub const MAX_SUMS_BYTES: u64 = 64 << 10;
pub const MAX_ASSET_BYTES: u64 = 512 << 20;
pub const MAX_REDIRECTS: usize = 5;
const MAX_ASSETS: usize = 200;
const MAX_SUMS_LINES: usize = 500;
const MAX_NOTES: usize = 20_000;
const CHUNK: usize = 64 * 1024;
/// Set to 1 / true / yes / on: no check at the start, whatever the setting.
pub const NO_CHECK_ENV: &str = "AXROSS_AMP_NO_UPDATE_CHECK";
const USER_AGENT: &str = concat!("axross-amp/", env!("CARGO_PKG_VERSION"));

/// The public halves (raw 32 bytes) of the keys a `SHA256SUMS` may be
/// signed with; one verifying is enough. More than one only while a key is
/// rotated: the new key is pinned here a release before releases are signed
/// with it. (The private key never leaves the maintainer's machine:
/// RELEASING.md.)
pub const TRUSTED_KEYS: &[[u8; 32]] = &[
    // amp-update-signing, from 0.15.0 on
    // (base64 SImWOeZobpCvcq4W+VnqKmiYMCa3aDzR0Amk5MAMZt4=)
    [
        0x48, 0x89, 0x96, 0x39, 0xe6, 0x68, 0x6e, 0x90, 0xaf, 0x72, 0xae, 0x16, 0xf9, 0x59, 0xea,
        0x2a, 0x68, 0x98, 0x30, 0x26, 0xb7, 0x68, 0x3c, 0xd1, 0xd0, 0x09, 0xa4, 0xe4, 0xc0, 0x0c,
        0x66, 0xde,
    ],
];

// --------------------------------------------------------------------------
// Versions
// --------------------------------------------------------------------------

/// X.Y.Z, each at most six digits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    /// `X.Y.Z` or `vX.Y.Z`, nothing else (no pre-release, no build part).
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.strip_prefix('v').unwrap_or(text);
        let mut parts = text.split('.');
        let mut next = || -> Option<u32> {
            let p = parts.next()?;
            (!p.is_empty() && p.len() <= 6 && p.bytes().all(|b| b.is_ascii_digit()))
                .then(|| p.parse().ok())
                .flatten()
        };
        let v = Version(next()?, next()?, next()?);
        parts.next().is_none().then_some(v)
    }

    /// This program's version.
    pub fn current() -> Version {
        Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version(0, 0, 0))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

// --------------------------------------------------------------------------
// The release
// --------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// GitHub's `digest` (`sha256:<hex>`), where the API gives one.
    pub digest: Option<[u8; 32]>,
}

#[derive(Clone, Debug)]
pub struct Release {
    pub tag: String,
    pub version: Version,
    pub assets: Vec<Asset>,
    /// The release notes (plain text, bounded).
    pub notes: String,
}

impl Release {
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }

    /// The release's page on GitHub.
    pub fn page(&self) -> String {
        format!("{RELEASES_PAGE}/tag/{}", self.tag)
    }
}

/// A plain file name: letters, digits, `. _ -`, not starting with a dot.
pub fn plain_name(name: &str) -> bool {
    let b = name.as_bytes();
    !b.is_empty()
        && b.len() <= 200
        && b[0].is_ascii_alphanumeric()
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(c))
}

fn hex32(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Text from the release for the window: control and bidi characters out,
/// bounded.
pub fn clean_text(text: &str, max: usize) -> String {
    text.chars()
        .filter(|c| {
            (*c == '\n' || *c == '\t' || !c.is_control())
                && !matches!(*c as u32, 0x200b..=0x200f | 0x202a..=0x202e | 0x2066..=0x2069 | 0xfeff)
        })
        .take(max)
        .collect()
}

/// The release from the API's answer (`releases/latest`). A file whose name
/// is not plain or whose URL is not the release's own download URL is left
/// out; a draft, a pre-release or a tag that is no `vX.Y.Z` is refused.
pub fn parse_release(body: &[u8]) -> Result<Release, String> {
    let data: Value =
        serde_json::from_slice(body).map_err(|_| "GitHub's answer is not JSON".to_string())?;
    let tag = data
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or("the release has no tag")?;
    let version = Version::parse(tag)
        .filter(|v| tag == format!("v{v}"))
        .ok_or_else(|| format!("the release's tag {:?} is no version", clean_text(tag, 40)))?;
    if data.get("draft").and_then(Value::as_bool) == Some(true)
        || data.get("prerelease").and_then(Value::as_bool) == Some(true)
    {
        return Err("the latest release is a draft or a pre-release".into());
    }
    let mut assets = Vec::new();
    for raw in data
        .get("assets")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .take(MAX_ASSETS)
    {
        let (Some(name), Some(url), Some(size)) = (
            raw.get("name").and_then(Value::as_str),
            raw.get("browser_download_url").and_then(Value::as_str),
            raw.get("size").and_then(Value::as_u64),
        ) else {
            continue;
        };
        if !plain_name(name) || url != format!("{DOWNLOAD_PREFIX}{tag}/{name}") {
            continue;
        }
        let digest = raw
            .get("digest")
            .and_then(Value::as_str)
            .and_then(|d| d.strip_prefix("sha256:"))
            .and_then(hex32);
        assets.push(Asset {
            name: name.into(),
            url: url.into(),
            size,
            digest,
        });
    }
    let notes = clean_text(
        data.get("body").and_then(Value::as_str).unwrap_or(""),
        MAX_NOTES,
    );
    Ok(Release {
        tag: tag.into(),
        version,
        assets,
        notes,
    })
}

// --------------------------------------------------------------------------
// How this program runs
// --------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A release binary (Linux, macOS, Windows), the macOS .app's program.
    Binary,
    MacApp,
    AppImage,
    /// `cargo install`: updated with cargo.
    Cargo,
    /// A build in a source tree (`target/debug`, `target/release`).
    Source,
    /// No release file for this system.
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Install {
    pub kind: Kind,
    /// The file replaced: the program, or the AppImage.
    pub target: PathBuf,
    /// "linux", "macos", "windows".
    pub os: &'static str,
    pub arch: &'static str,
}

impl Install {
    /// This program.
    pub fn current() -> Install {
        let exe = std::env::current_exe().unwrap_or_default();
        let exe = fs::canonicalize(&exe).unwrap_or(exe);
        detect(
            std::env::consts::OS,
            std::env::consts::ARCH,
            &exe,
            std::env::var_os("APPIMAGE").map(PathBuf::from),
            std::env::var_os("APPDIR").map(PathBuf::from),
        )
    }

    pub fn can_update(&self) -> bool {
        matches!(self.kind, Kind::Binary | Kind::MacApp | Kind::AppImage)
    }

    /// Why this program is not updated here (None: it is).
    pub fn why_not(&self) -> Option<String> {
        match self.kind {
            Kind::Cargo => {
                Some("installed with cargo: update with \"cargo install axross-amp\"".into())
            }
            Kind::Source => {
                Some("a build from the source tree: update the checkout and build again".into())
            }
            Kind::Unsupported => Some(format!(
                "the releases have no program for {} on {}",
                self.os, self.arch
            )),
            _ => None,
        }
    }
}

/// How the program at *exe* runs. *appimage* / *appdir*: the AppImage
/// runtime's `APPIMAGE` and `APPDIR` - an AppImage only when the program
/// runs from inside `APPDIR` and `APPIMAGE` names an absolute
/// `….AppImage` (a variable set by hand changes nothing).
pub fn detect(
    os: &'static str,
    arch: &'static str,
    exe: &Path,
    appimage: Option<PathBuf>,
    appdir: Option<PathBuf>,
) -> Install {
    let install = |kind, target: &Path| Install {
        kind,
        target: target.to_path_buf(),
        os,
        arch,
    };
    let parts: Vec<String> = exe
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let n = parts.len();
    // (a build in a source tree: target/debug, target/release, also
    // target/<triple>/release and the tests' target/debug/deps)
    let in_tree = (0..n.saturating_sub(1)).any(|i| {
        parts[i] == "target"
            && parts[i + 1..n - 1]
                .iter()
                .take(2)
                .any(|p| p == "debug" || p == "release")
    });
    if in_tree {
        return install(Kind::Source, exe);
    }
    if n >= 3 && parts[n - 3] == ".cargo" && parts[n - 2] == "bin" {
        return install(Kind::Cargo, exe);
    }
    let supported = matches!(
        (os, arch),
        ("linux", "x86_64") | ("macos", _) | ("windows", "x86_64")
    );
    if !supported {
        return install(Kind::Unsupported, exe);
    }
    if os == "linux"
        && let (Some(image), Some(dir)) = (appimage, appdir)
        // (from / on: absolute on Linux - and so also in the tests on any system)
        && image.has_root()
        && image.to_string_lossy().ends_with(".AppImage")
        && exe.starts_with(&dir)
    {
        return install(Kind::AppImage, &image);
    }
    if os == "macos"
        && n >= 4
        && parts[n - 2] == "MacOS"
        && parts[n - 3] == "Contents"
        && parts[n - 4].ends_with(".app")
    {
        return install(Kind::MacApp, exe);
    }
    install(Kind::Binary, exe)
}

/// The release file for *install* at *version* (None: none is fetched).
pub fn asset_name(install: &Install, version: Version) -> Option<String> {
    match (install.kind, install.os) {
        (Kind::AppImage, _) => Some(format!("axross-amp-{version}-x86_64.AppImage")),
        (Kind::Binary | Kind::MacApp, "macos") => {
            Some(format!("axross-amp-{version}-macos-universal"))
        }
        (Kind::Binary, "linux") => Some(format!("axross-amp-{version}-linux-x86_64")),
        (Kind::Binary, "windows") => Some(format!("axross-amp-{version}-windows-x64.exe")),
        _ => None,
    }
}

// --------------------------------------------------------------------------
// The network: https to GitHub only, redirects one by one
// --------------------------------------------------------------------------

/// One answer, redirects not followed.
pub struct Reply {
    pub status: u16,
    pub location: Option<String>,
    pub length: Option<u64>,
    pub body: Box<dyn Read + Send>,
}

/// What makes one request (the real one: [`Web`]; tests a fake GitHub).
pub trait Fetch: Send + Sync {
    fn get(&self, url: &str) -> Result<Reply, String>;
}

/// The host of an https URL that may be asked: one of [`ALLOWED_HOSTS`],
/// no user part, no port but 443.
pub fn allowed_url(url: &str) -> Result<&str, String> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| "not an https address".to_string())?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return Err("an address with a user name".into());
    }
    let host = match authority.rsplit_once(':') {
        Some((host, "443")) => host,
        Some(_) => return Err("an address with a port".into()),
        None => authority,
    };
    ALLOWED_HOSTS
        .iter()
        .find(|h| h.eq_ignore_ascii_case(host))
        .copied()
        .ok_or_else(|| format!("{} is not GitHub", clean_text(host, 80)))
}

/// Fetch *url* into *sink*: at most *max* bytes, redirects followed here
/// to allowed hosts only. Returns the bytes read.
pub fn fetch_into(
    fetch: &dyn Fetch,
    url: &str,
    max: u64,
    sink: &mut dyn FnMut(&[u8]) -> Result<(), String>,
    cancel: &AtomicBool,
) -> Result<u64, String> {
    let mut url = url.to_string();
    for _ in 0..=MAX_REDIRECTS {
        let host = allowed_url(&url)?.to_string();
        let reply = fetch.get(&url)?;
        if matches!(reply.status, 301 | 302 | 303 | 307 | 308) {
            url = reply
                .location
                .filter(|l| l.starts_with("https://"))
                .ok_or_else(|| format!("{host} redirected without an https address"))?;
            continue;
        }
        match reply.status {
            200 => {}
            403 | 429 => {
                return Err(format!(
                    "{host} refused ({}): perhaps too many checks; try again in an hour",
                    reply.status
                ));
            }
            404 => return Err(format!("{host}: not found (404)")),
            status => return Err(format!("{host} answered {status}")),
        }
        if reply.length.is_some_and(|n| n > max) {
            return Err(format!("{host}: the file is larger than {max} bytes"));
        }
        let mut body = reply.body;
        let mut buf = vec![0u8; CHUNK];
        let mut total = 0u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("cancelled".into());
            }
            let n = body.read(&mut buf).map_err(|e| format!("{host}: {e}"))?;
            if n == 0 {
                return Ok(total);
            }
            total += n as u64;
            if total > max {
                return Err(format!("{host}: the file is larger than {max} bytes"));
            }
            sink(&buf[..n])?;
        }
    }
    Err(format!("more than {MAX_REDIRECTS} redirects"))
}

pub fn fetch_bytes(fetch: &dyn Fetch, url: &str, max: u64) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    fetch_into(
        fetch,
        url,
        max,
        &mut |chunk| {
            out.extend_from_slice(chunk);
            Ok(())
        },
        &AtomicBool::new(false),
    )?;
    Ok(out)
}

/// The network, through ureq: no redirect followed by it, https only,
/// time-outs on connect, answer and every read; the environment's proxy.
pub struct Web {
    agent: ureq::Agent,
}

impl Default for Web {
    fn default() -> Self {
        let config = ureq::Agent::config_builder()
            .https_only(true)
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_response(Some(Duration::from_secs(20)))
            .timeout_per_read(Some(Duration::from_secs(30)))
            .user_agent(USER_AGENT)
            .build();
        Web {
            agent: ureq::Agent::new_with_config(config),
        }
    }
}

impl Fetch for Web {
    fn get(&self, url: &str) -> Result<Reply, String> {
        let mut request = self.agent.get(url);
        if url.starts_with("https://api.github.com/") {
            request = request
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28");
        }
        let response = request.call().map_err(|e| e.to_string())?;
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        let location = header("location");
        let length = header("content-length").and_then(|v| v.trim().parse().ok());
        let status = response.status().as_u16();
        Ok(Reply {
            status,
            location,
            length,
            body: Box::new(response.into_body().into_reader()),
        })
    }
}

/// The latest release.
pub fn latest(fetch: &dyn Fetch) -> Result<Release, String> {
    parse_release(&fetch_bytes(fetch, LATEST_URL, MAX_API_BYTES)?)
}

// --------------------------------------------------------------------------
// The signature and the sums
// --------------------------------------------------------------------------

/// *signature* is a valid Ed25519 signature of *data* by one of *keys*.
pub fn verify_signature(data: &[u8], signature: &[u8], keys: &[[u8; 32]]) -> Result<(), String> {
    if signature.len() as u64 != SIG_BYTES {
        return Err(format!(
            "{SIG_NAME} is {} bytes, not an Ed25519 signature",
            signature.len()
        ));
    }
    let ok = keys.iter().any(|key| {
        ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key)
            .verify(data, signature)
            .is_ok()
    });
    if ok {
        Ok(())
    } else {
        Err(format!(
            "{SUMS_NAME} is not signed with axross-amp's release key"
        ))
    }
}

/// `SHA256SUMS` (`<hex>  <name>` or `<hex> *<name>` lines).
pub fn parse_sums(text: &str) -> Result<HashMap<String, [u8; 32]>, String> {
    let mut sums = HashMap::new();
    for line in text.lines().take(MAX_SUMS_LINES + 1) {
        if sums.len() >= MAX_SUMS_LINES {
            return Err(format!("{SUMS_NAME} has too many lines"));
        }
        if line.trim().is_empty() {
            continue;
        }
        let (digest, name) = line
            .split_once(' ')
            .ok_or_else(|| format!("{SUMS_NAME}: a line without a name"))?;
        let name = name
            .strip_prefix(' ')
            .or_else(|| name.strip_prefix('*'))
            .unwrap_or(name);
        let digest =
            hex32(digest).ok_or_else(|| format!("{SUMS_NAME}: a line without a SHA-256"))?;
        if !plain_name(name) {
            return Err(format!("{SUMS_NAME}: a file name that is not plain"));
        }
        if sums.insert(name.to_string(), digest).is_some() {
            return Err(format!("{SUMS_NAME} names a file twice"));
        }
    }
    Ok(sums)
}

// --------------------------------------------------------------------------
// Download, install
// --------------------------------------------------------------------------

/// The new program, checked, beside the old one (or in the cache folder).
#[derive(Clone, Debug)]
pub struct Downloaded {
    pub path: PathBuf,
    pub sha256: [u8; 32],
    pub version: Version,
    pub target: PathBuf,
    /// Beside the program: [`install`] can put it in place.
    pub in_place: bool,
}

/// The new program in place.
#[derive(Clone, Debug)]
pub struct Installed {
    pub path: PathBuf,
    pub previous: PathBuf,
    pub sha256: [u8; 32],
    pub version: Version,
}

fn sha256_of(path: &Path) -> std::io::Result<[u8; 32]> {
    let mut file = File::open(path)?;
    let mut context = Context::new(&SHA256);
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        context.update(&buf[..n]);
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(context.finish().as_ref());
    Ok(out)
}

fn unique() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let mut bytes = [0u8; 6];
    if ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes).is_err() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        bytes.copy_from_slice(&nanos.to_le_bytes()[..6]);
    }
    format!("{}-{}", std::process::id(), hex(&bytes))
}

/// The cache folder for a program that cannot be put in place.
pub fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("axross-amp")
        .join("update")
}

/// Fetch, check and keep the program of *release* for *install*: the
/// signature first, then the entry, the size and the SHA-256 (see the
/// module's text). *progress(done, total)* for the bar; *cancel* stops it.
/// Nothing is left behind when a check fails.
#[allow(clippy::too_many_arguments)]
pub fn download(
    release: &Release,
    install: &Install,
    current: Version,
    fetch: &dyn Fetch,
    keys: &[[u8; 32]],
    cache: &Path,
    progress: &mut dyn FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<Downloaded, String> {
    if release.version <= current {
        return Err(format!(
            "{} is not newer than this program ({current})",
            release.version
        ));
    }
    if let Some(why) = install.why_not() {
        return Err(why);
    }
    let name = asset_name(install, release.version).ok_or("no release file for this program")?;
    let asset = release
        .asset(&name)
        .ok_or_else(|| format!("the release has no {name}"))?;
    let (sums_asset, sig_asset) = match (release.asset(SUMS_NAME), release.asset(SIG_NAME)) {
        (Some(s), Some(g)) => (s, g),
        _ => return Err(format!("the release is not signed (no {SIG_NAME})")),
    };
    let signature = fetch_bytes(fetch, &sig_asset.url, SIG_BYTES)?;
    let sums = fetch_bytes(fetch, &sums_asset.url, MAX_SUMS_BYTES)?;
    verify_signature(&sums, &signature, keys)?;
    let sums =
        parse_sums(std::str::from_utf8(&sums).map_err(|_| format!("{SUMS_NAME} is not text"))?)?;
    let expected = *sums
        .get(&name)
        .ok_or_else(|| format!("{SUMS_NAME} has no entry for {name}"))?;
    if asset.digest.is_some_and(|d| d != expected) {
        return Err(format!("GitHub's digest of {name} is not the signed one"));
    }
    if asset.size == 0 || asset.size > MAX_ASSET_BYTES {
        return Err(format!(
            "{name} is {} bytes; at most {MAX_ASSET_BYTES} are fetched",
            asset.size
        ));
    }
    // beside the program (one rename puts it in place), else the cache
    let folder = install
        .target
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let temp_name = format!(".axross-amp-update-{}", unique());
    let (path, mut file, in_place) = match create_new(&folder.join(&temp_name)) {
        Ok(file) => (folder.join(&temp_name), file, true),
        Err(_) => {
            fs::create_dir_all(cache).map_err(|e| format!("{}: {e}", cache.display()))?;
            let path = cache.join(&temp_name);
            let file = create_new(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            (path, file, false)
        }
    };
    let result = (|| -> Result<[u8; 32], String> {
        let mut context = Context::new(&SHA256);
        let mut done = 0u64;
        let total = asset.size;
        let got = fetch_into(
            fetch,
            &asset.url,
            asset.size,
            &mut |chunk| {
                context.update(chunk);
                file.write_all(chunk).map_err(|e| e.to_string())?;
                done += chunk.len() as u64;
                progress(done, total);
                Ok(())
            },
            cancel,
        )?;
        if got != asset.size {
            return Err(format!("{name}: {got} of {} bytes came", asset.size));
        }
        let mut digest = [0u8; 32];
        digest.copy_from_slice(context.finish().as_ref());
        if digest != expected {
            return Err(format!("{name}: the SHA-256 is not the signed one"));
        }
        file.sync_all().map_err(|e| e.to_string())?;
        Ok(digest)
    })();
    drop(file);
    let sha256 = match result {
        Ok(d) => d,
        Err(e) => {
            let _ = fs::remove_file(&path);
            return Err(e);
        }
    };
    let (path, in_place) = if in_place {
        (path, true)
    } else {
        // (a name of its own in the cache: the user takes it from there)
        let named = cache.join(&name);
        let _ = fs::remove_file(&named);
        fs::rename(&path, &named).map_err(|e| {
            let _ = fs::remove_file(&path);
            e.to_string()
        })?;
        (named, false)
    };
    executable(&path, &install.target);
    Ok(Downloaded {
        path,
        sha256,
        version: release.version,
        target: install.target.clone(),
        in_place,
    })
}

fn create_new(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// The old program's mode (rwxr-xr-x when there is none).
fn executable(path: &Path, like: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // (the old one's read and run bits, never more than rwxr-xr-x; the
        // owner may always run it)
        let mode = fs::metadata(like)
            .map(|m| m.permissions().mode() & 0o755)
            .unwrap_or(0o755)
            | 0o700;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
    }
    #[cfg(not(unix))]
    let _ = (path, like);
}

/// The old program's name with `.previous`.
pub fn previous_of(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".previous");
    target.with_file_name(name)
}

/// Put *downloaded* in place of the program: checked once more, the old
/// program kept as `<name>.previous`, one rename.
pub fn install(downloaded: &Downloaded) -> Result<Installed, String> {
    if !downloaded.in_place {
        return Err(format!(
            "the program's folder cannot be written; the new program is in {}",
            downloaded.path.display()
        ));
    }
    if sha256_of(&downloaded.path).map_err(|e| e.to_string())? != downloaded.sha256 {
        let _ = fs::remove_file(&downloaded.path);
        return Err("the downloaded file changed since it was checked".into());
    }
    let target = &downloaded.target;
    let previous = previous_of(target);
    let fail = |e: std::io::Error| {
        let _ = fs::remove_file(&downloaded.path);
        format!("{}: {e}", target.display())
    };
    if fs::symlink_metadata(&previous).is_ok_and(|m| m.is_file()) {
        let _ = fs::remove_file(&previous);
    }
    #[cfg(not(windows))]
    {
        // the old one stays under .previous (a hard link: the same file)
        if fs::hard_link(target, &previous).is_err() {
            fs::copy(target, &previous).map_err(fail)?;
        }
        fs::rename(&downloaded.path, target).map_err(fail)?;
    }
    #[cfg(windows)]
    {
        // (a running program may be renamed, not overwritten)
        fs::rename(target, &previous).map_err(fail)?;
        if let Err(e) = fs::rename(&downloaded.path, target) {
            let _ = fs::rename(&previous, target);
            return Err(fail(e));
        }
    }
    if let Some(folder) = target.parent()
        && let Ok(dir) = File::open(folder)
    {
        let _ = dir.sync_all();
    }
    Ok(Installed {
        path: target.clone(),
        previous,
        sha256: downloaded.sha256,
        version: downloaded.version,
    })
}

/// The new program has started: the old one (`.previous`) goes. Not a
/// link, not a folder - a regular file of that name only.
pub fn cleanup_previous(target: &Path) {
    let previous = previous_of(target);
    if fs::symlink_metadata(&previous).is_ok_and(|m| m.file_type().is_file()) {
        let _ = fs::remove_file(&previous);
    }
}

/// Start the program at *path* when it is still the file checked; the
/// caller then closes this one.
pub fn restart(path: &Path, sha256: &[u8; 32]) -> Result<(), String> {
    if sha256_of(path).map_err(|e| e.to_string())? != *sha256 {
        return Err("the new program changed since it was checked; not started".into());
    }
    std::process::Command::new(path)
        .arg("--new-instance")
        .stdin(std::process::Stdio::null())
        .spawn()
        .map(drop)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Whether the start check may run: the setting, unless
/// [`NO_CHECK_ENV`] says no.
pub fn start_check_allowed(setting: bool, env: Option<&str>) -> bool {
    let off = env.is_some_and(|v| {
        ["1", "true", "yes", "on"].contains(&v.trim().to_ascii_lowercase().as_str())
    });
    setting && !off
}

#[cfg(test)]
mod tests;

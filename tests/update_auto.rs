//! The updates by themselves and the updater's checks, against a fake
//! GitHub served from memory (review 16's, its findings fixed: M3 - M5,
//! L2 - L5).

// (the program's folder, its modes and links as on Linux; the release file
// a Linux one)
#![cfg(unix)]

use axross_amp::update::*;
use axross_amp::update_ui::{Source, Stage, Updater};
use ring::signature::{Ed25519KeyPair, KeyPair};
use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const PROGRAM: &[u8] = b"the new axross-amp\x7fELF...";
const OLD: &[u8] = b"the old axross-amp";
const NAME: &str = "axross-amp-99.0.0-linux-x86_64";
const TAG: &str = "v99.0.0";

// ---------------------------------------------------------------- scaffolding

struct Dir(PathBuf);
impl Dir {
    fn new(name: &str) -> Dir {
        let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join("update-auto")
            .join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        Dir(d)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
impl std::ops::Deref for Dir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

fn keypair() -> Ed25519KeyPair {
    let rng = ring::rand::SystemRandom::new();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
    Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap()
}
fn public(key: &Ed25519KeyPair) -> [u8; 32] {
    key.public_key().as_ref().try_into().unwrap()
}
fn sha(data: &[u8]) -> String {
    hex(ring::digest::digest(&ring::digest::SHA256, data).as_ref())
}

#[derive(Clone)]
struct Canned {
    status: u16,
    location: Option<String>,
    body: Vec<u8>,
    length: Option<u64>,
}
fn ok(body: &[u8]) -> Canned {
    Canned {
        status: 200,
        location: None,
        body: body.to_vec(),
        length: Some(body.len() as u64),
    }
}
fn chunked(body: &[u8]) -> Canned {
    Canned {
        length: None,
        ..ok(body)
    }
}
fn redirect(to: &str) -> Canned {
    Canned {
        status: 302,
        location: Some(to.into()),
        body: Vec::new(),
        length: Some(0),
    }
}

#[derive(Default)]
struct FakeGitHub {
    answers: Mutex<HashMap<String, Canned>>,
    asked: Mutex<Vec<String>>,
}
impl Fetch for FakeGitHub {
    fn get(&self, url: &str) -> Result<Reply, String> {
        self.asked.lock().unwrap().push(url.to_string());
        let c = self
            .answers
            .lock()
            .unwrap()
            .get(url)
            .cloned()
            .unwrap_or(Canned {
                status: 404,
                location: None,
                body: Vec::new(),
                length: Some(0),
            });
        Ok(Reply {
            status: c.status,
            location: c.location,
            length: c.length,
            body: Box::new(Cursor::new(c.body)),
        })
    }
}
impl FakeGitHub {
    fn set(&self, url: &str, c: Canned) {
        self.answers.lock().unwrap().insert(url.into(), c);
    }
    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
    fn times(&self, url: &str) -> usize {
        self.asked().iter().filter(|u| *u == url).count()
    }
}

fn url(tag: &str, name: &str) -> String {
    format!("https://github.com/c0decave/axross-amp/releases/download/{tag}/{name}")
}
fn blob(name: &str) -> String {
    format!("https://release-assets.githubusercontent.com/github-production-release-asset/1/{name}")
}

/// Serve a release of 99.0.0: `sums` (None: the right one) signed by `key`.
fn serve(github: &FakeGitHub, key: &Ed25519KeyPair, sums: Option<String>) -> Release {
    let sums = sums.unwrap_or_else(|| format!("{}  {NAME}\n", sha(PROGRAM)));
    let sig = key.sign(sums.as_bytes());
    for (file, body) in [
        (NAME, PROGRAM.to_vec()),
        (SUMS_NAME, sums.as_bytes().to_vec()),
        (SIG_NAME, sig.as_ref().to_vec()),
    ] {
        github.set(&url(TAG, file), redirect(&blob(file)));
        github.set(&blob(file), ok(&body));
    }
    let asset = |name: &str, size: usize| serde_json::json!({"name": name, "size": size, "browser_download_url": url(TAG, name)});
    let api = serde_json::json!({
        "tag_name": TAG, "draft": false, "prerelease": false, "body": "notes",
        "assets": [asset(NAME, PROGRAM.len()), asset(SUMS_NAME, sums.len()), asset(SIG_NAME, 64)],
    });
    github.set(LATEST_URL, ok(api.to_string().as_bytes()));
    latest(github).unwrap()
}

fn linux_at(target: &Path) -> Install {
    if target.parent().is_some_and(Path::exists) {
        fs::write(target, OLD).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(target, fs::Permissions::from_mode(0o755)).unwrap();
    }
    detect("linux", "x86_64", target, None, None)
}

fn dl(
    release: &Release,
    install: &Install,
    gh: &FakeGitHub,
    keys: &[[u8; 32]],
    cache: &Path,
) -> Result<Downloaded, String> {
    download(
        release,
        install,
        Version(0, 15, 0),
        gh,
        keys,
        cache,
        &mut |_, _| {},
        &AtomicBool::new(false),
    )
}

fn no_net() -> Network {
    Network::default()
}

fn updater(
    install: Install,
    gh: Arc<FakeGitHub>,
    key: &Ed25519KeyPair,
    cache: PathBuf,
    self_test: fn(&Path, Version) -> Result<(), String>,
) -> Updater {
    Updater::with_source(
        install,
        Source {
            web: gh,
            keys: vec![public(key)],
            cache,
            self_test,
            network: no_net,
            current: Version(0, 15, 0),
        },
    )
}

/// Frames until the updater rests, at most 10 s.
fn until_rest(u: &mut Updater, ctx: &egui::Context, mode: UpdateMode) {
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end {
        u.tick(ctx, mode);
        u.poll(ctx);
        if !matches!(u.stage, Stage::Checking | Stage::Downloading(_)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the updater still works: {:?}", u.stage);
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

// ------------------------------------------------------------------ versions

#[test]
fn ok_version_tricks_are_refused() {
    let api = |tag: &str| {
        serde_json::json!({"tag_name": tag, "draft": false, "prerelease": false, "assets": []})
            .to_string()
    };
    for tag in [
        "v0.16.0-rc",
        "v0.16.0 ",
        " v0.16.0",
        "v0.16.0\n",
        "v0.016.0",
        "v00.16.0",
        "v0.16.00",
        "v1000000.0.0",
        "v0.16",
        "v0.16.0.1",
        "V0.17.0",
        "vv0.17.0",
        "v+1.0.0",
        "v-1.0.0",
        "v\u{0661}.17.0",
        "v0.17.0+build",
        "0.17.0",
        "v99.0.0/../../x",
        "",
    ] {
        assert!(
            parse_release(api(tag).as_bytes()).is_err(),
            "{tag:?} accepted"
        );
    }
    assert_eq!(
        parse_release(api("v999999.0.0").as_bytes())
            .unwrap()
            .version,
        Version(999999, 0, 0)
    );
    // prerelease / draft refused
    let pre =
        serde_json::json!({"tag_name": "v99.0.0", "prerelease": true, "assets": []}).to_string();
    assert!(parse_release(pre.as_bytes()).is_err());
}

#[test]
fn ok_older_or_equal_version_never_downloaded() {
    let dir = Dir::new("downgrade");
    let gh = FakeGitHub::default();
    let key = keypair();
    let rel = serve(&gh, &key, None);
    let install = linux_at(&dir.join("axross-amp"));
    for current in [Version(99, 0, 0), Version(99, 0, 1), Version(100, 0, 0)] {
        let before = gh.asked().len();
        let err = download(
            &rel,
            &install,
            current,
            &gh,
            &[public(&key)],
            &dir.join("c"),
            &mut |_, _| {},
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(err.contains("not newer"), "{err}");
        assert_eq!(gh.asked().len(), before, "fetched something");
    }
}

// -------------------------------------------------------------- hosts, redirects

#[test]
fn ok_hostile_urls_and_redirects_refused() {
    let bad = [
        "http://github.com/x",
        "https://github.com.evil.com/x",
        "https://objects.githubusercontent.com.evil/x",
        "https://github.com@evil.com/x",
        "https://user:pw@github.com/x",
        "https://evil.com/https://github.com/x",
        "https://evil.com#@github.com",
        "https://evil.com?@github.com",
        "https://github.com:8443/x",
        "https://github.com:/x",
        "https://github.com:0443/x",
        "https://github.com:443:443/x",
        "https://github.com./x",
        "https://xn--gthub-cta.com/x",
        "https://g\u{0456}thub.com/x",
        "https://[::1]/x",
        "https://127.0.0.1/x",
        "https://github.com%2eevil.com/x",
        "https://github.com\\.evil.com/x",
        "https://github.com\\@evil.com/x",
        "https://github.com\t.evil.com/x",
        "https://github.com /x",
        "https:/github.com/x",
        "https:///github.com/x",
        "//github.com/x",
        "/c0decave/axross-amp",
        "ftp://github.com/x",
        "HTTPS://github.com/x",
    ];
    for u in bad {
        assert!(allowed_url(u).is_err(), "{u:?} allowed");
        // and as a redirect: refused, never asked
        let gh = FakeGitHub::default();
        gh.set("https://github.com/start", redirect(u));
        gh.set(u, ok(b"evil"));
        assert!(
            fetch_bytes(&gh, "https://github.com/start", 100).is_err(),
            "{u:?} followed"
        );
        assert_eq!(
            gh.asked(),
            vec!["https://github.com/start".to_string()],
            "{u:?}"
        );
    }
    for u in [
        "https://GITHUB.com/x",
        "https://github.com:443/x",
        "https://github.com?x",
        "https://github.com#f",
    ] {
        assert!(allowed_url(u).is_ok(), "{u:?}");
    }
    // the chain: 5 redirects followed, the 6th not
    for (hops, fine) in [(5usize, true), (6, false)] {
        let gh = FakeGitHub::default();
        for i in 0..hops {
            gh.set(
                &format!("https://github.com/{i}"),
                redirect(&format!("https://github.com/{}", i + 1)),
            );
        }
        gh.set(&format!("https://github.com/{hops}"), ok(b"end"));
        assert_eq!(
            fetch_bytes(&gh, "https://github.com/0", 10).is_ok(),
            fine,
            "{hops} hops"
        );
    }
    // a redirect status without a Location, a 3xx not in the list
    let gh = FakeGitHub::default();
    gh.set(
        "https://github.com/a",
        Canned {
            location: None,
            ..redirect("")
        },
    );
    assert!(fetch_bytes(&gh, "https://github.com/a", 10).is_err());
}

// ---------------------------------------------------------------- size caps

#[test]
fn ok_size_caps_hold_and_nothing_is_left() {
    // Content-Length over the cap: refused before a byte is read
    let gh = FakeGitHub::default();
    gh.set(
        LATEST_URL,
        Canned {
            length: Some(MAX_API_BYTES + 1),
            ..ok(b"{}")
        },
    );
    assert!(latest(&gh).unwrap_err().contains("larger"));
    // no length (chunked), over the cap: cut off
    let gh = FakeGitHub::default();
    gh.set(
        LATEST_URL,
        chunked(&vec![b' '; (MAX_API_BYTES + 1) as usize]),
    );
    assert!(latest(&gh).unwrap_err().contains("larger"));

    let key = keypair();
    // SHA256SUMS over its cap (chunked): refused, the program never fetched
    let dir = Dir::new("caps");
    let gh = FakeGitHub::default();
    let rel = serve(&gh, &key, None);
    gh.set(
        &blob(SUMS_NAME),
        chunked(&vec![b'\n'; (MAX_SUMS_BYTES + 1) as usize]),
    );
    let install = linux_at(&dir.join("axross-amp"));
    let err = dl(&rel, &install, &gh, &[public(&key)], &dir.join("c")).unwrap_err();
    assert!(err.contains("larger"), "{err}");
    assert_eq!(gh.times(&blob(NAME)), 0);
    // the program: longer than stated (chunked), or cut short (length lies)
    for canned in [
        chunked(&[PROGRAM, b"more"].concat()),
        Canned {
            length: Some(PROGRAM.len() as u64),
            ..ok(&PROGRAM[..10])
        },
        chunked(&PROGRAM[..10]),
    ] {
        let gh = FakeGitHub::default();
        let rel = serve(&gh, &key, None);
        gh.set(&blob(NAME), canned);
        let err = dl(&rel, &install, &gh, &[public(&key)], &dir.join("c")).unwrap_err();
        assert!(
            err.contains("larger") || err.contains("bytes came"),
            "{err}"
        );
        assert_eq!(names(&dir), vec!["axross-amp".to_string()], "left behind");
        assert_eq!(fs::read(dir.join("axross-amp")).unwrap(), OLD);
    }
    // a size stated over 512 MiB: refused before the download
    let gh = FakeGitHub::default();
    let mut rel = serve(&gh, &key, None);
    rel.assets.iter_mut().find(|a| a.name == NAME).unwrap().size = MAX_ASSET_BYTES + 1;
    assert!(dl(&rel, &install, &gh, &[public(&key)], &dir.join("c")).is_err());
    assert_eq!(gh.times(&blob(NAME)), 0);
}

// ----------------------------------------------------- signature, sums, names

#[test]
fn ok_signature_and_sums_checks() {
    let dir = Dir::new("sig");
    let install = linux_at(&dir.join("axross-amp"));
    let key = keypair();
    let refused = |gh: &FakeGitHub, rel: &Release, keys: &[[u8; 32]]| {
        let err = dl(rel, &install, gh, keys, &dir.join("c")).unwrap_err();
        assert_eq!(fs::read(dir.join("axross-amp")).unwrap(), OLD);
        assert_eq!(names(&dir), vec!["axross-amp".to_string()], "left behind");
        err
    };
    // an untrusted key: the program never fetched
    let gh = FakeGitHub::default();
    let rel = serve(&gh, &keypair(), None);
    assert!(refused(&gh, &rel, &[public(&key)]).contains("not signed"));
    assert_eq!(gh.times(&blob(NAME)), 0);
    // no keys at all
    assert!(refused(&gh, &rel, &[]).contains("not signed"));
    // a signature of 65 bytes / 63 bytes
    for len in [63usize, 65] {
        let gh = FakeGitHub::default();
        let rel = serve(&gh, &key, None);
        gh.set(&blob(SIG_NAME), ok(&vec![0u8; len]));
        assert!(!refused(&gh, &rel, &[public(&key)]).is_empty());
    }
    // signed sums with the entry under another name only
    for other in [
        format!("{NAME}.old"),
        format!("./{NAME}"),
        format!("{NAME} "),
        "axross-amp-99.0.0-x86_64.AppImage".into(),
        NAME.to_uppercase(),
    ] {
        let gh = FakeGitHub::default();
        let rel = serve(&gh, &key, Some(format!("{}  {other}\n", sha(PROGRAM))));
        let err = refused(&gh, &rel, &[public(&key)]);
        assert!(
            err.contains("no entry") || err.contains("not plain"),
            "{other:?}: {err}"
        );
    }
    // signed sums, the entry twice
    let gh = FakeGitHub::default();
    let rel = serve(
        &gh,
        &key,
        Some(format!("{}  {NAME}\n{}  {NAME}\n", sha(PROGRAM), sha(b"x"))),
    );
    assert!(refused(&gh, &rel, &[public(&key)]).contains("twice"));
    // signed sums, another SHA-256: the file is removed
    let gh = FakeGitHub::default();
    let rel = serve(&gh, &key, Some(format!("{}  {NAME}\n", sha(b"other"))));
    assert!(refused(&gh, &rel, &[public(&key)]).contains("SHA-256"));
    // the right signed sums, the served file differs
    let gh = FakeGitHub::default();
    let rel = serve(&gh, &key, None);
    let mut evil = PROGRAM.to_vec();
    evil[0] ^= 1;
    gh.set(&blob(NAME), ok(&evil));
    assert!(refused(&gh, &rel, &[public(&key)]).contains("SHA-256"));
    // sums tampered after signing
    let gh = FakeGitHub::default();
    let rel = serve(&gh, &key, None);
    gh.set(
        &blob(SUMS_NAME),
        ok(format!("{}  {NAME}\n", sha(b"evil")).as_bytes()),
    );
    assert!(refused(&gh, &rel, &[public(&key)]).contains("not signed"));
}

#[test]
fn ok_release_json_cannot_choose_a_path_or_a_host() {
    let asset = |name: &str, url: &str| serde_json::json!({"name": name, "size": 10, "browser_download_url": url});
    let api = serde_json::json!({
    "tag_name": TAG, "assets": [
        asset(&format!("../{NAME}"), &url(TAG, &format!("../{NAME}"))),
        asset(&format!("x/{NAME}"), &url(TAG, &format!("x/{NAME}"))),
        asset(&format!(".{NAME}"), &url(TAG, &format!(".{NAME}"))),
        asset(NAME, "https://evil.com/x"),
        asset(NAME, &url("v98.0.0", NAME)),
        asset(NAME, &format!("https://github.com/someone/else/releases/download/{TAG}/{NAME}")),
        asset(NAME, &format!("{}?x", url(TAG, NAME))),
        asset(&format!("{NAME}\u{0}"), &url(TAG, &format!("{NAME}\u{0}"))),
    ]});
    let rel = parse_release(api.to_string().as_bytes()).unwrap();
    assert!(rel.assets.is_empty(), "{:?}", rel.assets);
}

// ------------------------------------------------------------- the scheduler

static NET_CALLS: AtomicUsize = AtomicUsize::new(0);
fn counting_net() -> Network {
    NET_CALLS.fetch_add(1, Ordering::SeqCst);
    Network::default()
}

#[test]
fn ok_off_asks_nothing_not_even_networkmanager() {
    let dir = Dir::new("off");
    let gh = Arc::new(FakeGitHub::default());
    let key = keypair();
    serve(&gh, &key, None);
    let asked = gh.asked().len();
    let mut u = Updater::with_source(
        linux_at(&dir.join("axross-amp")),
        Source {
            web: gh.clone(),
            keys: vec![public(&key)],
            cache: dir.join("c"),
            self_test: |_, _| Ok(()),
            network: counting_net,
            current: Version(0, 15, 0),
        },
    );
    let ctx = egui::Context::default();
    for _ in 0..30 {
        u.tick(&ctx, UpdateMode::Off);
        u.poll(&ctx);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(gh.asked().len(), asked);
    assert_eq!(NET_CALLS.load(Ordering::SeqCst), 0);
    assert!(matches!(u.stage, Stage::Idle));
}

/// Run in a child process with the variable set (it is process-wide).
#[test]
#[ignore]
fn r16_inner_env_no_check() {
    let dir = Dir::new("env-inner");
    let gh = Arc::new(FakeGitHub::default());
    let key = keypair();
    serve(&gh, &key, None);
    let asked = gh.asked().len();
    let mut u = updater(
        linux_at(&dir.join("axross-amp")),
        gh.clone(),
        &key,
        dir.join("c"),
        |_, _| Ok(()),
    );
    let ctx = egui::Context::default();
    for mode in [UpdateMode::Notify, UpdateMode::Install] {
        for _ in 0..30 {
            u.tick(&ctx, mode);
            u.poll(&ctx);
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    assert_eq!(gh.asked().len(), asked, "asked with {NO_CHECK_ENV} set");
}

#[test]
fn ok_env_variable_turns_background_checks_off() {
    let me = std::env::current_exe().unwrap();
    let run = |value: Option<&str>| {
        let mut c = std::process::Command::new(&me);
        c.args([
            "--ignored",
            "--exact",
            "r16_inner_env_no_check",
            "--test-threads=1",
        ]);
        c.env_remove(NO_CHECK_ENV);
        if let Some(v) = value {
            c.env(NO_CHECK_ENV, v);
        }
        c.output().unwrap().status.success()
    };
    assert!(run(Some("1")));
    assert!(run(Some(" Yes ")));
    assert!(!run(None), "control: without the variable it does ask");
    assert!(!run(Some("0")));
}

/// By themselves, the program's folder cannot be written: not downloaded at
/// every start (up to 512 MiB) and never put in place, silently - said
/// once a start why, the release left to be fetched by hand (review 16,
/// M5); fetched by hand, the checked file is kept in the cache and not
/// fetched again.
#[test]
fn by_themselves_an_unwritable_folder_is_said_and_nothing_fetched_again() {
    let dir = Dir::new("auto-ro");
    let gh = Arc::new(FakeGitHub::default());
    let key = keypair();
    serve(&gh, &key, None);
    let target = dir.join("not-writable/axross-amp");
    let ctx = egui::Context::default();
    let mut notices = Vec::new();
    for _start in 0..2 {
        let mut u = updater(
            linux_at(&target),
            gh.clone(),
            &key,
            dir.join("cache"),
            |_, _| Ok(()),
        );
        until_rest(&mut u, &ctx, UpdateMode::Install);
        assert!(matches!(u.stage, Stage::Available(..)), "{:?}", u.stage);
        notices.push(u.take_notice());
    }
    let fetched = gh.times(&blob(NAME));
    eprintln!("notices: {notices:?}; the program fetched {fetched} times in 2 starts");
    assert!(
        notices.iter().all(|n| n
            .as_deref()
            .is_some_and(|n| n.contains("cannot be put in place by itself"))),
        "said nothing of why: {notices:?}"
    );
    assert_eq!(
        fetched, 0,
        "fetched by itself where it cannot be put in place"
    );
    // by hand: kept in the cache, once
    let install = linux_at(&target);
    let release = latest(&*gh).unwrap();
    for _ in 0..2 {
        let got = dl(&release, &install, &gh, &[public(&key)], &dir.join("cache")).unwrap();
        assert!(!got.in_place);
    }
    assert_eq!(
        gh.times(&blob(NAME)),
        1,
        "the checked file in the cache fetched again"
    );
}

/// By themselves, the new program's self-test fails: not fetched again at
/// every start - the release is refused here for good (review 16, M4).
#[test]
fn by_themselves_a_release_that_failed_its_self_test_is_not_fetched_again() {
    let dir = Dir::new("auto-selftest");
    let gh = Arc::new(FakeGitHub::default());
    let key = keypair();
    serve(&gh, &key, None);
    let target = dir.join("axross-amp");
    let ctx = egui::Context::default();
    let mut notices = Vec::new();
    for start in 0..3 {
        let mut u = updater(
            linux_at(&target),
            gh.clone(),
            &key,
            dir.join("cache"),
            |_, _| Err("libfoo.so missing".into()),
        );
        until_rest(&mut u, &ctx, UpdateMode::Install);
        if start == 0 {
            assert!(matches!(u.stage, Stage::Failed(_)), "{:?}", u.stage);
        } else {
            assert!(matches!(u.stage, Stage::Available(_)), "{:?}", u.stage);
        }
        notices.push(u.take_notice().unwrap_or_default());
        assert_eq!(fs::read(&target).unwrap(), OLD);
    }
    let fetched = gh.times(&blob(NAME));
    eprintln!("the program fetched {fetched} times in 3 starts; notices: {notices:?}");
    assert_eq!(
        fetched, 1,
        "a release whose self-test failed here is fetched again at every start (and daily)"
    );
}

/// A release rolled back (Restart now: it ended at its start) is not put
/// in place again by itself at the next start of the old program (review
/// 16, M4).
#[test]
fn a_rolled_back_release_is_not_put_in_place_again_by_itself() {
    let dir = Dir::new("auto-rollback");
    let gh = Arc::new(FakeGitHub::default());
    let key = keypair();
    serve(&gh, &key, None);
    let target = dir.join("axross-amp");
    let ctx = egui::Context::default();
    let mut u = updater(
        linux_at(&target),
        gh.clone(),
        &key,
        dir.join("cache"),
        |_, _| Ok(()),
    );
    until_rest(&mut u, &ctx, UpdateMode::Install);
    let Stage::Ready(installed) = u.stage.clone() else {
        panic!("{:?}", u.stage)
    };
    // Restart now: the new one ended at its start -> roll_back (as update_ui::restart does)
    roll_back(&installed).unwrap();
    assert_eq!(fs::read(&target).unwrap(), OLD);
    // the old program's next start, still "install by themselves"
    let mut u = updater(
        detect("linux", "x86_64", &target, None, None),
        gh.clone(),
        &key,
        dir.join("cache"),
        |_, _| Ok(()),
    );
    until_rest(&mut u, &ctx, UpdateMode::Install);
    eprintln!(
        "after the next start: stage {:?}",
        std::mem::discriminant(&u.stage)
    );
    assert_eq!(
        fs::read(&target).unwrap(),
        OLD,
        "the release that did not start was put in place again by itself"
    );
}

/// Two players (two processes, e.g. --new-instance, macOS/Windows, no
/// session bus) both installing by themselves: the second does not put a
/// program in place over the one the first put there - it kept the new
/// program as `.previous`, the old one lost (review 16, L2).
#[test]
fn two_players_by_themselves_keep_the_old_program() {
    let dir = Dir::new("two");
    let gh = Arc::new(FakeGitHub::default());
    let key = keypair();
    serve(&gh, &key, None);
    let target = dir.join("axross-amp");
    let install = linux_at(&target);
    let ctx = egui::Context::default();
    let mut a = updater(
        install.clone(),
        gh.clone(),
        &key,
        dir.join("cache"),
        |_, _| Ok(()),
    );
    let mut b = updater(install, gh.clone(), &key, dir.join("cache"), |_, _| Ok(()));
    until_rest(&mut a, &ctx, UpdateMode::Install);
    until_rest(&mut b, &ctx, UpdateMode::Install);
    assert!(matches!(a.stage, Stage::Ready(_)), "{:?}", a.stage);
    assert!(!matches!(b.stage, Stage::Ready(_)), "{:?}", b.stage);
    let previous = fs::read(previous_of(&target)).unwrap();
    eprintln!(
        "files: {:?}; .previous is the {} program",
        names(&dir),
        if previous == OLD { "old" } else { "NEW" }
    );
    assert_eq!(
        previous, OLD,
        "the old program is no longer kept: .previous holds the new one"
    );
}

/// A second player whose program another one updated: it downloads
/// nothing, fails nothing, and says to restart - it downloaded the release
/// in full every day and failed to install it ("changed since this player
/// started"), with that notice (review 17, L4).
#[test]
fn a_second_player_after_the_first_updated_downloads_nothing() {
    let dir = Dir::new("second");
    let gh = Arc::new(FakeGitHub::default());
    let key = keypair();
    serve(&gh, &key, None);
    let target = dir.join("axross-amp");
    let install = linux_at(&target);
    let ctx = egui::Context::default();
    let mut a = updater(
        install.clone(),
        gh.clone(),
        &key,
        dir.join("cache"),
        |_, _| Ok(()),
    );
    let mut b = updater(install, gh.clone(), &key, dir.join("cache"), |_, _| Ok(()));
    until_rest(&mut a, &ctx, UpdateMode::Install);
    assert!(matches!(a.stage, Stage::Ready(_)), "{:?}", a.stage);
    let asked = gh.asked.lock().unwrap().len();
    until_rest(&mut b, &ctx, UpdateMode::Install);
    let after: Vec<String> = gh.asked.lock().unwrap()[asked..].to_vec();
    let notice = b.take_notice().unwrap_or_default();
    eprintln!("b: {:?}; asked {after:?}; said {notice:?}", b.stage);
    assert!(
        !after.iter().any(|u| u.contains(NAME)),
        "downloaded the program again: {after:?}"
    );
    assert!(!matches!(b.stage, Stage::Failed(_)), "{:?}", b.stage);
    assert!(notice.contains("restart"), "said {notice:?}");
}

/// Off chosen while a check by themselves is under way: nothing is
/// downloaded or put in place (review 16, L3).
#[test]
fn off_chosen_while_a_check_is_under_way_stops_it() {
    let dir = Dir::new("auto-off");
    let gh = Arc::new(FakeGitHub::default());
    let key = keypair();
    serve(&gh, &key, None);
    let target = dir.join("axross-amp");
    let ctx = egui::Context::default();
    let mut u = updater(
        linux_at(&target),
        gh.clone(),
        &key,
        dir.join("cache"),
        |_, _| Ok(()),
    );
    u.tick(&ctx, UpdateMode::Install);
    // the user picks Off now
    until_rest(&mut u, &ctx, UpdateMode::Off);
    eprintln!("after Off: {:?}", std::mem::discriminant(&u.stage));
    assert_eq!(
        fs::read(&target).unwrap(),
        OLD,
        "installed although Off was chosen while it was under way"
    );
}

// ------------------------------------------------------------ install details

/// `<name>.previous` a symlink (left there, or planted by whoever may write
/// the folder): the link is replaced, the file it points at untouched - a
/// copy followed it and overwrote that file (review 16, L4).
#[test]
fn a_previous_that_is_a_link_is_not_followed() {
    let dir = Dir::new("prevlink");
    let outside = Dir::new("prevlink-victim");
    let victim = outside.join("victim");
    fs::write(&victim, b"precious").unwrap();
    let target = dir.join("axross-amp");
    let install = linux_at(&target);
    std::os::unix::fs::symlink(&victim, previous_of(&target)).unwrap();
    let gh = FakeGitHub::default();
    let key = keypair();
    let rel = serve(&gh, &key, None);
    let got = dl(&rel, &install, &gh, &[public(&key)], &dir.join("c")).unwrap();
    let res = install_fn(&got);
    eprintln!(
        "install: {res:?}; victim now {:?}",
        String::from_utf8_lossy(&fs::read(&victim).unwrap())
    );
    assert_eq!(
        fs::read(&victim).unwrap(),
        b"precious",
        "the file .previous points at was overwritten"
    );
}

fn install_fn(got: &Downloaded) -> Result<Installed, String> {
    axross_amp::update::install(got, |_, _| Ok(()))
}

/// `.previous` a directory: every update fails, for good.
#[test]
fn edge_previous_directory_blocks_every_update() {
    let dir = Dir::new("prevdir");
    let target = dir.join("axross-amp");
    let install = linux_at(&target);
    fs::create_dir(previous_of(&target)).unwrap();
    let gh = FakeGitHub::default();
    let key = keypair();
    let rel = serve(&gh, &key, None);
    let got = dl(&rel, &install, &gh, &[public(&key)], &dir.join("c")).unwrap();
    let res = install_fn(&got);
    eprintln!("install with .previous a folder: {res:?}");
    assert!(res.is_err());
    assert_eq!(fs::read(&target).unwrap(), OLD);
}

// --------------------------------------------------------------- self-test

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join(name);
    fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

/// A new program whose `--self-test` leaves a child holding stdout: the
/// self-test keeps its bound - it waited for that child past
/// SELF_TEST_SECS (forever for a daemon), and the download thread, the
/// Updates window's stage and every later check with it (review 16, L5).
#[test]
fn the_self_test_keeps_its_bound_when_a_child_keeps_the_pipe() {
    let dir = Dir::new("selftest-hang");
    let v = Version(99, 0, 0);
    let p = script(
        &dir,
        "prog",
        &format!("echo '{}'\nsleep 35 &\nexit 0", self_test_answer(v)),
    );
    let start = Instant::now();
    let r = self_test(&p, v);
    let took = start.elapsed();
    eprintln!("self_test: {r:?} after {took:?} (bound {SELF_TEST_SECS} s)");
    assert!(
        took < Duration::from_secs(SELF_TEST_SECS + 3),
        "the self-test took {took:?}"
    );
}

/// A self-test printing more than a pipe's buffer before its answer: it is
/// never read while it runs, blocks, and is killed at the bound - a program
/// that works is judged "did not answer". (Safe: refused; noted only.)
#[test]
fn edge_self_test_large_output_blocks_until_killed() {
    let dir = Dir::new("selftest-flood");
    let v = Version(99, 0, 0);
    let p = script(
        &dir,
        "prog",
        &format!(
            "head -c 200000 /dev/zero | tr '\\0' x\necho\necho '{}'",
            self_test_answer(v)
        ),
    );
    let start = Instant::now();
    let r = self_test(&p, v);
    eprintln!(
        "self_test with 200 KB output: {r:?} after {:?}",
        start.elapsed()
    );
    assert!(r.is_err());
}

// ------------------------------------------------------------ the settings

/// A player of 0.15 (it always wrote "update_check", false unless ticked)
/// keeps its choice: off - "nothing goes to the network" - or, ticked,
/// said; a new installation says when one is out (review 16, M3).
#[test]
fn a_player_of_0_15_keeps_its_choice() {
    let dir = Dir::new("migrate");
    let path = dir.join("axross-amp.json");
    // what 0.15 wrote for a user who never ticked "Check for updates at start"
    fs::write(&path, r#"{"design":"modern","update_check":false}"#).unwrap();
    let s = axross_amp::state::read_amp(&path);
    eprintln!("0.15 update_check=false -> {:?}", s.update_mode);
    // what 0.16 writes back for a 0.15 player
    axross_amp::state::write_amp(&path, &s).unwrap();
    eprintln!("written back: {}", fs::read_to_string(&path).unwrap());
    assert_eq!(
        s.update_mode,
        UpdateMode::Off,
        "0.15's explicit/default 'off' (\"nothing goes to the network\") became {:?}",
        s.update_mode
    );
    let again = axross_amp::state::read_amp(&path);
    assert_eq!(
        again.update_mode,
        UpdateMode::Off,
        "written back, read again"
    );
    fs::write(&path, r#"{"design":"modern","update_check":true}"#).unwrap();
    assert_eq!(
        axross_amp::state::read_amp(&path).update_mode,
        UpdateMode::Notify
    );
    fs::write(&path, r#"{"design":"modern"}"#).unwrap();
    assert_eq!(
        axross_amp::state::read_amp(&path).update_mode,
        UpdateMode::Notify
    );
    fs::write(&path, r#"{"update_mode":"install","update_check":false}"#).unwrap();
    assert_eq!(
        axross_amp::state::read_amp(&path).update_mode,
        UpdateMode::Install
    );
}

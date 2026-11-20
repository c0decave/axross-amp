//! What a release carries besides the program: its licence, the licences of
//! the crates it is built from (made from Cargo.lock, kept up to date), and
//! on Linux its desktop entry and icon.

use std::path::Path;
#[cfg(target_os = "linux")]
use std::process::Command;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// THIRD_PARTY_LICENSES.md is what Cargo.lock gives now (a crate added or
/// updated without the file made again fails), and it carries egui's fonts'
/// licences (the OFL, the Ubuntu Font Licence, Apache-2.0 / MIT).
#[test]
#[cfg(target_os = "linux")]
fn the_third_party_licences_are_up_to_date() {
    let file = Path::new(ROOT).join("THIRD_PARTY_LICENSES.md");
    let text = std::fs::read_to_string(&file).expect("THIRD_PARTY_LICENSES.md is there");
    for what in [
        "| epaint_default_fonts |",
        "SIL OPEN FONT LICENSE",
        "UBUNTU FONT LICENCE",
        "Apache License",
        "Permission is hereby granted",
        "| egui |",
        "| symphonia |",
    ] {
        assert!(
            text.contains(what),
            "no {what:?} in THIRD_PARTY_LICENSES.md"
        );
    }
    let python = std::env::var("AMP_PYTHON").unwrap_or_else(|_| "python3".into());
    let out = Command::new(python)
        .arg(Path::new(ROOT).join("scripts/third_party_licenses.py"))
        .args([
            "--target",
            "x86_64-unknown-linux-gnu",
            "--offline",
            "--check",
        ])
        .arg(&file)
        .output()
        .expect("python runs");
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A Linux archive: the program, LICENSE, THIRD_PARTY_LICENSES.md, the
/// README, the desktop entry and the icon, in a folder of its own.
#[test]
#[cfg(target_os = "linux")]
fn a_release_archive_carries_the_licences_and_the_desktop_entry() {
    let dir = std::env::temp_dir().join(format!("amp-pack-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let binary = dir.join("axross-amp");
    std::fs::write(&binary, b"#!/bin/sh\n").unwrap();
    let out = Command::new("bash")
        .arg(Path::new(ROOT).join("scripts/pack_release.sh"))
        .arg(&binary)
        .arg("axross-amp-0.0.0-linux-x86_64")
        .arg(dir.join("dist"))
        .arg(Path::new(ROOT).join("THIRD_PARTY_LICENSES.md"))
        .arg("linux")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let list = Command::new("tar")
        .arg("-tzf")
        .arg(dir.join("dist/axross-amp-0.0.0-linux-x86_64.tar.gz"))
        .output()
        .unwrap();
    let list = String::from_utf8_lossy(&list.stdout).into_owned();
    let _ = std::fs::remove_dir_all(&dir);
    for file in [
        "axross-amp",
        "LICENSE",
        "THIRD_PARTY_LICENSES.md",
        "README.md",
        "axross-amp.desktop",
        "axross-amp.svg",
    ] {
        assert!(
            list.lines()
                .any(|l| l == format!("axross-amp-0.0.0-linux-x86_64/{file}")),
            "{file} not in the archive: {list}"
        );
    }
}

/// The jobs of a workflow file: (name, its lines), by their indentation.
fn jobs(text: &str) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line.starts_with("jobs:") {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if let Some(name) = line.strip_prefix("  ").and_then(|l| l.strip_suffix(':'))
            && !name.starts_with(' ')
            && !name.starts_with('#')
        {
            out.push((name.to_owned(), Vec::new()));
        } else if let Some(job) = out.last_mut() {
            job.1.push(line.trim().to_owned());
        }
    }
    out
}

/// Every action a workflow uses is pinned by its commit (a tag can be moved).
#[test]
fn every_action_is_pinned_by_its_commit() {
    let dir = Path::new(ROOT).join(".github/workflows");
    let mut loose = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        for line in text.lines() {
            let Some(what) = line.trim().trim_start_matches("- ").strip_prefix("uses: ") else {
                continue;
            };
            let what = what.split_whitespace().next().unwrap_or("");
            if what.starts_with("./") {
                continue;
            }
            let pinned = what.rsplit_once('@').is_some_and(|(_, sha)| {
                sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit())
            });
            if !pinned {
                loose.push(format!("{}: {what}", path.display()));
            }
        }
    }
    assert!(loose.is_empty(), "{loose:#?}");
}

/// A release is built only after the tests and the supply-chain gate pass
/// (ci.yml, called), and what it publishes gets a provenance attestation.
#[test]
fn a_release_waits_for_the_tests_and_the_gate_and_is_attested() {
    let ci = std::fs::read_to_string(Path::new(ROOT).join(".github/workflows/ci.yml")).unwrap();
    assert!(
        ci.contains("\n  workflow_call:"),
        "ci.yml cannot be called by the release"
    );
    let text =
        std::fs::read_to_string(Path::new(ROOT).join(".github/workflows/release.yml")).unwrap();
    let jobs = jobs(&text);
    let job = |name: &str| {
        jobs.iter()
            .find(|(n, _)| n == name)
            .map(|(_, l)| l.clone())
            .unwrap_or_else(|| panic!("no job {name} in release.yml"))
    };
    assert!(
        job("ci")
            .iter()
            .any(|l| l == "uses: ./.github/workflows/ci.yml"),
        "the release does not run ci.yml (tests, supply-chain gate)"
    );
    for build in ["linux", "macos", "windows"] {
        let needs = job(build)
            .into_iter()
            .find(|l| l.starts_with("needs:"))
            .unwrap_or_default();
        assert!(
            needs.contains("ci"),
            "{build} is built without the tests and the gate: {needs}"
        );
    }
    let release = job("release");
    assert!(
        release
            .iter()
            .any(|l| l.contains("actions/attest-build-provenance@"))
    );
    assert!(
        release
            .iter()
            .any(|l| l == "subject-checksums: release/SHA256SUMS")
    );
    // (ci.yml's own uploads - test builds, the gate's report - not published)
    assert!(release.iter().any(|l| l == "pattern: release-*"));
    for permission in ["id-token: write", "attestations: write", "contents: write"] {
        assert!(
            release.iter().any(|l| l == permission),
            "release lacks {permission}"
        );
    }
}

/// The release as the user's rule for assets has it (review 11): Linux
/// built on a glibc 2.34 floor (AlmaLinux 9) and checked; macOS one
/// universal binary - arm64 and x86_64, neither left out quietly - as an
/// .app in a DMG; the supply-chain gate's axross checked out at a tag.
#[test]
fn the_release_builds_what_the_asset_rule_asks() {
    let text =
        std::fs::read_to_string(Path::new(ROOT).join(".github/workflows/release.yml")).unwrap();
    let jobs = jobs(&text);
    let job = |name: &str| {
        jobs.iter()
            .find(|(n, _)| n == name)
            .map(|(_, l)| l.clone())
            .unwrap_or_else(|| panic!("no job {name}"))
    };
    let linux = job("linux");
    assert!(
        linux.iter().any(|l| l == "container: almalinux:9"),
        "Linux not built on glibc 2.34"
    );
    assert!(
        linux
            .iter()
            .any(|l| l.contains("scripts/glibc_floor.py target/release/axross-amp 2.34"))
    );
    let macos = job("macos");
    assert!(
        !macos.iter().any(|l| l.starts_with("continue-on-error")),
        "a macOS build may drop out quietly"
    );
    for want in [
        "run: cargo build --release --locked --target aarch64-apple-darwin",
        "run: cargo build --release --locked --target x86_64-apple-darwin",
        "dist/*.dmg",
    ] {
        assert!(macos.iter().any(|l| l == want), "macOS: no {want:?}");
    }
    assert!(
        macos
            .iter()
            .any(|l| l.contains("scripts/build_macos_app.sh"))
    );
    let ci = std::fs::read_to_string(Path::new(ROOT).join(".github/workflows/ci.yml")).unwrap();
    let gate = ci
        .split("repository: c0decave/axross")
        .nth(1)
        .expect("the gate's checkout of axross");
    assert!(
        gate.lines().take(8).any(|l| {
            let r = l.trim().strip_prefix("ref: ").unwrap_or("");
            r.len() == 40 && r.bytes().all(|b| b.is_ascii_hexdigit())
        }),
        "the gate's axross is not pinned to a full commit"
    );
}

/// The .app as built (without hdiutil: any system): the program, its
/// Info.plist with the version, the licences.
#[test]
#[cfg(unix)]
fn the_macos_app_is_put_together() {
    let dir = std::env::temp_dir().join(format!("amp-app-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let binary = dir.join("axross-amp");
    std::fs::write(&binary, b"#!/bin/sh\n").unwrap();
    let out = std::process::Command::new("bash")
        .arg(Path::new(ROOT).join("scripts/build_macos_app.sh"))
        .arg(&binary)
        .arg("9.8.7")
        .arg(dir.join("dist"))
        .arg(Path::new(ROOT).join("THIRD_PARTY_LICENSES.md"))
        .env("APP_ONLY", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let app = dir.join("dist/axross-amp.app/Contents");
    for file in [
        "MacOS/axross-amp",
        "Info.plist",
        "PkgInfo",
        "Resources/LICENSE",
        "Resources/THIRD_PARTY_LICENSES.md",
    ] {
        assert!(app.join(file).is_file(), "no {file} in the app");
    }
    let plist = std::fs::read_to_string(app.join("Info.plist")).unwrap();
    assert!(plist.contains("<string>9.8.7</string>"));
    assert!(plist.contains("<key>CFBundleExecutable</key>\n\t<string>axross-amp</string>"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// scripts/glibc_floor.py reads an ELF's glibc needs (this test's own
/// binary needs some glibc; a floor of 9.0 always holds, 2.0 never).
#[test]
#[cfg(target_os = "linux")]
fn the_glibc_floor_is_read_from_the_binary() {
    let me = std::env::current_exe().unwrap();
    let run = |floor: &str| {
        Command::new("python3")
            .arg(Path::new(ROOT).join("scripts/glibc_floor.py"))
            .arg(&me)
            .arg(floor)
            .output()
            .unwrap()
    };
    let high = run("9.0");
    assert!(
        high.status.success(),
        "{}",
        String::from_utf8_lossy(&high.stderr)
    );
    assert!(String::from_utf8_lossy(&high.stdout).contains("needs glibc 2."));
    assert!(!run("2.0").status.success());
}

/// When the tests fail in CI, each failing test is an annotation of its
/// own - its name, where it panicked, the message - so a failure can be
/// read from the check run alone (the job's log is not always at hand):
/// every test step keeps its output and runs scripts/test_failures.py on
/// it when it fails, and the script reads cargo's output so.
#[test]
#[cfg(target_os = "linux")]
fn a_failing_test_in_ci_is_named_in_an_annotation() {
    let ci = std::fs::read_to_string(Path::new(ROOT).join(".github/workflows/ci.yml")).unwrap();
    let steps: Vec<&str> = ci
        .split("      - ")
        .filter(|s| s.contains("cargo test"))
        .collect();
    assert_eq!(
        steps.len(),
        2,
        "the Linux and the other systems' test steps"
    );
    for step in steps {
        for want in [
            "shell: bash",
            "set -o pipefail",
            "--no-fail-fast",
            "| tee target/test-output.log",
            "scripts/test_failures.py target/test-output.log; exit 1;",
        ] {
            assert!(step.contains(want), "no {want:?} in:\n{step}");
        }
    }
    let log = "     Running unittests src/lib.rs (target/debug/deps/axross_amp-0123456789abcdef)
test playlist::tests::a ... FAILED
test playlist::tests::b ... ok

failures:

---- playlist::tests::a stdout ----
printed first

thread 'playlist::tests::a' panicked at src\\playlist.rs:1170:9:
assertion `left == right` failed
  left: [\"a, b: 100%\"]
 right: []
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    playlist::tests::a

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

     Running tests/cover_skip.rs (target/debug/deps/cover_skip-fedcba9876543210)
test skipping_never_blinks_the_cover has been running for over 60 seconds
error: test failed, to rerun pass `--test cover_skip`

Caused by:
  process didn't exit successfully: `target/debug/deps/cover_skip-fedcba9876543210` (signal: 11, SIGSEGV: invalid memory reference)
";
    let dir = std::env::temp_dir().join(format!("amp-annotations-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("test-output.log");
    std::fs::write(&file, log).unwrap();
    let out = Command::new("python3")
        .arg(Path::new(ROOT).join("scripts/test_failures.py"))
        .arg(&file)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(out.status.success());
    let lines: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(
        lines[0],
        "::error file=src/playlist.rs,line=1170,title=lib.rs%3A playlist%3A%3Atests%3A%3Aa::\
         assertion `left == right` failed%0A  left: [\"a, b: 100%25\"]%0A right: []"
    );
    assert!(
        lines.iter().any(
            |l| l.starts_with("::error title=cover_skip.rs%3A crashed::") && l.contains("SIGSEGV")
        ),
        "{lines:#?}"
    );
    assert!(
        lines.iter().any(|l| l
            == "::warning title=cover_skip.rs%3A skipping_never_blinks_the_cover::running for over 60 seconds"),
        "{lines:#?}"
    );
    // (short enough for GitHub: at most ten errors a step, each under 1 KB)
    let many: String = (0..30)
        .map(|i| {
            format!(
                "---- t{i} stdout ----\nthread 't{i}' panicked at src/x.rs:1:1:\n{}\n\n",
                "x".repeat(5000)
            )
        })
        .collect();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(&file, format!("failures:\n\n{many}failures:\n")).unwrap();
    let out = Command::new("python3")
        .arg(Path::new(ROOT).join("scripts/test_failures.py"))
        .arg(&file)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        text.lines().filter(|l| l.starts_with("::error")).count(),
        10
    );
    assert!(text.lines().all(|l| l.len() < 1100), "{text}");
    assert!(text.contains("30 tests failed"));
}

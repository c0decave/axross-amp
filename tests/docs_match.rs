//! The documentation says what the code does (review 10 found six places
//! where it did not): checked where it can be.

use std::path::Path;
use std::process::Command;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn read(file: &str) -> String {
    std::fs::read_to_string(Path::new(ROOT).join(file)).unwrap()
}

/// The words as written, line breaks and quotes set aside.
fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `--help` and the README give macOS's log path, and say a start still
/// going after two seconds exits 0 (not "exits 1" alone).
#[test]
fn the_background_start_is_described_as_it_is() {
    let help = Command::new(env!("CARGO_BIN_EXE_axross-amp"))
        .arg("--help")
        .output()
        .unwrap();
    let help = flat(&String::from_utf8_lossy(&help.stdout));
    let readme = flat(&read("docs/README-extended.md"));
    for (what, text) in [("--help", &help), ("README-extended", &readme)] {
        assert!(
            text.contains("~/Library/Caches/axross-amp/axross-amp.log"),
            "{what}: no macOS log path"
        );
        assert!(
            text.contains("~/.cache/axross-amp/axross-amp.log"),
            "{what}: no Linux log path"
        );
    }
    assert!(help.contains("(exit status 0)") && help.contains("(exit status 1)"));
    assert!(readme.contains("the exit status is 0"));
    // (the AppImage from a terminal goes to the background too)
    assert!(!readme.contains("or the AppImage it just runs"));
    assert!(readme.contains("the AppImage started from a terminal goes to the background"));
    // (the log path the code uses: on macOS dirs' cache dir, ~/Library/Caches)
    let path = axross_amp::detach::log_path();
    assert!(path.ends_with("axross-amp/axross-amp.log"));
    #[cfg(target_os = "macos")]
    assert!(path.to_string_lossy().contains("Library/Caches"));
}

/// The scheme's reds are turned toward a warm grey as light as they are
/// (unred_level), not toward a warm white, as the docs said.
#[test]
fn the_reds_are_said_to_go_toward_a_warm_grey() {
    for file in ["CHANGELOG.md", "docs/FACES.md", "src/viz/mod.rs"] {
        let text = flat(&read(file));
        assert!(
            !text.contains("a custom one are taken toward a warm white")
                && !text.contains("a custom one: each is taken toward a warm white")
                && !text.contains("a custom one: every colour taken toward a warm white"),
            "{file} says the scheme's reds go toward a warm white"
        );
        assert!(text.contains("warm grey"), "{file} does not say warm grey");
    }
}

/// No page says Glass and Soft are Modern's arrangement any more (they
/// have their own), and the designs page describes theirs.
#[test]
fn glass_and_soft_are_described_as_their_own() {
    for file in ["CHANGELOG.md", "docs/LIMITATIONS.md", "docs/DESIGNS.md"] {
        let text = flat(&read(file));
        assert!(
            !text.contains("Glass, Soft and Zen share the Modern design's arrangement")
                && !text.contains("The designs Glass, Soft and Zen share"),
            "{file} says Glass and Soft are Modern's arrangement"
        );
    }
    let designs = flat(&read("docs/DESIGNS.md"));
    assert!(designs.contains("in two columns") && designs.contains("a large play key"));
}

/// The photosensitivity page agrees with itself: four readings, one count
/// of the cases at six, and the 100 ms reading's results.
#[test]
fn the_photosensitivity_page_agrees_with_itself() {
    let text = flat(&read("docs/FLASH_SAFETY.md"));
    assert!(text.contains("Four readings; all are enforced."));
    assert!(!text.contains("Three readings; all are enforced."));
    assert!(!text.contains("all three count at most six"));
    assert!(!text.contains("Twelve cases sit at six"));
    assert!(text.contains("14 cases at the limit") && text.contains("14 cases sit at six"));
    assert!(text.contains("By a 100 ms reading of motion."));
}

/// The README opens with the screenshots, then the ten punchlines, then the
/// contents; the photosensitivity warning after them (review 11: the
/// user's order for a README), then the other pictures (review 12).
#[test]
fn the_readme_opens_with_screenshots_then_punchlines_then_contents() {
    let text = read("README.md");
    let at = |what: &str| {
        text.find(what)
            .unwrap_or_else(|| panic!("no {what:?} in the README"))
    };
    let screenshots = at("![The window](docs/window.png)");
    let punchlines = at("- **Plays almost anything**");
    let contents = at("## Contents");
    let warning = at("**Photosensitivity warning.**");
    assert!(
        screenshots < punchlines,
        "the punchlines before the screenshots"
    );
    assert!(punchlines < contents, "the contents before the punchlines");
    assert!(contents < warning, "the warning before the contents");
    // (the hero first; the gallery of the other pictures after the
    // punchlines and the contents - review 12: it lay between the hero and
    // the punchlines)
    let gallery = at("docs/designs.png");
    assert!(contents < gallery, "the gallery before the contents");
    // (macOS 15 no longer opens an app it has not checked by right-click →
    // Open: System Settings → Privacy & Security → Open Anyway)
    for doc in ["README.md", "docs/README-extended.md"] {
        let text = read(doc);
        assert!(
            !text.contains("right-click → Open"),
            "{doc}: right-click → Open"
        );
        assert!(text.contains("Open Anyway"), "{doc}: no Open Anyway");
    }
    // (ten punchlines)
    let count = text[punchlines..contents]
        .lines()
        .filter(|l| l.starts_with("- **"))
        .count();
    assert_eq!(count, 10);
}

/// `--help` names --version and the keys F11, Esc and Ctrl+F (review 11).
#[test]
fn the_help_names_every_option_and_key() {
    let help = Command::new(env!("CARGO_BIN_EXE_axross-amp"))
        .arg("--help")
        .output()
        .unwrap();
    let help = flat(&String::from_utf8_lossy(&help.stdout));
    for what in [
        "--version",
        "--foreground",
        "--new-instance",
        "F11",
        "Esc",
        "Ctrl+F",
    ] {
        assert!(help.contains(what), "--help does not name {what}");
    }
}

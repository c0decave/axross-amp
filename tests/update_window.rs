//! Options › Check for updates…: off by default, nothing asked of the
//! network unless clicked or ticked, and a build that is not a release file
//! (this test's, from the source tree) says so instead of replacing itself.

#[path = "support/player.rs"]
mod player;

use egui_kittest::kittest::Queryable;
use player::*;

fn open_options(h: &mut egui_kittest::Harness<'_, axross_amp::app::App>) {
    h.get_by_label("⚙").click();
    settle(h, 3);
    let options = h
        .get_by_role_and_label(egui::accesskit::Role::Button, "Options")
        .rect();
    h.hover_at(options.center());
    settle(h, 2);
    if h.query_by_label("Check for updates…").is_none() {
        h.get_by_role_and_label(egui::accesskit::Role::Button, "Options")
            .click();
        settle(h, 3);
    }
}

#[test]
fn the_check_is_off_by_default_and_a_source_build_is_not_replaced() {
    let dir = folder("update-window");
    let mut h = harness(&dir, Vec::new());
    settle(&mut h, 2);
    open_options(&mut h);
    // the setting: off until ticked
    assert!(!h.state().settings_update_check());
    h.get_by_label("Check for updates…").click();
    settle(&mut h, 3);
    assert!(h.query_by_label("Updates").is_some(), "the window opens");
    assert!(
        h.query_by_label_contains("Not updated here: a build from the source tree")
            .is_some()
    );
    assert!(h.query_by_label("Download and install").is_none());
}

#[test]
fn ticking_the_start_check_is_kept_in_the_settings() {
    let dir = folder("update-setting");
    let mut h = harness(&dir, Vec::new());
    settle(&mut h, 2);
    open_options(&mut h);
    h.get_by_label("Check for updates at start").click();
    settle(&mut h, 3);
    assert!(h.state().settings_update_check());
    // (written as the other settings: state.rs's round trip)
}

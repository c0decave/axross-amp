//! Options › Check for updates…: the updates said by default (looked for at
//! the start and once a day), off or put in place by themselves when
//! chosen; a build that is not a release file (this test's, from the source
//! tree) asks nothing of the network and says so instead of replacing
//! itself.

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
fn updates_are_said_by_default_and_a_source_build_is_not_replaced() {
    let dir = folder("update-window");
    let mut h = harness(&dir, Vec::new());
    settle(&mut h, 2);
    open_options(&mut h);
    // the setting: said when one is out, by default
    assert_eq!(
        h.state().settings_update_mode(),
        axross_amp::update::UpdateMode::Notify
    );
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
fn the_update_setting_is_kept_in_the_settings() {
    let dir = folder("update-setting");
    let mut h = harness(&dir, Vec::new());
    settle(&mut h, 2);
    open_options(&mut h);
    h.get_by_label("Download and install by themselves").click();
    settle(&mut h, 3);
    assert_eq!(
        h.state().settings_update_mode(),
        axross_amp::update::UpdateMode::Install
    );
    // (written as the other settings: state.rs's round trip)
}

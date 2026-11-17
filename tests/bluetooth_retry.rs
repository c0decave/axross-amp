//! The Bluetooth tab when the bus cannot be reached: it says why and offers
//! to try again - which then finds BlueZ (axross's fake on a private bus).
#![cfg(target_os = "linux")]

#[path = "support/bluez.rs"]
mod bluez;

#[path = "support/player.rs"]
mod player;

use axross_amp::app::Tab;
use egui_kittest::kittest::Queryable;
use player::*;

#[test]
fn a_bus_out_of_reach_can_be_tried_again() {
    let Some(bus) = bluez::Bus::start() else {
        eprintln!("AMP_FAKE_BLUEZ or dbus-daemon missing: skipped");
        return;
    };
    let dir = folder("bt-retry");
    // SAFETY: the only test of this binary; set before the tab connects
    unsafe {
        std::env::set_var(
            "AXROSS_AMP_BLUEZ_BUS",
            format!("unix:path={}", dir.join("no-bus").display()),
        )
    };
    let mut h = harness(&dir, vec![]);
    h.state_mut().set_tab(Tab::Bluetooth);
    let mut offered = false;
    for _ in 0..200 {
        settle(&mut h, 1);
        if h.query_by_label("Try again").is_some() {
            offered = true;
            break;
        }
    }
    assert!(offered, "no way out of an unreachable bus");
    // SAFETY: as above
    unsafe { std::env::set_var("AXROSS_AMP_BLUEZ_BUS", &bus.address) };
    h.get_by_label("Try again").click();
    let mut back = false;
    for _ in 0..200 {
        settle(&mut h, 1);
        if h.query_by_label_contains("No Bluetooth adapter").is_some() {
            back = true;
            break;
        }
    }
    assert!(back, "BlueZ was not found the second time");
}

//! Whether the desktop is light or dark, for "Follow the desktop's light
//! or dark": winit says nothing on Linux (X11 and Wayland: None), so it is
//! asked here - the XDG desktop portal's `org.freedesktop.appearance
//! color-scheme` (GNOME, KDE, and others through it), else GNOME's
//! gsettings `color-scheme` (its "Default" is light, unless the GTK theme
//! itself is a dark one), else GTK's `gtk-application-prefer-dark-theme`.
//! On a thread of its own, started only while the window follows the
//! desktop: one connection to the bus for as long as it watches, the
//! portal's `SettingChanged` waking it; without a portal, the settings are
//! read every two seconds.

use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Some(true): dark; Some(false): light; None: the desktop does not say.
pub type Shade = Arc<Mutex<Option<bool>>>;

/// Watch the desktop's shade (`bus`: the D-Bus to ask the portal on; None:
/// the session bus); `wake` when it changes (the window shows it). The
/// watch ends when the last other holder of the shade lets it go (seen at
/// the next change, or within two seconds without a portal).
pub fn watch(bus: Option<String>, wake: impl Fn() + Send + 'static) -> Shade {
    let out: Shade = Arc::new(Mutex::new(None));
    let shade = out.clone();
    let _ = std::thread::Builder::new()
        .name("amp-shade".into())
        .spawn(move || {
            #[cfg(target_os = "linux")]
            let connection = connect(bus.as_deref());
            #[cfg(target_os = "linux")]
            let mut changes = connection.as_ref().and_then(|c| {
                // (asked once: is a portal there to say when it changes)
                portal_on(c)?;
                let rule = zbus::MatchRule::builder()
                    .msg_type(zbus::message::Type::Signal)
                    .interface("org.freedesktop.portal.Settings")
                    .ok()?
                    .member("SettingChanged")
                    .ok()?
                    .build();
                zbus::blocking::MessageIterator::for_match_rule(rule, c, Some(16)).ok()
            });
            loop {
                #[cfg(target_os = "linux")]
                let now = ask_on(connection.as_ref(), bus.is_some());
                #[cfg(not(target_os = "linux"))]
                let now = ask(bus.as_deref());
                if let Ok(mut s) = shade.lock()
                    && *s != now
                {
                    *s = now;
                    wake();
                }
                // (the window gone: no one holds the other end)
                if Arc::strong_count(&shade) == 1 {
                    return;
                }
                #[cfg(target_os = "linux")]
                if let Some(changes) = changes.as_mut() {
                    // (the next change of a setting: the appearance's, or
                    // another - asked again either way)
                    match changes.next() {
                        Some(_) => continue,
                        None => return,
                    }
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        });
    out
}

/// The desktop's shade now (a connection of its own for the asking).
pub fn ask(bus: Option<&str>) -> Option<bool> {
    #[cfg(target_os = "linux")]
    {
        let connection = connect(bus);
        ask_on(connection.as_ref(), bus.is_some())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = bus;
        gsettings().or_else(gtk_settings)
    }
}

/// The portal's word on `connection`, else GNOME's, else GTK's (on a bus
/// of the tests' own, `own_bus`, the host's settings are not asked).
#[cfg(target_os = "linux")]
fn ask_on(connection: Option<&zbus::blocking::Connection>, own_bus: bool) -> Option<bool> {
    match connection.and_then(portal_on) {
        Some(1) => return Some(true),
        Some(2) => return Some(false),
        // (0: no preference - the desktop's own settings decide)
        _ => {}
    }
    if own_bus {
        return None;
    }
    gsettings().or_else(gtk_settings)
}

#[cfg(target_os = "linux")]
fn connect(bus: Option<&str>) -> Option<zbus::blocking::Connection> {
    let builder = match bus {
        Some(a) => zbus::blocking::connection::Builder::address(a).ok()?,
        None => zbus::blocking::connection::Builder::session().ok()?,
    };
    builder.build().ok()
}

/// The portal's `color-scheme`: 1 prefers dark, 2 prefers light, 0 says
/// nothing; None: no portal.
#[cfg(target_os = "linux")]
fn portal_on(connection: &zbus::blocking::Connection) -> Option<u32> {
    use zbus::zvariant::{OwnedValue, Value};
    let proxy = zbus::blocking::Proxy::new(
        connection,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Settings",
    )
    .ok()?;
    let value: OwnedValue = proxy
        .call("ReadOne", &("org.freedesktop.appearance", "color-scheme"))
        .or_else(|_| {
            // (an older portal: Read, the value in a variant of its own)
            proxy.call("Read", &("org.freedesktop.appearance", "color-scheme"))
        })
        .ok()?;
    match &*value {
        Value::U32(v) => Some(*v),
        Value::Value(inner) => match &**inner {
            Value::U32(v) => Some(*v),
            _ => None,
        },
        _ => None,
    }
}

/// GNOME's `color-scheme` and, for its "Default", the GTK theme's name.
fn gsettings() -> Option<bool> {
    let get = |key: &str| {
        crate::offload::helper(&mut std::process::Command::new("gsettings"))
            .args(["get", "org.gnome.desktop.interface", key])
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
    };
    let scheme = get("color-scheme")?;
    let theme = if scheme.contains("default") {
        get("gtk-theme").unwrap_or_default()
    } else {
        String::new()
    };
    gnome_shade(&scheme, &theme)
}

/// GNOME's words: 'prefer-dark', 'prefer-light', or 'default' - GNOME's
/// light appearance, unless the GTK theme is a dark one ("Adwaita-dark").
fn gnome_shade(scheme: &str, gtk_theme: &str) -> Option<bool> {
    match scheme.trim().trim_matches('\'') {
        "prefer-dark" => Some(true),
        "prefer-light" => Some(false),
        "default" => Some(
            gtk_theme
                .trim()
                .trim_matches('\'')
                .to_ascii_lowercase()
                .ends_with("dark"),
        ),
        _ => None,
    }
}

/// GTK's settings.ini: `gtk-application-prefer-dark-theme`.
fn gtk_settings() -> Option<bool> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".config")))?;
    for gtk in ["gtk-4.0/settings.ini", "gtk-3.0/settings.ini"] {
        if let Ok(text) = std::fs::read_to_string(config.join(gtk)) {
            for line in text.lines() {
                if let Some((k, v)) = line.split_once('=')
                    && k.trim() == "gtk-application-prefer-dark-theme"
                {
                    let v = v.trim();
                    return Some(v == "1" || v == "true");
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gnomes_words_for_light_and_dark() {
        assert_eq!(gnome_shade("'prefer-dark'\n", "'Adwaita'"), Some(true));
        assert_eq!(gnome_shade("'prefer-light'", "'Adwaita-dark'"), Some(false));
        // GNOME's own "Default" appearance is light - unless the theme
        // itself is a dark one (an older setup)
        assert_eq!(gnome_shade("'default'", "'Adwaita'"), Some(false));
        assert_eq!(gnome_shade("'default'\n", "'Adwaita-dark'\n"), Some(true));
        assert_eq!(gnome_shade("'default'", ""), Some(false));
        assert_eq!(gnome_shade("", ""), None);
    }
}

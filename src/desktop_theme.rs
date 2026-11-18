//! Whether the desktop is light or dark, for "Follow the desktop's light
//! or dark": winit says nothing on Linux (X11 and Wayland: None), so it is
//! asked here - the XDG desktop portal's `org.freedesktop.appearance
//! color-scheme` (GNOME, KDE, and others through it), else GNOME's
//! gsettings `color-scheme`, else GTK's `gtk-application-prefer-dark-theme`;
//! on a thread of its own, every two seconds (a change while the window is
//! open is followed).

use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Some(true): dark; Some(false): light; None: the desktop does not say.
pub type Shade = Arc<Mutex<Option<bool>>>;

/// Watch the desktop's shade (`bus`: the D-Bus to ask the portal on; None:
/// the session bus); `wake` when it changes (the window shows it).
pub fn watch(bus: Option<String>, wake: impl Fn() + Send + 'static) -> Shade {
    let out: Shade = Arc::new(Mutex::new(None));
    let shade = out.clone();
    let _ = std::thread::Builder::new()
        .name("amp-shade".into())
        .spawn(move || {
            loop {
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
                std::thread::sleep(Duration::from_secs(2));
            }
        });
    out
}

/// The desktop's shade now.
pub fn ask(bus: Option<&str>) -> Option<bool> {
    #[cfg(target_os = "linux")]
    if let Some(dark) = portal(bus) {
        return Some(dark);
    }
    #[cfg(not(target_os = "linux"))]
    let _ = bus;
    // (on a bus of the tests' own, the host's settings are not asked)
    if bus.is_some() {
        return None;
    }
    gsettings().or_else(gtk_settings)
}

/// The portal's word: 1 prefers dark, 2 prefers light, 0 says nothing.
#[cfg(target_os = "linux")]
fn portal(bus: Option<&str>) -> Option<bool> {
    use zbus::zvariant::{OwnedValue, Value};
    let builder = match bus {
        Some(a) => zbus::blocking::connection::Builder::address(a).ok()?,
        None => zbus::blocking::connection::Builder::session().ok()?,
    };
    let connection = builder.build().ok()?;
    let proxy = zbus::blocking::Proxy::new(
        &connection,
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
    let scheme = match &*value {
        Value::U32(v) => *v,
        Value::Value(inner) => match &**inner {
            Value::U32(v) => *v,
            _ => return None,
        },
        _ => return None,
    };
    match scheme {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

/// GNOME's `color-scheme` ('prefer-dark', 'prefer-light', 'default').
fn gsettings() -> Option<bool> {
    let out = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    scheme_word(&String::from_utf8_lossy(&out.stdout))
}

/// A `color-scheme` value in words.
fn scheme_word(text: &str) -> Option<bool> {
    let v = text.trim().trim_matches('\'');
    match v {
        "prefer-dark" => Some(true),
        "prefer-light" => Some(false),
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
        assert_eq!(scheme_word("'prefer-dark'\n"), Some(true));
        assert_eq!(scheme_word("'prefer-light'"), Some(false));
        assert_eq!(scheme_word("'default'"), None);
    }
}

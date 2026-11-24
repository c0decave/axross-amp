//! Which graphics adapter draws the window. A music player sits on a
//! laptop's battery for hours: with an integrated and a dedicated GPU a
//! machine with a battery takes the integrated one, one without (a
//! desktop: the monitor is on the dedicated card, and nothing is saved by
//! the other) the dedicated one, unless the user says which (Options →
//! Graphics: Auto, Power saving, High performance); never a software
//! rasterizer (llvmpipe, WARP) while a real GPU is there - egui's default
//! asks wgpu for the high-performance adapter. `WGPU_ADAPTER_NAME` (a part
//! of an adapter's name) and `WGPU_POWER_PREF` (`low`, `high`) decide
//! instead of the setting; `WGPU_BACKEND` (`vulkan`, `gl`, `metal`, `dx12`)
//! says which backends are asked at all (wgpu's own).
//!
//! The choice itself is [`choose`], a function of what the adapters say of
//! themselves; [`selector`] gives it to egui's wgpu setup.

use eframe::egui_wgpu::wgpu;

/// The setting (`graphics` in axross-amp.json).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Preference {
    /// By the machine ([`Preference::settled`]): Power saving where it has
    /// a battery or does not say, High performance where it has none.
    #[default]
    Auto,
    /// The integrated GPU where there is one, else whatever real GPU there
    /// is; a software adapter only when there is nothing else.
    PowerSaving,
    /// The dedicated GPU where there is one.
    HighPerformance,
}

impl Preference {
    pub const ALL: [Preference; 3] = [
        Preference::Auto,
        Preference::PowerSaving,
        Preference::HighPerformance,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Preference::Auto => "auto",
            Preference::PowerSaving => "low",
            Preference::HighPerformance => "high",
        }
    }

    pub fn from_key(key: &str) -> Preference {
        match key {
            "low" => Preference::PowerSaving,
            "high" => Preference::HighPerformance,
            _ => Preference::Auto,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Preference::Auto => "Auto",
            Preference::PowerSaving => "Power saving",
            Preference::HighPerformance => "High performance",
        }
    }

    /// The setting, unless `WGPU_POWER_PREF` says `low` or `high`.
    pub fn with_env(self) -> Preference {
        match std::env::var("WGPU_POWER_PREF").as_deref() {
            Ok("low") => Preference::PowerSaving,
            Ok("high") => Preference::HighPerformance,
            _ => self,
        }
    }

    /// Auto made up its mind: on a machine known to have no battery
    /// (`Some(false)`) High performance, else Power saving. The other two
    /// are what they are.
    pub fn settled(self, battery: Option<bool>) -> Preference {
        match (self, battery) {
            (Preference::Auto, Some(false)) => Preference::HighPerformance,
            (Preference::Auto, _) => Preference::PowerSaving,
            (other, _) => other,
        }
    }
}

/// Whether this machine runs on a battery at times: Some(false) for a
/// desktop, None where it cannot be asked (macOS, the BSDs, a Linux
/// without /sys).
pub fn has_battery() -> Option<bool> {
    #[cfg(target_os = "linux")]
    {
        battery_in(std::path::Path::new("/sys/class/power_supply"))
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
        let mut status = SYSTEM_POWER_STATUS {
            ACLineStatus: 255,
            BatteryFlag: 255,
            BatteryLifePercent: 255,
            SystemStatusFlag: 0,
            BatteryLifeTime: 0,
            BatteryFullLifeTime: 0,
        };
        // SAFETY: a pointer to a structure of the call's own type, alive
        // for the call; nothing is kept
        let answered = unsafe { GetSystemPowerStatus(&mut status) } != 0;
        match status.BatteryFlag {
            _ if !answered => None,
            // (128: no system battery; 255: unknown)
            128 => Some(false),
            255 => None,
            _ => Some(true),
        }
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        None
    }
}

/// [`has_battery`] read from Linux's list of power supplies: one of the
/// type "Battery" that is the machine's own (a mouse's or a headset's says
/// `scope` "Device"). None - not known - where the list cannot be read or
/// is empty (some sandboxes show an empty folder on what is a laptop) or
/// names no supply whose type can be read: a desktop is one that says of
/// a supply that it is the mains and of none that it is a battery.
pub fn battery_in(supplies: &std::path::Path) -> Option<bool> {
    let entries = std::fs::read_dir(supplies).ok()?;
    let read = |path: std::path::PathBuf| {
        std::fs::read_to_string(path)
            .map(|s| s.trim().to_ascii_lowercase())
            .unwrap_or_default()
    };
    let (mut battery, mut told) = (false, false);
    for e in entries.flatten() {
        let kind = read(e.path().join("type"));
        told |= !kind.is_empty();
        battery |= kind == "battery" && read(e.path().join("scope")) != "device";
    }
    told.then_some(battery)
}

/// What to say when the window's graphics could not be set up (`error`:
/// eframe's own, as it prints): what it was, and the three ways out that
/// need no window - Options cannot be reached then.
pub fn start_failed(error: &str) -> String {
    format!(
        "axross-amp: the window's graphics could not be set up: {error}\n\
         Another adapter or backend can be asked for at the start:\n  \
         WGPU_BACKEND=gl axross-amp          OpenGL in place of Vulkan / DX12 / Metal (also: vulkan, dx12, metal)\n  \
         WGPU_POWER_PREF=high axross-amp     the dedicated GPU (low: the integrated one)\n  \
         WGPU_ADAPTER_NAME=intel axross-amp  the adapter whose name holds the word\n\
         The setting itself is \"graphics\" (\"auto\", \"low\", \"high\") in {}.",
        crate::state::config_dir()
            .join(crate::state::AMP_FILE)
            .display()
    )
}

/// What kind of device an adapter is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Integrated,
    Discrete,
    /// A virtual machine's.
    Virtual,
    Other,
    /// A software rasterizer: llvmpipe, lavapipe, WARP.
    Software,
}

/// An adapter as it describes itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Adapter {
    pub name: String,
    /// "Vulkan", "Metal", "DX12", "OpenGL", ...
    pub backend: &'static str,
    pub kind: Kind,
    /// It can draw to the window (always true where that cannot be asked).
    pub usable: bool,
}

impl Adapter {
    pub fn of(info: &wgpu::AdapterInfo, usable: bool) -> Adapter {
        Adapter {
            name: info.name.clone(),
            backend: match info.backend {
                wgpu::Backend::Vulkan => "Vulkan",
                wgpu::Backend::Metal => "Metal",
                wgpu::Backend::Dx12 => "DX12",
                wgpu::Backend::Gl => "OpenGL",
                wgpu::Backend::BrowserWebGpu => "WebGPU",
                wgpu::Backend::Noop => "none",
            },
            kind: match info.device_type {
                wgpu::DeviceType::IntegratedGpu => Kind::Integrated,
                wgpu::DeviceType::DiscreteGpu => Kind::Discrete,
                wgpu::DeviceType::VirtualGpu => Kind::Virtual,
                wgpu::DeviceType::Cpu => Kind::Software,
                wgpu::DeviceType::Other => Kind::Other,
            },
            usable,
        }
    }

    pub fn software(&self) -> bool {
        self.kind == Kind::Software
    }

    /// "Vulkan – Intel(R) UHD Graphics 620".
    pub fn label(&self) -> String {
        format!("{} – {}", self.backend, self.name)
    }
}

/// Which of `adapters` draws the window (its index), None when none can.
///
/// * `named` (`WGPU_ADAPTER_NAME`): the first usable one whose name holds
///   it, whatever its kind - the user's word; where none does, as if it
///   were not set.
/// * Else by kind: Power saving (and Auto, not yet [`Preference::settled`])
///   takes an integrated GPU before a dedicated one, High performance the
///   dedicated one first; then a
///   virtual machine's, then one that does not say what it is; a software
///   rasterizer only when nothing else can draw to the window.
/// * Of two of a kind - the same GPU by two backends - the one on the
///   platform's own backend (Vulkan, Metal, DX12) before OpenGL; else the
///   first listed.
pub fn choose(adapters: &[Adapter], preference: Preference, named: Option<&str>) -> Option<usize> {
    if let Some(wanted) = named.map(str::trim).filter(|n| !n.is_empty()) {
        let wanted = wanted.to_lowercase();
        if let Some(at) = adapters
            .iter()
            .position(|a| a.usable && a.name.to_lowercase().contains(&wanted))
        {
            return Some(at);
        }
    }
    let kind = |a: &Adapter| match (a.kind, preference) {
        (Kind::Discrete, Preference::HighPerformance) => 0,
        (Kind::Integrated, Preference::HighPerformance) => 1,
        (Kind::Integrated, _) => 0,
        (Kind::Discrete, _) => 1,
        (Kind::Virtual, _) => 2,
        (Kind::Other, _) => 3,
        (Kind::Software, _) => 4,
    };
    let backend = |a: &Adapter| u8::from(a.backend == "OpenGL");
    adapters
        .iter()
        .enumerate()
        .filter(|(_, a)| a.usable)
        .min_by_key(|(at, a)| (kind(a), backend(a), *at))
        .map(|(at, _)| at)
}

/// [`choose`] for egui's wgpu setup: among the adapters wgpu lists, by the
/// setting, the machine (Auto) and the environment.
pub fn selector(preference: Preference) -> eframe::egui_wgpu::NativeAdapterSelectorMethod {
    std::sync::Arc::new(move |adapters, surface| {
        let described: Vec<Adapter> = adapters
            .iter()
            .map(|a| {
                let usable = surface.is_none_or(|s| a.is_surface_supported(s));
                Adapter::of(&a.get_info(), usable)
            })
            .collect();
        let named = std::env::var("WGPU_ADAPTER_NAME").ok();
        let preference = preference.with_env().settled(has_battery());
        choose(&described, preference, named.as_deref())
            .map(|at| adapters[at].clone())
            .ok_or_else(|| "no graphics adapter can draw to the window".to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adapter(name: &str, backend: &'static str, kind: Kind) -> Adapter {
        Adapter {
            name: name.into(),
            backend,
            kind,
            usable: true,
        }
    }

    /// A laptop with both, each by Vulkan and by OpenGL, and Mesa's
    /// software rasterizer.
    fn laptop() -> Vec<Adapter> {
        vec![
            adapter("NVIDIA GeForce RTX 3050", "Vulkan", Kind::Discrete),
            adapter("Intel(R) UHD Graphics 620", "Vulkan", Kind::Integrated),
            adapter("llvmpipe (LLVM 19.1, 256 bits)", "Vulkan", Kind::Software),
            adapter("Mesa Intel(R) UHD Graphics 620", "OpenGL", Kind::Integrated),
        ]
    }

    fn name(adapters: &[Adapter], preference: Preference, named: Option<&str>) -> String {
        choose(adapters, preference, named)
            .map(|at| adapters[at].label())
            .unwrap_or_default()
    }

    #[test]
    fn a_laptop_draws_with_its_integrated_gpu_unless_asked() {
        let l = laptop();
        for p in [Preference::Auto, Preference::PowerSaving] {
            assert_eq!(name(&l, p, None), "Vulkan – Intel(R) UHD Graphics 620");
        }
        assert_eq!(
            name(&l, Preference::HighPerformance, None),
            "Vulkan – NVIDIA GeForce RTX 3050"
        );
    }

    #[test]
    fn a_software_adapter_only_when_there_is_no_gpu() {
        // (listed first, as Mesa's can be)
        let mut l = laptop();
        l.rotate_left(2);
        assert!(l[0].software());
        for p in Preference::ALL {
            assert!(!l[choose(&l, p, None).unwrap()].software(), "{p:?}");
        }
        // one GPU only, of either kind: that one, whatever the setting
        for kind in [Kind::Discrete, Kind::Integrated, Kind::Virtual, Kind::Other] {
            let one = vec![
                adapter("llvmpipe", "Vulkan", Kind::Software),
                adapter("The GPU", "OpenGL", kind),
            ];
            for p in Preference::ALL {
                assert_eq!(choose(&one, p, None), Some(1), "{kind:?} {p:?}");
            }
        }
        // nothing else: the software one, and it is said to be one
        let soft = vec![adapter("llvmpipe", "Vulkan", Kind::Software)];
        for p in Preference::ALL {
            assert_eq!(choose(&soft, p, None), Some(0));
        }
        assert!(soft[0].software());
        assert_eq!(choose(&[], Preference::Auto, None), None);
    }

    #[test]
    fn a_gpu_that_cannot_draw_to_the_window_is_passed_over() {
        let mut l = laptop();
        l[1].usable = false;
        // (the integrated one by OpenGL, then: still not the dedicated one)
        assert_eq!(
            name(&l, Preference::Auto, None),
            "OpenGL – Mesa Intel(R) UHD Graphics 620"
        );
        l[3].usable = false;
        assert_eq!(
            name(&l, Preference::Auto, None),
            "Vulkan – NVIDIA GeForce RTX 3050"
        );
        for a in &mut l {
            a.usable = false;
        }
        assert_eq!(choose(&l, Preference::Auto, None), None);
    }

    #[test]
    fn the_platforms_own_backend_before_opengl() {
        let l = vec![
            adapter("Mesa Intel(R) UHD Graphics 620", "OpenGL", Kind::Integrated),
            adapter("Intel(R) UHD Graphics 620", "Vulkan", Kind::Integrated),
        ];
        assert_eq!(choose(&l, Preference::Auto, None), Some(1));
    }

    #[test]
    fn an_adapter_named_in_the_environment_is_taken() {
        let l = laptop();
        assert_eq!(
            name(&l, Preference::Auto, Some("nvidia")),
            "Vulkan – NVIDIA GeForce RTX 3050"
        );
        // (the user's word: a software one too)
        assert_eq!(
            name(&l, Preference::HighPerformance, Some("llvmpipe")),
            "Vulkan – llvmpipe (LLVM 19.1, 256 bits)"
        );
        // (no such adapter, or an empty name: as if it were not set)
        for named in ["Radeon", "", "  "] {
            assert_eq!(
                name(&l, Preference::Auto, Some(named)),
                "Vulkan – Intel(R) UHD Graphics 620"
            );
        }
    }

    #[test]
    fn auto_is_the_dedicated_gpu_on_a_desktop_and_the_integrated_one_on_a_laptop() {
        // (review 19, L2: Auto and Power saving were one behaviour under
        // two names, and a desktop with its monitor on the dedicated card
        // drew with the integrated one)
        let l = laptop();
        let by = |p: Preference, battery| name(&l, p.settled(battery), None);
        assert_eq!(
            by(Preference::Auto, Some(true)),
            "Vulkan – Intel(R) UHD Graphics 620"
        );
        assert_eq!(
            by(Preference::Auto, None),
            "Vulkan – Intel(R) UHD Graphics 620"
        );
        assert_eq!(
            by(Preference::Auto, Some(false)),
            "Vulkan – NVIDIA GeForce RTX 3050"
        );
        // the two said outright: whatever the machine
        for battery in [Some(true), Some(false), None] {
            assert_eq!(
                by(Preference::PowerSaving, battery),
                "Vulkan – Intel(R) UHD Graphics 620"
            );
            assert_eq!(
                by(Preference::HighPerformance, battery),
                "Vulkan – NVIDIA GeForce RTX 3050"
            );
        }
        assert_ne!(
            by(Preference::Auto, Some(false)),
            by(Preference::PowerSaving, Some(false))
        );
    }

    #[test]
    fn a_machines_battery_is_told_from_a_mouses() {
        let dir = std::env::temp_dir().join(format!("axross-amp-power-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(battery_in(&dir), None);
        let supply = |name: &str, kind: &str, scope: Option<&str>| {
            let d = dir.join(name);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("type"), format!("{kind}\n")).unwrap();
            if let Some(scope) = scope {
                std::fs::write(d.join("scope"), format!("{scope}\n")).unwrap();
            }
        };
        // (an empty list - a sandbox's - says nothing: not "a desktop")
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(battery_in(&dir), None);
        // (nor does a supply whose type cannot be read)
        std::fs::create_dir_all(dir.join("unreadable")).unwrap();
        assert_eq!(battery_in(&dir), None);
        assert_eq!(
            Preference::Auto.settled(battery_in(&dir)),
            Preference::PowerSaving
        );
        supply("AC", "Mains", None);
        supply("hidpp_battery_0", "Battery", Some("Device"));
        assert_eq!(battery_in(&dir), Some(false));
        supply("BAT0", "Battery", Some("System"));
        assert_eq!(battery_in(&dir), Some(true));
        std::fs::remove_dir_all(dir.join("BAT0")).unwrap();
        supply("BAT1", "battery", None);
        assert_eq!(battery_in(&dir), Some(true));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_start_that_fails_says_what_can_be_tried() {
        let said = start_failed(
            "Wgpu(CreateSurfaceError(CreateSurfaceError { inner: Hal(FailedToCreateSurfaceForAnyBackend({})) }))",
        );
        for word in [
            "FailedToCreateSurfaceForAnyBackend",
            "WGPU_BACKEND=gl",
            "WGPU_POWER_PREF=high",
            "WGPU_ADAPTER_NAME=",
            "\"graphics\"",
            "axross-amp.json",
        ] {
            assert!(said.contains(word), "{word}: {said}");
        }
    }

    #[test]
    fn the_setting_is_kept_by_its_key() {
        for p in Preference::ALL {
            assert_eq!(Preference::from_key(p.key()), p);
        }
        assert_eq!(Preference::from_key("something else"), Preference::Auto);
    }
}

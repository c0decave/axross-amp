//! A laptop with two GPUs (Optimus, PRIME): the integrated one draws the
//! desktop, the dedicated one sleeps - and an OpenGL or EGL program sees
//! only the first (the owner's: Intel Iris Xe + NVIDIA with its own driver,
//! no Vulkan loader, so wgpu's OpenGL: "High performance" found no dedicated
//! adapter to choose). When the Graphics setting wants the dedicated GPU
//! ([`crate::graphics::Preference`]: High performance, or Auto on a machine
//! without a battery) and the machine has one beside an integrated GPU that
//! draws the screen, the program asks for it the way the drivers want to be
//! asked - PRIME render offload - in its own environment, at the very start
//! of `main`, before any thread and before the graphics: NVIDIA's own
//! driver with `__NV_PRIME_RENDER_OFFLOAD=1`, `__GLX_VENDOR_LIBRARY_NAME=
//! nvidia`, `__VK_LAYER_NV_optimus=NVIDIA_only` (and NVIDIA's EGL vendor
//! file, for EGL); a Mesa driver (nouveau, amdgpu, radeon) with
//! `DRI_PRIME=1`. Variables the user set already are left as they are.
//!
//! If the window cannot be made that way, the program is started again in
//! its place without them ([`NO_OFFLOAD`]: `main`). [`MARK`], set beside
//! them, tells a process started from this one - the copy that goes to the
//! background from a terminal (src/detach.rs) - that they are the
//! program's, not the user's: it sets them as its own again, and keeps its
//! way back to the integrated GPU.
//!
//! The decision is a pure function of what /sys, /proc and the vendor
//! files say ([`System`], [`system_at`]) and of the environment
//! ([`decide`]). Linux only: Windows and macOS choose the GPU themselves.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Frames the window has begun to draw ([`frame_begun`]). Past the first,
/// the dedicated GPU has given a window, and a panic is the program's, not
/// the GPU's: one hours in was put on the GPU - the program started again
/// without it, the sound stopped, Options blaming the GPU (review 2 of
/// 0.17.1, L-N5).
static FRAMES_BEGUN: AtomicU64 = AtomicU64::new(0);

/// A frame of the window begun (its `ui`).
pub fn frame_begun() {
    FRAMES_BEGUN.fetch_add(1, Ordering::Relaxed);
}

/// The window's first frame drawn and shown (the second begun).
pub fn window_shown() -> bool {
    FRAMES_BEGUN.load(Ordering::Relaxed) >= 2
}

/// A GPU as /sys/class/drm tells it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gpu {
    /// The PCI vendor (0x8086 Intel, 0x10de NVIDIA, 0x1002 AMD).
    pub vendor: u32,
    /// The kernel driver (i915, xe, nvidia, nouveau, amdgpu, radeon, ...).
    pub driver: String,
    /// It draws the boot screen (the integrated one, on a laptop).
    pub boot_vga: bool,
    /// Its own memory, where the driver tells it (amdgpu's
    /// `mem_info_vram_total`): an APU's is a small carve-out of the
    /// system's, a discrete card's its own gigabytes.
    pub vram: Option<u64>,
    /// Whether a display is connected to one of its outputs (None: it has
    /// none listed - a render-only GPU, or not told).
    pub connected: Option<bool>,
}

impl Gpu {
    pub fn vendor_name(&self) -> &'static str {
        match self.vendor {
            0x8086 => "Intel",
            0x10de => "NVIDIA",
            0x1002 => "AMD",
            0x1af4 | 0x1234 | 0x15ad | 0x80ee => "a virtual GPU",
            _ => "a GPU",
        }
    }
}

/// What the machine has.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct System {
    pub gpus: Vec<Gpu>,
    /// NVIDIA's own driver is loaded (/proc/driver/nvidia).
    pub nvidia_driver: bool,
    /// NVIDIA's EGL vendor file (glvnd), if there is one.
    pub nvidia_egl: Option<PathBuf>,
    /// A Vulkan loader is installed (libvulkan).
    pub vulkan_loader: bool,
}

/// The tests' stand-in for `/` (a fixture tree of /sys and /proc): read in
/// builds with debug assertions only - the tests' - never in a release
/// (review of 0.17.1, L9).
pub const FAKE_ROOT: &str = "AXROSS_AMP_FAKE_ROOT";

/// The root the machine is read under: `/`, or (tests) [`FAKE_ROOT`]'s.
pub fn root() -> PathBuf {
    let fake = if cfg!(debug_assertions) {
        std::env::var_os(FAKE_ROOT)
    } else {
        None
    };
    fake.map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Reads the machine (Linux; elsewhere nothing).
pub fn system() -> System {
    if !cfg!(target_os = "linux") {
        return System::default();
    }
    system_at(&root())
}

/// [`system`] under `root` (the tests' fixture trees).
pub fn system_at(root: &Path) -> System {
    let drm = root.join("sys/class/drm");
    let mut gpus = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&drm) {
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            // (card0, card1 - not their connectors, card0-HDMI-A-1)
            .filter(|n| {
                n.len() > 4 && n.starts_with("card") && n[4..].chars().all(|c| c.is_ascii_digit())
            })
            .collect();
        names.sort();
        for n in names {
            let dev = drm.join(&n).join("device");
            let read = |f: &str| std::fs::read_to_string(dev.join(f)).unwrap_or_default();
            let vendor = u32::from_str_radix(read("vendor").trim().trim_start_matches("0x"), 16)
                .unwrap_or(0);
            let driver = std::fs::read_link(dev.join("driver"))
                .ok()
                .and_then(|p| p.file_name().map(|f| f.to_string_lossy().into_owned()))
                .unwrap_or_default();
            let boot_vga = read("boot_vga").trim() == "1";
            let vram = read("mem_info_vram_total").trim().parse::<u64>().ok();
            // (its connectors: card0-eDP-1, card0-HDMI-A-1 ... and whether one
            // has a display)
            let prefix = format!("{n}-");
            let statuses: Vec<String> = std::fs::read_dir(&drm)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
                .map(|e| {
                    std::fs::read_to_string(e.path().join("status"))
                        .unwrap_or_default()
                        .trim()
                        .to_owned()
                })
                .collect();
            let connected =
                (!statuses.is_empty()).then(|| statuses.iter().any(|s| s == "connected"));
            if vendor != 0 {
                gpus.push(Gpu {
                    vendor,
                    driver,
                    boot_vga,
                    vram,
                    connected,
                });
            }
        }
    }
    let nvidia_egl = [
        "usr/share/glvnd/egl_vendor.d/10_nvidia.json",
        "etc/glvnd/egl_vendor.d/10_nvidia.json",
    ]
    .iter()
    .map(|p| root.join(p))
    .find(|p| p.exists());
    let vulkan_loader = [
        "usr/lib/x86_64-linux-gnu/libvulkan.so.1",
        "usr/lib64/libvulkan.so.1",
        "usr/lib/libvulkan.so.1",
        "usr/lib/aarch64-linux-gnu/libvulkan.so.1",
    ]
    .iter()
    .any(|p| root.join(p).exists());
    System {
        gpus,
        nvidia_driver: root.join("proc/driver/nvidia/version").exists(),
        nvidia_egl,
        vulkan_loader,
    }
}

/// What to do.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Decision {
    /// Variables to set before the graphics start (none: nothing to do).
    pub set: Vec<(&'static str, String)>,
    /// What is said (Options → Graphics, `--adapters`).
    pub note: Option<String>,
}

/// The offload variables (a user's own stand; the start again without
/// offload takes them away).
pub const VARIABLES: [&str; 5] = [
    "__NV_PRIME_RENDER_OFFLOAD",
    "__GLX_VENDOR_LIBRARY_NAME",
    "__EGL_VENDOR_LIBRARY_FILENAMES",
    "__VK_LAYER_NV_optimus",
    "DRI_PRIME",
];

/// Set beside the offload variables by the program itself: a process
/// started from it (the copy that goes to the background) knows they are
/// the program's, not the user's - and keeps its way back to the
/// integrated GPU.
pub const MARK: &str = "AXROSS_AMP_OFFLOAD";

/// Set when the program is started again without asking for the dedicated
/// GPU (it gave no window).
pub const NO_OFFLOAD: &str = "AXROSS_AMP_NO_OFFLOAD";

/// The decision: `dedicated` whether the setting wants the dedicated GPU,
/// `env(name)` the environment.
pub fn decide(sys: &System, dedicated: bool, env: &dyn Fn(&str) -> Option<String>) -> Decision {
    // (the boot GPU draws the screen: offload only away from an
    // integrated one - an Intel GPU, or an AMD APU beside an NVIDIA card
    // or a larger AMD card; a discrete card drawing the screen with the
    // processor's GPU still on is left alone)
    let Some(boot) = sys.gpus.iter().find(|g| g.boot_vga) else {
        return Decision::default();
    };
    let other = sys
        .gpus
        .iter()
        .find(|g| *g != boot && matches!(g.vendor, 0x10de | 0x1002 | 0x8086));
    let Some(dgpu) = other else {
        // (one GPU, a virtual one, or none told: nothing to choose)
        return Decision::default();
    };
    let igpu = boot;
    // (the GPU that drew the boot screen draws no display, the other one
    // does - a desktop whose BIOS made the processor's GPU the first while
    // the monitor is on the card: DRI_PRIME would move the drawing to the
    // processor's GPU - review of 0.17.1, L5)
    if boot.connected == Some(false) && dgpu.connected == Some(true) {
        return Decision {
            set: Vec::new(),
            note: Some(format!(
                "Two GPUs: the display is on the {} one - nothing to ask for",
                dgpu.vendor_name()
            )),
        };
    }
    // (a GPU bound to another driver than a graphics one - vfio-pci, handed
    // to a virtual machine - is none to draw with: L6)
    const DRM: [&str; 6] = ["nvidia", "nouveau", "amdgpu", "radeon", "i915", "xe"];
    if !dgpu.driver.is_empty() && !DRM.contains(&dgpu.driver.as_str()) {
        return Decision {
            set: Vec::new(),
            note: Some(format!(
                "Two GPUs: the {} one is held by {} (not a graphics driver) - nothing to ask for",
                dgpu.vendor_name(),
                dgpu.driver
            )),
        };
    }
    // (an AMD APU beside an AMD card - both amdgpu - is told by its memory:
    // a carve-out smaller than the card's own)
    let amd_apu = boot.vendor == 0x1002
        && dgpu.vendor == 0x1002
        && matches!((boot.vram, dgpu.vram), (Some(a), Some(b)) if a < b);
    let boot_integrated =
        boot.vendor == 0x8086 || (boot.vendor == 0x1002 && dgpu.vendor == 0x10de) || amd_apu;
    if !boot_integrated {
        return Decision {
            set: Vec::new(),
            note: Some(format!(
                "Two GPUs: {} draws the screen already - nothing to ask for",
                boot.vendor_name()
            )),
        };
    }
    // (the program's own, from the process it was started by: not the
    // user's)
    let ours = env(MARK).is_some();
    let user_set: Vec<&str> = VARIABLES
        .iter()
        .copied()
        .filter(|n| !ours && env(n).is_some())
        .collect();
    if !user_set.is_empty() {
        return Decision {
            set: Vec::new(),
            note: Some(format!(
                "Two GPUs ({} and {}): the environment chooses ({} set) - left as it is",
                igpu.vendor_name(),
                dgpu.vendor_name(),
                user_set.join(", ")
            )),
        };
    }
    if !dedicated {
        return Decision {
            set: Vec::new(),
            note: Some(format!(
                "Two GPUs: the integrated one ({}) is used, as the setting says",
                igpu.vendor_name()
            )),
        };
    }
    if dgpu.vendor == 0x10de && (dgpu.driver == "nvidia" || sys.nvidia_driver) {
        let mut set = vec![
            (MARK, "1".to_string()),
            ("__NV_PRIME_RENDER_OFFLOAD", "1".to_string()),
            ("__GLX_VENDOR_LIBRARY_NAME", "nvidia".to_string()),
            ("__VK_LAYER_NV_optimus", "NVIDIA_only".to_string()),
        ];
        // (OpenGL through EGL - wgpu's, and Wayland's - asks glvnd for
        // NVIDIA's EGL vendor by its file)
        if let Some(f) = &sys.nvidia_egl {
            set.push((
                "__EGL_VENDOR_LIBRARY_FILENAMES",
                f.to_string_lossy().into_owned(),
            ));
        }
        let mut note = format!(
            "Dedicated GPU (NVIDIA, its own driver) asked for through PRIME render offload{}",
            if sys.nvidia_egl.is_none() {
                " - NVIDIA's EGL vendor file was not found, OpenGL may stay on the integrated GPU"
            } else {
                ""
            }
        );
        if !sys.vulkan_loader {
            note.push_str(
                "; no Vulkan loader is installed (Debian and Ubuntu: libvulkan1, Fedora: \
                 vulkan-loader, Arch: vulkan-icd-loader) - drawn with OpenGL",
            );
        }
        return Decision {
            set,
            note: Some(note),
        };
    }
    // a Mesa driver for the dedicated GPU (nouveau, amdgpu, radeon): DRI_PRIME
    Decision {
        set: vec![(MARK, "1".to_string()), ("DRI_PRIME", "1".to_string())],
        note: Some(format!(
            "Dedicated GPU ({}, driver {}) asked for through DRI_PRIME",
            dgpu.vendor_name(),
            if dgpu.driver.is_empty() {
                "unknown"
            } else {
                &dgpu.driver
            }
        )),
    }
}

/// The note of the decision taken at the start (Options → Graphics,
/// `--adapters`).
pub static NOTE: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

/// At the very start of `main`, before any thread (the environment is still
/// this thread's alone) and before the graphics: the offload variables set
/// where the setting (`graphics`) wants the dedicated GPU and the machine
/// has one to offload to; the note kept for Options and `--adapters`.
/// Whether anything was set - then a window that cannot be made is made
/// again without them ([`again_without`]).
pub fn prefer_dedicated(graphics: &str) -> Start {
    // (started again without the offload: said once, taken out of the
    // environment - a "Restart now" for an update, a helper, inherited it
    // and never tried the dedicated GPU, nor watched the update's first
    // start - review of 0.17.1, M3)
    let again = std::env::var_os(NO_OFFLOAD).is_some();
    if again {
        // SAFETY: at the very start of main, before any thread exists
        unsafe { std::env::remove_var(NO_OFFLOAD) };
    }
    let sys = system();
    let root = root();
    let battery = if cfg!(target_os = "linux") {
        crate::graphics::battery_in(&root.join("sys/class/power_supply"))
    } else {
        crate::graphics::has_battery()
    };
    let wanted = crate::graphics::Preference::from_key(graphics)
        .with_env()
        .settled(battery)
        == crate::graphics::Preference::HighPerformance;
    let decision = decide(&sys, wanted && !again, &|name| std::env::var(name).ok());
    for (name, value) in &decision.set {
        // SAFETY: at the very start of main, before any thread of this
        // program exists (no other thread reads the environment)
        unsafe { std::env::set_var(name, value) };
    }
    let note = if again && sys.gpus.len() > 1 {
        Some("The dedicated GPU gave no window: the integrated one is used".to_string())
    } else {
        decision.note
    };
    let _ = NOTE.set(note);
    Start {
        offloaded: !decision.set.is_empty(),
        again,
    }
}

/// What [`prefer_dedicated`] did at the start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Start {
    /// The offload variables set (a window that fails is made again
    /// without them).
    pub offloaded: bool,
    /// This is the start again without them.
    pub again: bool,
}

/// A helper program started by this one (gsettings, xrandr, pactl, a new
/// release): without the offload variables the program set for itself -
/// a program drawing with GL would wake the dedicated GPU (review of
/// 0.17.1, L8). The user's own stand.
pub fn helper(command: &mut std::process::Command) -> &mut std::process::Command {
    command.env_remove(NO_OFFLOAD);
    if std::env::var_os(MARK).is_some() {
        command.env_remove(MARK);
        for name in VARIABLES {
            command.env_remove(name);
        }
    }
    command
}

/// The command that starts this program again in its place without the
/// offload (its arguments; [`NO_OFFLOAD`] set, the variables and [`MARK`]
/// taken away - the rest of the environment as it is: a detached child
/// stays one, its pipe to the parent open).
pub fn again_without(exe: &Path, args: &[std::ffi::OsString]) -> std::process::Command {
    let mut command = std::process::Command::new(exe);
    command.args(args).env(NO_OFFLOAD, "1").env_remove(MARK);
    for name in VARIABLES {
        command.env_remove(name);
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu(vendor: u32, driver: &str, boot: bool) -> Gpu {
        Gpu {
            vendor,
            driver: driver.into(),
            boot_vga: boot,
            vram: None,
            connected: None,
        }
    }

    /// A fixture tree of /sys/class/drm (and /proc, the vendor files):
    /// (card, vendor, driver, boot_vga, vram).
    fn tree(
        root: &Path,
        cards: &[(&str, &str, &str, &str, Option<&str>)],
        nvidia: bool,
        egl: bool,
        vulkan: bool,
    ) {
        for (card, vendor, driver, boot, vram) in cards {
            let dev = root.join("sys/class/drm").join(card).join("device");
            std::fs::create_dir_all(&dev).unwrap();
            std::fs::write(dev.join("vendor"), format!("{vendor}\n")).unwrap();
            std::fs::write(dev.join("boot_vga"), format!("{boot}\n")).unwrap();
            if let Some(v) = vram {
                std::fs::write(dev.join("mem_info_vram_total"), format!("{v}\n")).unwrap();
            }
            let drv = root.join("sys/bus/pci/drivers").join(driver);
            std::fs::create_dir_all(&drv).unwrap();
            #[cfg(unix)]
            std::os::unix::fs::symlink(&drv, dev.join("driver")).unwrap();
            // (a connector is no GPU)
            std::fs::create_dir_all(root.join("sys/class/drm").join(format!("{card}-HDMI-A-1")))
                .unwrap();
        }
        if nvidia {
            std::fs::create_dir_all(root.join("proc/driver/nvidia")).unwrap();
            std::fs::write(root.join("proc/driver/nvidia/version"), "NVRM 550").unwrap();
        }
        if egl {
            let d = root.join("usr/share/glvnd/egl_vendor.d");
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("10_nvidia.json"), "{}").unwrap();
        }
        if vulkan {
            let d = root.join("usr/lib/x86_64-linux-gnu");
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("libvulkan.so.1"), "").unwrap();
        }
    }

    fn names(d: &Decision) -> Vec<&str> {
        d.set.iter().map(|(n, _)| *n).collect()
    }

    #[test]
    fn the_owners_laptop_asks_for_its_nvidia_gpu_through_prime_offload() {
        // Intel Iris Xe draws the screen, NVIDIA with its own driver, its
        // EGL vendor file, no Vulkan loader
        let dir = crate::testdir::Scratch::new("offload-owner");
        tree(
            &dir,
            &[
                ("card0", "0x8086", "i915", "1", None),
                ("card1", "0x10de", "nvidia", "0", None),
            ],
            true,
            true,
            false,
        );
        let sys = system_at(&dir);
        assert_eq!(sys.gpus.len(), 2, "{sys:?}");
        assert!(sys.gpus[0].boot_vga && sys.gpus[0].vendor == 0x8086);
        #[cfg(unix)]
        assert_eq!(sys.gpus[1].driver, "nvidia");
        assert!(sys.nvidia_driver && sys.nvidia_egl.is_some() && !sys.vulkan_loader);
        let none = |_: &str| None;
        let d = decide(&sys, true, &none);
        assert_eq!(
            names(&d),
            [
                MARK,
                "__NV_PRIME_RENDER_OFFLOAD",
                "__GLX_VENDOR_LIBRARY_NAME",
                "__VK_LAYER_NV_optimus",
                "__EGL_VENDOR_LIBRARY_FILENAMES"
            ]
        );
        let value = |n: &str| d.set.iter().find(|(k, _)| *k == n).unwrap().1.clone();
        assert_eq!(value("__NV_PRIME_RENDER_OFFLOAD"), "1");
        assert_eq!(value("__GLX_VENDOR_LIBRARY_NAME"), "nvidia");
        assert_eq!(value("__VK_LAYER_NV_optimus"), "NVIDIA_only");
        assert!(value("__EGL_VENDOR_LIBRARY_FILENAMES").ends_with("10_nvidia.json"));
        assert!(d.note.as_deref().unwrap().contains("libvulkan1"));
        // Power saving (or Auto on a battery): nothing set, the integrated
        // GPU said
        let low = decide(&sys, false, &none);
        assert!(low.set.is_empty() && low.note.unwrap().contains("integrated one (Intel)"));
        // the user's own variables stand
        let user = |n: &str| (n == "__NV_PRIME_RENDER_OFFLOAD").then(|| "0".to_string());
        let d = decide(&sys, true, &user);
        assert!(d.set.is_empty() && d.note.unwrap().contains("left as it is"));
    }

    #[test]
    fn intel_with_nouveau_asks_through_dri_prime() {
        let dir = crate::testdir::Scratch::new("offload-nouveau");
        tree(
            &dir,
            &[
                ("card0", "0x8086", "i915", "1", None),
                ("card1", "0x10de", "nouveau", "0", None),
            ],
            false,
            false,
            true,
        );
        let d = decide(&system_at(&dir), true, &|_| None);
        assert_eq!(
            d.set,
            vec![(MARK, "1".to_string()), ("DRI_PRIME", "1".to_string())]
        );
        #[cfg(unix)]
        assert!(d.note.unwrap().contains("nouveau"));
    }

    #[test]
    fn an_amd_apu_and_an_amd_card_are_told_apart_by_their_memory() {
        // a Ryzen APU (512 MB carved out) draws the screen, a Radeon RX
        // (8 GB) sleeps: DRI_PRIME
        let dir = crate::testdir::Scratch::new("offload-amd");
        tree(
            &dir,
            &[
                ("card0", "0x1002", "amdgpu", "1", Some("536870912")),
                ("card1", "0x1002", "amdgpu", "0", Some("8589934592")),
            ],
            false,
            false,
            true,
        );
        let sys = system_at(&dir);
        assert_eq!(sys.gpus[0].vram, Some(512 << 20));
        assert_eq!(
            decide(&sys, true, &|_| None).set,
            vec![(MARK, "1".to_string()), ("DRI_PRIME", "1".to_string())]
        );
        // a desktop: the big card draws the screen, a small one beside it -
        // nothing (DRI_PRIME would choose the small one)
        let mut desktop = sys.clone();
        desktop.gpus[0].boot_vga = false;
        desktop.gpus[1].boot_vga = true;
        let d = decide(&desktop, true, &|_| None);
        assert!(d.set.is_empty() && d.note.unwrap().contains("draws the screen already"));
        // two of the same memory not told: nothing asked
        let mut same = sys;
        same.gpus[0].vram = same.gpus[1].vram;
        assert!(decide(&same, true, &|_| None).set.is_empty());
    }

    #[test]
    fn one_gpu_or_a_virtual_machine_asks_for_nothing() {
        let none = |_: &str| None;
        for (name, cards) in [
            ("single", vec![("card0", "0x8086", "i915", "1", None)]),
            ("vm", vec![("card0", "0x1af4", "virtio-pci", "1", None)]),
            (
                "vm-two",
                vec![
                    ("card0", "0x15ad", "vmwgfx", "1", None),
                    ("card1", "0x1234", "bochs-drm", "0", None),
                ],
            ),
            ("none", vec![]),
        ] {
            let dir = crate::testdir::Scratch::new(&format!("offload-{name}"));
            tree(&dir, &cards, false, false, false);
            assert_eq!(
                decide(&system_at(&dir), true, &none),
                Decision::default(),
                "{name}"
            );
        }
        // a desktop with NVIDIA drawing the screen, the Intel GPU on
        let sys = System {
            gpus: vec![gpu(0x8086, "i915", false), gpu(0x10de, "nvidia", true)],
            nvidia_driver: true,
            ..System::default()
        };
        assert!(decide(&sys, true, &none).set.is_empty());
        // an AMD APU beside an NVIDIA card (a laptop): offloaded
        let sys = System {
            gpus: vec![gpu(0x1002, "amdgpu", true), gpu(0x10de, "nouveau", false)],
            ..System::default()
        };
        assert!(!decide(&sys, true, &none).set.is_empty());
    }

    #[test]
    fn the_copy_in_the_background_keeps_its_way_back() {
        let sys = System {
            gpus: vec![gpu(0x8086, "i915", true), gpu(0x10de, "nvidia", false)],
            nvidia_driver: true,
            nvidia_egl: Some(PathBuf::from(
                "/usr/share/glvnd/egl_vendor.d/10_nvidia.json",
            )),
            vulkan_loader: false,
        };
        // the program started from a terminal sets the variables ...
        let parent = decide(&sys, true, &|_| None);
        assert!(!parent.set.is_empty());
        let env: std::collections::HashMap<&str, String> = parent.set.iter().cloned().collect();
        // ... the copy that goes to the background inherits them, and knows
        // them as the program's: it sets them as its own again (so it, too,
        // starts again on the integrated GPU when it gets no window)
        let child = decide(&sys, true, &|n| env.get(n).cloned());
        assert_eq!(child.set, parent.set, "{:?}", child.note);
        // a user's own variable (no mark) still stands
        let user = decide(&sys, true, &|n| {
            (n == "__NV_PRIME_RENDER_OFFLOAD").then(|| "1".to_string())
        });
        assert!(user.set.is_empty());
    }

    #[test]
    fn a_desktop_whose_display_is_on_the_card_or_a_card_held_for_a_vm_asks_nothing() {
        let none = |_: &str| None;
        // the BIOS made the Intel GPU the first, the monitor is on the card
        let mut sys = System {
            gpus: vec![
                Gpu {
                    connected: Some(false),
                    ..gpu(0x8086, "i915", true)
                },
                Gpu {
                    connected: Some(true),
                    ..gpu(0x1002, "amdgpu", false)
                },
            ],
            ..System::default()
        };
        let d = decide(&sys, true, &none);
        assert!(d.set.is_empty() && d.note.unwrap().contains("display is on the AMD"));
        // (both with a display - a laptop's panel and a monitor on the
        // card's port: offloaded as before)
        sys.gpus[0].connected = Some(true);
        assert!(!decide(&sys, true, &none).set.is_empty());
        // a card handed to a virtual machine
        sys.gpus[1].driver = "vfio-pci".into();
        let d = decide(&sys, true, &none);
        assert!(
            d.set.is_empty() && d.note.as_deref().unwrap_or("").contains("vfio-pci"),
            "{d:?}"
        );
        // (read from /sys: a connector's status)
        let dir = crate::testdir::Scratch::new("offload-connected");
        tree(
            &dir,
            &[
                ("card0", "0x8086", "i915", "1", None),
                ("card1", "0x1002", "amdgpu", "0", None),
            ],
            false,
            false,
            true,
        );
        std::fs::write(
            dir.join("sys/class/drm/card1-HDMI-A-1/status"),
            "connected\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("sys/class/drm/card0-HDMI-A-1/status"),
            "disconnected\n",
        )
        .unwrap();
        let sys = system_at(&dir);
        assert_eq!(
            (sys.gpus[0].connected, sys.gpus[1].connected),
            (Some(false), Some(true))
        );
        assert!(decide(&sys, true, &none).set.is_empty());
    }

    #[test]
    fn helpers_get_none_of_the_programs_variables() {
        let mut c = std::process::Command::new("/bin/true");
        helper(&mut c);
        let removed: Vec<String> = c
            .get_envs()
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();
        assert!(removed.contains(&NO_OFFLOAD.to_string()), "{removed:?}");
        // (the variables themselves only where the program set them: its
        // mark - the user's own stand)
        if std::env::var_os(MARK).is_none() {
            assert!(!removed.contains(&"DRI_PRIME".to_string()));
        }
    }

    #[test]
    fn started_again_it_has_none_of_the_variables() {
        let command = again_without(Path::new("/bin/true"), &["-F".into(), "a.mp3".into()]);
        let envs: Vec<(String, Option<String>)> = command
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect();
        assert!(envs.contains(&(NO_OFFLOAD.to_string(), Some("1".to_string()))));
        for name in VARIABLES.iter().chain([&MARK]) {
            assert!(
                envs.contains(&(name.to_string(), None)),
                "{name} taken away"
            );
        }
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, ["-F", "a.mp3"]);
    }
}

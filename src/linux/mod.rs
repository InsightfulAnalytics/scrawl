//! Linux platform helpers: the session (X11 or Wayland), displays, top-level windows,
//! processes, the clipboard, the shell, autostart, the tray and the media pipeline.
//!
//! Scrawl's own windows are GPUI windows on X11, through XWayland on a Wayland session
//! (`prepare_process`), because GPUI 0.2.2's Wayland windows can't be placed, kept on top or
//! made click-through. Global coordinates are X11 root-window pixels, top-left origin, like
//! Windows' physical pixels; `TargetFrame` holds pixels too.
//!
//! What Scrawl can see depends on the session: on X11 it reads the window list, the pointer and
//! the keys straight from the X server (`x11`). On Wayland the compositor keeps those to itself:
//! the screen comes from the ScreenCast portal (or GNOME's own screencast API), the shortcut
//! from the GlobalShortcuts portal, and clicks and ⇧ from the input devices when the user may
//! read them (`input`). See `docs/linux.md`.

pub mod atspi;
pub mod clipboard;
pub mod hyprland;
pub mod input;
pub mod ipc;
pub mod login;
pub mod media;
pub mod overlay;
pub mod portal;
pub mod privacy;
pub mod shell;
pub mod shortcuts;
pub mod x11;

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub use x11::{Monitor, WindowInfo, monitor_at, monitors, primary_monitor};

/// Who answers Scrawl's questions about the desktop, settled once at launch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Session {
    /// An X11 session: the X server knows every window, the pointer and the keys.
    X11,
    /// Hyprland: its socket knows every window and the pointer, binds keys and types (`hyprland`).
    Hyprland,
    /// Another Wayland desktop (GNOME, KDE, …): XWayland sees X11 apps only; portals and the
    /// accessibility bus fill in what they can.
    Wayland,
}

pub fn session() -> Session {
    static SESSION: OnceLock<Session> = OnceLock::new();
    *SESSION.get_or_init(|| {
        if !wayland_session() {
            Session::X11
        } else if hyprland::active() {
            Session::Hyprland
        } else {
            Session::Wayland
        }
    })
}

/// A value read at most once a second: the display layout and window lists change rarely, and
/// the pointer is polled many times a second.
pub(crate) fn once_a_second<T: Clone>(slot: &Mutex<Option<(Instant, T)>>, read: impl FnOnce() -> T) -> T {
    let mut slot = slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((at, value)) = &*slot
        && at.elapsed() < Duration::from_secs(1)
    {
        return value.clone();
    }
    let value = read();
    *slot = Some((Instant::now(), value.clone()));
    value
}

/// `$XDG_RUNTIME_DIR`, else `/run/user/<uid>`, else the temporary folder: where per-login
/// sockets live (Scrawl's own, Hyprland's).
pub fn runtime_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    // SAFETY: getuid has no preconditions and can't fail.
    let user = PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() }));
    if user.is_dir() { user } else { std::env::temp_dir() }
}

/// The program to start Scrawl again with: the AppImage while running from one (`$APPIMAGE`),
/// else this executable. Keybindings, autostart and a relaunch all run it.
pub fn app_executable() -> std::io::Result<PathBuf> {
    std::env::var_os("APPIMAGE")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .map_or_else(std::env::current_exe, Ok)
}

/// A window id as `WindowInfo::hwnd` carries it. X ids fit in 29 bits (the X protocol keeps
/// the top three clear), so a Hyprland window, which may be native Wayland and has no X id, is
/// given an id with bit 30 set (`HYPRLAND_TAG`) that never collides with one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowId {
    X11(u32),
    /// The low 30 bits of the window's Hyprland address.
    Hyprland(u32),
}

impl WindowId {
    const HYPRLAND_TAG: isize = 1 << 30;
    const LOW_BITS: u64 = (1 << 30) - 1;

    /// The id for a Hyprland window's address (`0x55bca1f25da0`).
    pub fn for_hyprland_address(address: &str) -> Self {
        let raw = u64::from_str_radix(address.trim_start_matches("0x"), 16).unwrap_or(0);
        Self::Hyprland((raw & Self::LOW_BITS) as u32)
    }

    pub fn raw(self) -> isize {
        match self {
            Self::X11(id) => id as isize,
            Self::Hyprland(low) => Self::HYPRLAND_TAG | low as isize,
        }
    }

    pub fn from_raw(raw: isize) -> Self {
        if raw & Self::HYPRLAND_TAG != 0 {
            Self::Hyprland((raw as u64 & Self::LOW_BITS) as u32)
        } else {
            Self::X11(raw as u32)
        }
    }
}

/// A Hyprland window as the rest of `linux` sees windows (`WindowId::Hyprland`).
fn hyprland_window(client: &hyprland::Client) -> WindowInfo {
    WindowInfo {
        hwnd: client.id().raw(),
        pid: client.pid(),
        title: client.title.clone(),
        class: client.class.clone(),
        rect: client.rect(),
        kind: x11::WindowKind::Normal,
        hidden: false,
        // Floating is Hyprland's ordinary stacking, not "always on top"; pinned windows show on
        // every workspace above the rest.
        above: client.pinned,
        skip_taskbar: false,
    }
}

/// The focused window: Hyprland's (any app), else the X server's (X11 apps only on Wayland).
pub fn foreground_window() -> Option<WindowInfo> {
    match session() {
        Session::Hyprland => hyprland::active_window().map(|c| hyprland_window(&c)),
        Session::X11 | Session::Wayland => x11::active_window(),
    }
}

/// Top-level windows, front to back: on Hyprland every window on a showing workspace, else the
/// X window manager's list.
pub fn top_level_windows() -> Vec<WindowInfo> {
    match session() {
        Session::Hyprland => hyprland::visible_clients().iter().map(hyprland_window).collect(),
        Session::X11 | Session::Wayland => x11::top_level_windows(),
    }
}

/// A window by its `WindowInfo::hwnd`, from whichever side owns that id.
pub fn window_info(id: isize) -> Option<WindowInfo> {
    match WindowId::from_raw(id) {
        WindowId::Hyprland(_) => hyprland::window_for_id(WindowId::from_raw(id))
            .map(|c| hyprland_window(&c)),
        WindowId::X11(_) => x11::window_info(id),
    }
}

/// Brings a window to the front (for a paste into the app that was there).
pub fn activate(id: isize) {
    match WindowId::from_raw(id) {
        WindowId::Hyprland(_) => {
            if let Some(client) = hyprland::window_for_id(WindowId::from_raw(id)) {
                hyprland::focus(&client.address);
            }
        }
        WindowId::X11(_) => x11::activate(id),
    }
}

/// The app in front as the accessibility bus reports it, on a Wayland desktop where that's the
/// best source (Hyprland and X11 know the focused window themselves, so it's `None` there).
pub fn accessibility_front_app() -> Option<atspi::FocusedApp> {
    (session() == Session::Wayland)
        .then(atspi::last_external_app)
        .flatten()
}

/// Ctrl+V (Ctrl+Shift+V when `shift`) into the focused app on a Wayland session, where no app
/// may type into another: Hyprland types it, other desktops through the RemoteDesktop portal.
pub fn send_paste(shift: bool) -> anyhow::Result<()> {
    match session() {
        Session::Hyprland => hyprland::send_paste(shift),
        Session::Wayland | Session::X11 => portal::send_paste(shift),
    }
}

static WAYLAND_DISPLAY: OnceLock<Option<OsString>> = OnceLock::new();

/// Called first thing in `main`, before any thread starts: remembers the Wayland display (for
/// the clipboard and the portals) and hides it from GPUI, so its windows go through XWayland.
pub fn prepare_process() {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").filter(|v| !v.is_empty());
    let _ = WAYLAND_DISPLAY.set(wayland.clone());
    if wayland.is_some() && std::env::var_os("SCRAWL_NATIVE_WAYLAND").is_none() {
        // SAFETY: called from `main` before any other thread exists.
        unsafe {
            std::env::remove_var("WAYLAND_DISPLAY");
            // GTK dialogs GPUI opens (file pickers) and xdg-open's children follow along.
            std::env::set_var("SCRAWL_WAYLAND_DISPLAY", &wayland.unwrap_or_default());
        }
    }
}

/// The Wayland display name the session had, if it is a Wayland session.
pub fn wayland_display() -> Option<OsString> {
    WAYLAND_DISPLAY
        .get_or_init(|| {
            std::env::var_os("WAYLAND_DISPLAY")
                .or_else(|| std::env::var_os("SCRAWL_WAYLAND_DISPLAY"))
                .filter(|v| !v.is_empty())
        })
        .clone()
}

/// A Wayland session: the X server Scrawl talks to is XWayland, which sees only X11 apps.
pub fn wayland_session() -> bool {
    wayland_display().is_some()
        || std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t.eq_ignore_ascii_case("wayland"))
}

/// Where the pointer is (global pixels). On Wayland the X server sees it only over X11 windows,
/// so an absolute pointing device's position comes first when one moved it last
/// (`input::absolute_pointer`).
pub fn cursor_position() -> Option<(f64, f64)> {
    match session() {
        // Hyprland knows where the pointer is over every window.
        Session::Hyprland => {
            if let Some(position) = hyprland::cursor_position() {
                return Some(position);
            }
        }
        Session::Wayland => {
            if let Some([nx, ny]) = input::absolute_pointer()
                && let Some([x, y, w, h]) = desktop_rect()
            {
                return Some((x + nx * w, y + ny * h));
            }
        }
        Session::X11 => {}
    }
    x11::cursor_position()
}

/// The bounding box of every monitor (global pixels).
fn desktop_rect() -> Option<[f64; 4]> {
    static CACHE: Mutex<Option<(Instant, Option<[f64; 4]>)>> = Mutex::new(None);
    once_a_second(&CACHE, || {
        let monitors = monitors();
        let x0 = monitors.iter().map(|m| m.rect[0]).reduce(f64::min)?;
        let y0 = monitors.iter().map(|m| m.rect[1]).reduce(f64::min)?;
        let x1 = monitors.iter().map(|m| m.rect[0] + m.rect[2]).reduce(f64::max)?;
        let y1 = monitors.iter().map(|m| m.rect[1] + m.rect[3]).reduce(f64::max)?;
        Some([x0, y0, x1 - x0, y1 - y0])
    })
}

/// The desktop, lowercased (`gnome`, `kde`, `ubuntu:gnome`, …).
pub fn desktop() -> String {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_lowercase()
}

pub fn gnome() -> bool {
    desktop().split(':').any(|d| d == "gnome")
}

pub fn kde() -> bool {
    desktop().split(':').any(|d| d == "kde")
}

/// What the session is, for logs and Settings: "GNOME on Wayland", "KDE Plasma on X11", …
pub fn session_label() -> String {
    let desktop = if gnome() {
        "GNOME".to_owned()
    } else if kde() {
        "KDE Plasma".to_owned()
    } else {
        std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "Linux".to_owned())
    };
    format!(
        "{desktop} on {}",
        if wayland_session() { "Wayland" } else { "X11" }
    )
}

/// `~/.local/share/scrawl`: logs and machine-local state.
pub fn local_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("scrawl")
}

/// A process's executable.
pub fn process_path(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/exe")).ok()
}

/// An app's name and identifier for a process: the identifier stands in for a macOS bundle
/// identifier (the executable's name, `firefox`, `gnome-terminal-server`), and the name comes
/// from its window's class when Scrawl knows one (`Firefox`), else the executable's.
pub fn app_identity(pid: u32) -> Option<(String, String)> {
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    let exe = process_path(pid)
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .or(comm)?;
    // Hyprland knows every window's class (`firefox`, or a web app's own `chrome-…-Default`);
    // XWayland only X11 apps'.
    let class = match session() {
        Session::Hyprland => hyprland::class_of_pid(pid),
        Session::X11 | Session::Wayland => None,
    }
    .or_else(|| x11::class_of_pid(pid));
    let name = class
        .map(|class| pretty_app_name(&class))
        .unwrap_or_else(|| pretty_app_name(&exe));
    Some((name, exe.to_ascii_lowercase()))
}

/// `gnome-terminal-server` → `Gnome Terminal Server`; `firefox` → `Firefox`; a class that
/// already has capitals (`Google-chrome`) keeps them.
fn pretty_app_name(raw: &str) -> String {
    let raw = raw.rsplit('.').next().unwrap_or(raw);
    raw.split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_ids_round_trip_and_never_look_like_x_ids() {
        let hypr = WindowId::for_hyprland_address("0x55bca1f25da0");
        assert!(hypr.raw() >= 1 << 30);
        assert_eq!(WindowId::from_raw(hypr.raw()), hypr);
        let x = WindowId::X11(0x1200_005f);
        assert_eq!(WindowId::from_raw(x.raw()), x);
        // The largest X id (29 bits) still reads as X.
        assert_eq!(WindowId::from_raw(0x1fff_ffff), WindowId::X11(0x1fff_ffff));
    }

    #[test]
    fn app_names_read_like_names() {
        assert_eq!(pretty_app_name("firefox"), "Firefox");
        assert_eq!(pretty_app_name("gnome-terminal-server"), "Gnome Terminal Server");
        assert_eq!(pretty_app_name("org.gnome.Nautilus"), "Nautilus");
        assert_eq!(pretty_app_name("Google-chrome"), "Google Chrome");
    }
}

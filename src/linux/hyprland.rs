//! Hyprland, through its IPC socket (`$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE`).
//!
//! On other Wayland desktops Scrawl can't see the pointer over native apps, which app is in
//! front, or bind a shortcut without a portal dialog. Hyprland answers all of that to any client
//! of its socket: the pointer in global layout pixels (`cursorpos`), the focused window and every
//! window with its rect (`activewindow`, `clients`), and Lua it runs in the compositor (`eval`),
//! which binds Scrawl's shortcuts and types the paste. Hyprland's portal has no RemoteDesktop,
//! so without this a paste could not be sent at all.
//!
//! Hyprland's layout and XWayland's root window place the outputs differently, so every point
//! and rect read here is moved into root pixels (`to_root`), which is what the rest of `linux`
//! speaks. Scaled outputs are untested: Scrawl assumes scale 1 with `force_zero_scaling`.

use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::capture::scrawl_log;

fn socket_dir() -> Option<PathBuf> {
    let signature = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").filter(|s| !s.is_empty())?;
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() })));
    let dir = runtime.join("hypr").join(signature);
    dir.join(".socket.sock").exists().then_some(dir)
}

/// A Hyprland session whose socket answers.
pub fn active() -> bool {
    static ACTIVE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ACTIVE.get_or_init(|| socket_dir().is_some())
}

/// One request on the command socket, and its whole reply.
fn request(command: &str) -> Result<String> {
    let path = socket_dir()
        .context("Not a Hyprland session")?
        .join(".socket.sock");
    let mut stream = UnixStream::connect(&path).context("Hyprland's socket")?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(command.as_bytes())?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply)?;
    Ok(reply)
}

fn json<T: for<'de> Deserialize<'de>>(command: &str) -> Result<T> {
    let reply = request(&format!("j/{command}"))?;
    serde_json::from_str(&reply).with_context(|| format!("Hyprland's {command} reply"))
}

/// Runs Lua in the compositor. Hyprland answers `ok` or the error.
pub fn eval(lua: &str) -> Result<()> {
    let reply = request(&format!("eval {lua}"))?;
    let reply = reply.trim();
    if reply.is_empty() || reply == "ok" {
        Ok(())
    } else {
        bail!("Hyprland: {reply}")
    }
}

/// A Lua string literal.
fn lua_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A string as one shell word (single quotes).
fn shell_word(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

// MARK: Pointer and windows

#[derive(Deserialize)]
struct CursorPos {
    x: f64,
    y: f64,
}

/// Where the pointer is, in global layout pixels, over any window.
pub fn cursor_position() -> Option<(f64, f64)> {
    let pos: CursorPos = json("cursorpos").ok()?;
    Some(to_root(pos.x, pos.y))
}

#[derive(Clone, Debug, Deserialize)]
pub struct Workspace {
    pub id: i64,
}

/// A window, as `clients` and `activewindow` describe it.
#[derive(Clone, Debug, Deserialize)]
pub struct Client {
    pub address: String,
    #[serde(default)]
    pub mapped: bool,
    #[serde(default)]
    pub hidden: bool,
    pub at: [f64; 2],
    pub size: [f64; 2],
    pub workspace: Workspace,
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub pid: i64,
    #[serde(default)]
    pub floating: bool,
    #[serde(default, rename = "focusHistoryID")]
    pub focus_history_id: i64,
}

impl Client {
    /// The window's rect in root pixels (`to_root`).
    pub fn rect(&self) -> [f64; 4] {
        rect_to_root([self.at[0], self.at[1], self.size[0], self.size[1]])
    }

    pub fn pid(&self) -> u32 {
        u32::try_from(self.pid).unwrap_or(0)
    }

    /// A stand-in for an X11 window id: the address's low 31 bits, with the top bit set so
    /// it never collides with a real X id (XWayland's are small). `window_for_id` reads it back.
    pub fn id(&self) -> isize {
        let raw = u64::from_str_radix(self.address.trim_start_matches("0x"), 16).unwrap_or(0);
        (0x4000_0000 | (raw as isize & 0x3fff_ffff)) as isize
    }
}

#[derive(Deserialize)]
struct Monitor {
    #[serde(rename = "activeWorkspace")]
    active_workspace: Workspace,
    #[serde(rename = "specialWorkspace")]
    special_workspace: Option<Workspace>,
}

#[derive(Clone, Deserialize)]
struct Output {
    name: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    #[serde(default)]
    scale: f64,
    /// Space bars keep: left, top, right, bottom.
    #[serde(default)]
    reserved: [f64; 4],
    #[serde(default)]
    disabled: bool,
}

impl Output {
    /// Where the output sits in Hyprland's layout (logical pixels).
    fn rect(&self) -> [f64; 4] {
        let scale = if self.scale > 0.0 { self.scale } else { 1.0 };
        [self.x, self.y, self.width / scale, self.height / scale]
    }
}

/// Hyprland's outputs, read at most once a second (`linux::monitors` runs often).
fn outputs() -> Vec<Output> {
    static CACHE: Mutex<Option<(std::time::Instant, Vec<Output>)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((at, list)) = &*cache
        && at.elapsed() < Duration::from_secs(1)
    {
        return list.clone();
    }
    let list: Vec<Output> = json::<Vec<Output>>("monitors")
        .unwrap_or_default()
        .into_iter()
        .filter(|o| !o.disabled)
        .collect();
    *cache = Some((std::time::Instant::now(), list.clone()));
    list
}

fn contains(rect: [f64; 4], x: f64, y: f64) -> bool {
    x >= rect[0] && y >= rect[1] && x < rect[0] + rect[2] && y < rect[1] + rect[3]
}

/// The work area of the XWayland output `name` at `rect` (root pixels): the output less the
/// space Hyprland's bars reserve. XWayland has no `_NET_WORKAREA` under Hyprland.
pub fn work_area(name: &str, rect: [f64; 4]) -> Option<[f64; 4]> {
    let output = outputs().into_iter().find(|o| o.name == name)?;
    let [left, top, right, bottom] = output.reserved;
    Some([
        rect[0] + left,
        rect[1] + top,
        (rect[2] - left - right).max(1.0),
        (rect[3] - top - bottom).max(1.0),
    ])
}

/// A point in Hyprland's layout as XWayland root pixels, which is what the rest of `linux`
/// speaks. The two spaces differ: XWayland lays the outputs out its own way (here the left
/// display of Hyprland's layout can be the right one in XWayland's), so the point is moved by
/// the offset between its output's two origins, matched by output name.
pub fn to_root(x: f64, y: f64) -> (f64, f64) {
    let outputs = outputs();
    let Some(output) = outputs
        .iter()
        .find(|o| contains(o.rect(), x, y))
        .or_else(|| {
            outputs.iter().min_by(|a, b| {
                let d = |o: &&Output| {
                    let [ox, oy, ow, oh] = o.rect();
                    let dx = (ox - x).max(x - (ox + ow)).max(0.0);
                    let dy = (oy - y).max(y - (oy + oh)).max(0.0);
                    dx * dx + dy * dy
                };
                d(a).total_cmp(&d(b))
            })
        })
    else {
        return (x, y);
    };
    let Some(root) = super::x11::randr_monitors()
        .into_iter()
        .find(|(name, _)| *name == output.name)
        .map(|(_, rect)| rect)
    else {
        return (x, y);
    };
    let [lx, ly, ..] = output.rect();
    (x - lx + root[0], y - ly + root[1])
}

/// A rect in Hyprland's layout as root pixels (moved with the output under its centre).
pub fn rect_to_root(rect: [f64; 4]) -> [f64; 4] {
    let (cx, cy) = to_root(rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0);
    [cx - rect[2] / 2.0, cy - rect[3] / 2.0, rect[2], rect[3]]
}

/// The focused window.
pub fn active_window() -> Option<Client> {
    let client: Client = json("activewindow").ok()?;
    (!client.address.is_empty() && client.mapped).then_some(client)
}

/// Windows on the workspaces that are showing, front to back (most recently focused first,
/// floating windows ahead of tiled ones).
pub fn visible_clients() -> Vec<Client> {
    let showing: Vec<i64> = json::<Vec<Monitor>>("monitors")
        .map(|monitors| {
            monitors
                .into_iter()
                .flat_map(|m| {
                    [Some(m.active_workspace.id), m.special_workspace.map(|w| w.id)]
                        .into_iter()
                        .flatten()
                        .filter(|&id| id != 0)
                })
                .collect()
        })
        .unwrap_or_default();
    let mut clients: Vec<Client> = json::<Vec<Client>>("clients")
        .unwrap_or_default()
        .into_iter()
        .filter(|c| c.mapped && !c.hidden && showing.contains(&c.workspace.id))
        .filter(|c| c.size[0] > 0.0 && c.size[1] > 0.0)
        .collect();
    clients.sort_by_key(|c| (!c.floating, c.focus_history_id));
    clients
}

/// The window behind a `Client::id`.
pub fn window_for_id(id: isize) -> Option<Client> {
    json::<Vec<Client>>("clients")
        .ok()?
        .into_iter()
        .find(|c| c.id() == id)
}

/// Focuses a window by its address, for a paste into the app that was in front.
pub fn focus(address: &str) {
    let lua = format!(
        "hl.dispatch(hl.dsp.focus({{ window = {addr} }}))",
        addr = lua_string(&format!("address:{address}"))
    );
    if let Err(error) = eval(&lua) {
        scrawl_log(format!("hyprland: focus {address}: {error:#}"));
    }
}

// MARK: Paste

/// Ctrl+V (Ctrl+Shift+V in a terminal) into the focused window, typed by the compositor, the
/// way Omarchy's universal paste does it. `shift` is Scrawl's own guess; Hyprland's terminal tag
/// wins when the window has one.
pub fn send_paste(shift: bool) -> Result<()> {
    let fallback = if shift { "CTRL SHIFT" } else { "CTRL" };
    let lua = format!(
        r#"local mods = {fallback}
local w = hl.get_active_window()
if w then for _, t in ipairs(w.tags or {{}}) do if t:gsub("%*$", "") == "terminal" then mods = "CTRL SHIFT" end end end
hl.dispatch(hl.dsp.send_key_state({{ mods = mods, key = "V", state = "down" }}))
hl.timer(function() hl.dispatch(hl.dsp.send_key_state({{ mods = mods, key = "V", state = "up" }})) end, {{ timeout = 50, type = "oneshot" }})"#,
        fallback = lua_string(fallback)
    );
    eval(&lua)
}

// MARK: Shortcuts

/// Hyprland's key syntax (`ALT + SHIFT + S`).
pub fn key_combo(carbon_modifiers: u32, key: &str) -> String {
    use crate::capture::{CARBON_CMD_KEY, CARBON_CONTROL_KEY, CARBON_OPTION_KEY, CARBON_SHIFT_KEY};
    let mut parts = Vec::new();
    for (bit, name) in [
        (CARBON_CMD_KEY, "SUPER"),
        (CARBON_CONTROL_KEY, "CTRL"),
        (CARBON_OPTION_KEY, "ALT"),
        (CARBON_SHIFT_KEY, "SHIFT"),
    ] {
        if carbon_modifiers & bit != 0 {
            parts.push(name.to_owned());
        }
    }
    parts.push(key.to_ascii_uppercase());
    parts.join(" + ")
}

/// The binds Scrawl holds, (name, combo), so a config reload can put them back.
static BINDS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// The command a bind runs: this executable (or the AppImage) with `shortcut <name>`.
fn shortcut_command(name: &str) -> String {
    let exe = std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok())
        .unwrap_or_else(|| PathBuf::from("scrawl"));
    format!(
        "{} shortcut {name}",
        shell_word(&exe.to_string_lossy())
    )
}

/// The Lua that swaps Scrawl's previous binds for `binds`. The combos Scrawl bound are kept in
/// a Lua global, so a second Scrawl or a rebind removes exactly those.
fn bind_lua(binds: &[(String, String)]) -> String {
    let mut lua = String::from(
        "scrawl_binds = scrawl_binds or {}\n\
         for _, keys in ipairs(scrawl_binds) do pcall(hl.unbind, keys) end\n\
         scrawl_binds = {}\n",
    );
    for (name, combo) in binds {
        let combo = lua_string(combo);
        let command = lua_string(&shortcut_command(name));
        lua.push_str(&format!(
            "hl.bind({combo}, hl.dsp.exec_cmd({command}), {{ description = {desc} }})\n\
             table.insert(scrawl_binds, {combo})\n",
            desc = lua_string(&format!("Scrawl: {name}")),
        ));
    }
    lua
}

/// Binds `binds` (name → combo; an empty combo is left unbound) in Hyprland, each running
/// `scrawl shortcut <name>`, which `linux::ipc` hands to the app. Binds made this way live until
/// Hyprland reloads its config; `watch_reloads` puts them back.
pub fn bind(binds: &[(&str, String)]) -> Result<()> {
    let binds: Vec<(String, String)> = binds
        .iter()
        .filter(|(_, combo)| !combo.is_empty())
        .map(|(name, combo)| ((*name).to_owned(), combo.clone()))
        .collect();
    eval(&bind_lua(&binds))?;
    scrawl_log(format!("hyprland: bound {binds:?}"));
    *BINDS.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = binds;
    watch_reloads();
    Ok(())
}

/// Hyprland drops binds made over the socket when it reloads its config (Omarchy reloads it on
/// every theme or setting change), so its event socket is watched and they're bound again.
fn watch_reloads() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let _ = std::thread::Builder::new()
            .name("scrawl-hyprland-events".into())
            .spawn(|| {
                loop {
                    if let Err(error) = follow_events() {
                        scrawl_log(format!("hyprland: event socket: {error:#}"));
                    }
                    std::thread::sleep(Duration::from_secs(2));
                }
            });
    });
}

fn follow_events() -> Result<()> {
    use std::io::BufRead as _;
    let path = socket_dir()
        .context("Not a Hyprland session")?
        .join(".socket2.sock");
    let stream = UnixStream::connect(path)?;
    for line in std::io::BufReader::new(stream).lines() {
        let line = line?;
        if line.starts_with("configreloaded>>") {
            let binds = BINDS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            if !binds.is_empty()
                && let Err(error) = eval(&bind_lua(&binds))
            {
                scrawl_log(format!("hyprland: rebinding after a reload: {error:#}"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{CARBON_CMD_KEY, CARBON_OPTION_KEY, CARBON_SHIFT_KEY};

    #[test]
    fn shortcuts_are_written_the_way_hyprland_reads_them() {
        assert_eq!(
            key_combo(CARBON_OPTION_KEY | CARBON_SHIFT_KEY, "s"),
            "ALT + SHIFT + S"
        );
        assert_eq!(key_combo(CARBON_CMD_KEY, "F5"), "SUPER + F5");
    }

    #[test]
    fn lua_and_shell_quoting_survive_awkward_paths() {
        assert_eq!(lua_string(r#"a "b" \c"#), r#""a \"b\" \\c""#);
        assert_eq!(shell_word("/it's/scrawl"), r"'/it'\''s/scrawl'");
    }

    #[test]
    fn rebinding_removes_what_scrawl_bound_before() {
        let lua = bind_lua(&[("record".into(), "ALT + SHIFT + S".into())]);
        assert!(lua.contains("pcall(hl.unbind, keys)"));
        assert!(lua.contains(r#"hl.bind("ALT + SHIFT + S""#));
        assert!(lua.contains("shortcut record"));
    }

    #[test]
    fn window_ids_stay_clear_of_x11_ids() {
        let client: Client = serde_json::from_str(
            r#"{"address":"0x55bca1f25da0","mapped":true,"at":[1,2],"size":[3,4],
                "workspace":{"id":1},"class":"kitty","title":"t","pid":7,"tags":["terminal*"]}"#,
        )
        .unwrap();
        assert!(client.id() >= 0x4000_0000);
        assert_eq!((client.at, client.size), ([1.0, 2.0], [3.0, 4.0]));
    }
}

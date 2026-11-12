//! Runs CLIProxyAPI as a child process tied to the app's lifetime: started
//! when the app launches (if enabled in Settings → Auto Label), stopped when
//! it quits. On Windows the child is put in a kill-on-close Job Object, so it
//! also dies if the app crashes or is killed from Task Manager.
//!
//! By default the proxy runs from a config file the app writes itself
//! (`<app home>/cliproxy/config.yaml`, loopback only, one client key taken
//! from the Auto Label settings). A custom config path can be set instead.

use std::fs::File;
use std::io::Write as _;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::settings::AutoLabelSettings;
use crate::state::paths;

#[cfg(windows)]
const EXE_NAME: &str = "cli-proxy-api.exe";
#[cfg(not(windows))]
const EXE_NAME: &str = "cli-proxy-api";

/// How long `start` waits for the port to open.
const READY_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyState {
    /// Not started by the app.
    Off,
    Starting,
    /// Started by the app and accepting connections.
    Running { pid: u32 },
    /// Something else is already listening on the port; left alone.
    External,
    Failed(String),
}

/// Everything that decides how the proxy is launched; a change restarts it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LaunchParams {
    exe: PathBuf,
    config: PathBuf,
    managed_config: bool,
    host: String,
    port: u16,
    key: String,
}

struct Manager {
    child: Option<Child>,
    params: Option<LaunchParams>,
    state: ProxyState,
    login: Option<Child>,
}

static MANAGER: Mutex<Manager> =
    Mutex::new(Manager { child: None, params: None, state: ProxyState::Off, login: None });

/// Serializes the decide-stop-spawn part of [`start`]. The `MANAGER` lock
/// can't be held across it (spawning and `port_open` block), and without
/// this two concurrent `start` calls (e.g. Settings "Save" plus an Auto Label
/// action) could both pass the "already running?" check and both spawn —
/// orphaning the first child and reporting Failed while it served.
static START: Mutex<()> = Mutex::new(());

fn manager() -> std::sync::MutexGuard<'static, Manager> {
    MANAGER.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn state() -> ProxyState {
    let mut m = manager();
    // Notice a proxy that exited on its own.
    if let Some(child) = m.child.as_mut() {
        if let Ok(Some(status)) = child.try_wait() {
            m.child = None;
            m.params = None;
            m.state = ProxyState::Failed(format!("CLIProxyAPI exited ({status}). {}", log_tail(&log_path())));
        }
    }
    m.state.clone()
}

/// Folder holding the managed config and logs.
pub fn proxy_dir() -> PathBuf {
    paths::app_home().join("cliproxy")
}

pub fn managed_config_path() -> PathBuf {
    proxy_dir().join("config.yaml")
}

pub fn log_path() -> PathBuf {
    proxy_dir().join("cli-proxy-api.log")
}

/// CLIProxyAPI's credential folder (the `oauth.auth-dir` the managed config uses).
pub fn auth_dir() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(home).join(".cli-proxy-api"))
}

/// Codex credential files in the auth dir (`codex-<account>.json`), sorted.
pub fn codex_accounts() -> Vec<String> {
    let Some(dir) = auth_dir() else {
        return Vec::new();
    };
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let stem = name.strip_suffix(".json")?;
            let account = stem.strip_prefix("codex-")?;
            Some(account.to_string())
        })
        .collect();
    out.sort();
    out
}

/// The configured executable, else `cli-proxy-api(.exe)` in `<app home>/cliproxy/`,
/// the app home, or on PATH.
pub fn find_exe(configured: &str) -> Option<PathBuf> {
    let configured = configured.trim().trim_matches('"');
    if !configured.is_empty() {
        let p = PathBuf::from(configured);
        return p.is_file().then_some(p);
    }
    let home = paths::app_home();
    for candidate in [proxy_dir().join(EXE_NAME), home.join(EXE_NAME)] {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(EXE_NAME)).find(|p| p.is_file())
}

/// Host and port of an `http://host:port/...` URL (port defaults to 80/443).
pub fn host_port(base_url: &str) -> Option<(String, u16)> {
    let url = base_url.trim();
    let (default_port, rest) = if let Some(r) = url.strip_prefix("http://") {
        (80, r)
    } else if let Some(r) = url.strip_prefix("https://") {
        (443, r)
    } else {
        return None;
    };
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.is_empty() {
        return None;
    }
    if let Some(stripped) = authority.strip_prefix('[') {
        // [::1]:8317
        let (host, after) = stripped.split_once(']')?;
        let port = after.strip_prefix(':').map(|p| p.parse().ok()).unwrap_or(Some(default_port))?;
        return Some((host.to_string(), port));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => Some((host.to_string(), port.parse().ok()?)),
        None => Some((authority.to_string(), default_port)),
    }
}

fn is_loopback(host: &str) -> bool {
    matches!(host.to_ascii_lowercase().as_str(), "127.0.0.1" | "localhost" | "::1")
}

fn port_open(host: &str, port: u16) -> bool {
    let addrs: Vec<SocketAddr> = match (host, port).to_socket_addrs() {
        Ok(a) => a.collect(),
        Err(_) => return false,
    };
    addrs.iter().any(|a| TcpStream::connect_timeout(a, Duration::from_millis(300)).is_ok())
}

/// A fresh random client key for the managed config.
pub fn generate_key() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    h.write_u32(std::process::id());
    let a = h.finish();
    h.write_u64(a);
    format!("annot-{:016x}{:016x}", a, h.finish())
}

fn yaml_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The config the app writes for a managed proxy.
pub fn managed_config_text(host: &str, port: u16, key: &str) -> String {
    format!(
        "# Managed by Annotation Tool: rewritten every time the app starts the proxy.\n\
         # To use your own file instead, set \"Proxy config\" in Settings -> Auto Label.\n\
         config-version: 8\n\
         server:\n  host: {host}\n  port: {port}\n\
         access:\n  api-keys:\n    - {key}\n\
         oauth:\n  auth-dir: \"~/.cli-proxy-api\"\n",
        host = yaml_str(host),
        key = yaml_str(key),
    )
}

/// What `start` should launch, or why it can't.
fn launch_params(s: &AutoLabelSettings) -> Result<LaunchParams, String> {
    let exe = find_exe(&s.proxy_exe).ok_or_else(|| {
        if s.proxy_exe.trim().is_empty() {
            format!(
                "{EXE_NAME} not found. Put it in {} or set its path in Settings → Auto Label.",
                proxy_dir().display()
            )
        } else {
            format!("{} does not exist.", s.proxy_exe.trim())
        }
    })?;
    let (host, port) =
        host_port(&s.base_url).ok_or_else(|| format!("Can't read host/port from endpoint URL {}", s.base_url))?;
    let custom = s.proxy_config.trim().trim_matches('"');
    let (config, managed_config) = if custom.is_empty() {
        if !is_loopback(&host) {
            return Err(format!(
                "The endpoint URL points to {host}, not this computer. Use http://127.0.0.1:{port}/v1 to let the app run the proxy."
            ));
        }
        if s.api_key.trim().is_empty() {
            return Err("Set an API key (any value; the app writes it into the proxy config).".into());
        }
        (managed_config_path(), true)
    } else {
        let p = PathBuf::from(custom);
        if !p.is_file() {
            return Err(format!("Proxy config {} does not exist.", p.display()));
        }
        (p, false)
    };
    Ok(LaunchParams { exe, config, managed_config, host, port, key: s.api_key.trim().to_string() })
}

/// Starts the proxy (or restarts it when its settings changed) and waits until
/// it accepts connections. Blocking — call from a background thread.
pub fn start(s: &AutoLabelSettings) -> ProxyState {
    // Held until the child is registered as `Starting`; a concurrent caller
    // with the same settings then sees `Starting` and returns.
    let start_guard = START.lock().unwrap_or_else(|e| e.into_inner());
    let params = match launch_params(s) {
        Ok(p) => p,
        Err(e) => {
            stop();
            let state = ProxyState::Failed(e);
            manager().state = state.clone();
            return state;
        }
    };

    {
        let m = manager();
        if m.params.as_ref() == Some(&params) && matches!(m.state, ProxyState::Running { .. } | ProxyState::Starting) {
            drop(m);
            return state();
        }
    }
    stop();

    if port_open(&params.host, params.port) {
        manager().state = ProxyState::External;
        return ProxyState::External;
    }

    let spawned = (|| -> Result<Child, String> {
        std::fs::create_dir_all(proxy_dir()).map_err(|e| format!("Can't create {}: {e}", proxy_dir().display()))?;
        if params.managed_config {
            std::fs::write(&params.config, managed_config_text(&params.host, params.port, &params.key))
                .map_err(|e| format!("Can't write {}: {e}", params.config.display()))?;
        }
        let mut log = File::create(log_path()).map_err(|e| format!("Can't create {}: {e}", log_path().display()))?;
        let _ = writeln!(log, "# {} -config {}", params.exe.display(), params.config.display());
        let log_err = log.try_clone().map_err(|e| e.to_string())?;
        let mut cmd = Command::new(&params.exe);
        cmd.arg("-config")
            .arg(&params.config)
            .current_dir(params.config.parent().unwrap_or(Path::new(".")))
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(log_err);
        hide_window(&mut cmd);
        let child = cmd.spawn().map_err(|e| format!("Can't start {}: {e}", params.exe.display()))?;
        tie_to_app(&child);
        Ok(child)
    })();
    let child = match spawned {
        Ok(c) => c,
        Err(e) => {
            let state = ProxyState::Failed(e);
            manager().state = state.clone();
            return state;
        }
    };
    let pid = child.id();
    {
        let mut m = manager();
        m.child = Some(child);
        m.params = Some(params.clone());
        m.state = ProxyState::Starting;
    }
    drop(start_guard);

    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        // Stopped or replaced while we waited?
        {
            let m = manager();
            if m.params.as_ref() != Some(&params) || m.child.as_ref().map(Child::id) != Some(pid) {
                drop(m);
                return state();
            }
        }
        if let ProxyState::Failed(e) = state() {
            return ProxyState::Failed(e);
        }
        if port_open(&params.host, params.port) {
            let mut m = manager();
            m.state = ProxyState::Running { pid };
            return m.state.clone();
        }
        if Instant::now() >= deadline {
            stop();
            let state = ProxyState::Failed(format!(
                "CLIProxyAPI didn't open port {} within {}s. {}",
                params.port,
                READY_TIMEOUT.as_secs(),
                log_tail(&log_path())
            ));
            manager().state = state.clone();
            return state;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Stops the proxy if the app started it (an external one is left alone).
pub fn stop() {
    let mut m = manager();
    if let Some(mut child) = m.child.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    m.params = None;
    m.state = ProxyState::Off;
}

/// Stops the proxy and any login window; called when the app quits.
pub fn shutdown() {
    stop();
    if let Some(mut child) = manager().login.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// Opens `cli-proxy-api -codex-login` in its own console window. The browser
/// opens for the ChatGPT sign-in; the window closes when it's done.
pub fn start_codex_login(s: &AutoLabelSettings) -> Result<(), String> {
    if login_running() {
        return Err("A sign-in window is already open.".into());
    }
    let exe = find_exe(&s.proxy_exe).ok_or_else(|| format!("{EXE_NAME} not found. Set its path first."))?;
    let custom = s.proxy_config.trim().trim_matches('"');
    let config = if custom.is_empty() {
        let (host, port) = host_port(&s.base_url).unwrap_or(("127.0.0.1".into(), 8317));
        let key = if s.api_key.trim().is_empty() { generate_key() } else { s.api_key.trim().to_string() };
        let path = managed_config_path();
        std::fs::create_dir_all(proxy_dir()).map_err(|e| e.to_string())?;
        if !path.is_file() {
            std::fs::write(&path, managed_config_text(&host, port, &key)).map_err(|e| e.to_string())?;
        }
        path
    } else {
        PathBuf::from(custom)
    };
    let mut cmd = Command::new(&exe);
    cmd.arg("-config").arg(&config).arg("-codex-login").current_dir(config.parent().unwrap_or(Path::new(".")));
    new_console(&mut cmd);
    let child = cmd.spawn().map_err(|e| format!("Can't start {}: {e}", exe.display()))?;
    tie_to_app(&child);
    manager().login = Some(child);
    Ok(())
}

/// Whether the sign-in window is still open.
pub fn login_running() -> bool {
    let mut m = manager();
    match m.login.as_mut().map(|c| c.try_wait()) {
        Some(Ok(None)) => true,
        Some(_) => {
            m.login = None;
            false
        }
        None => false,
    }
}

/// Last few non-empty lines of the proxy log, for error messages.
pub fn log_tail(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("# ")).collect();
    let tail = lines[lines.len().saturating_sub(3)..].join(" | ");
    if tail.is_empty() { String::new() } else { format!("Log: {}", super::client::truncate(&tail, 400)) }
}

#[cfg(windows)]
fn hide_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_window(_cmd: &mut Command) {}

#[cfg(windows)]
fn new_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    cmd.creation_flags(CREATE_NEW_CONSOLE);
}

#[cfg(not(windows))]
fn new_console(_cmd: &mut Command) {}

/// Puts the child in a Job Object that kills it when the app process ends,
/// however it ends. The job handle is never closed, so it lives exactly as
/// long as this process.
#[cfg(windows)]
fn tie_to_app(child: &Child) {
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    static JOB: OnceLock<usize> = OnceLock::new();
    let job = *JOB.get_or_init(|| unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return 0;
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        job as usize
    });
    if job != 0 {
        unsafe {
            AssignProcessToJobObject(job as _, child.as_raw_handle() as _);
        }
    }
}

#[cfg(not(windows))]
fn tie_to_app(_child: &Child) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_host_port() {
        assert_eq!(host_port("http://127.0.0.1:8317/v1"), Some(("127.0.0.1".into(), 8317)));
        assert_eq!(host_port("http://localhost/v1"), Some(("localhost".into(), 80)));
        assert_eq!(host_port("https://api.example.com/v1/"), Some(("api.example.com".into(), 443)));
        assert_eq!(host_port("http://[::1]:9000/v1"), Some(("::1".into(), 9000)));
        assert_eq!(host_port("127.0.0.1:8317"), None);
        assert_eq!(host_port("http://host:notaport/v1"), None);
    }

    #[test]
    fn managed_config_quotes_values() {
        let text = managed_config_text("127.0.0.1", 8317, "k\"ey");
        assert!(text.contains("host: \"127.0.0.1\""));
        assert!(text.contains("port: 8317"));
        assert!(text.contains("- \"k\\\"ey\""));
        assert!(text.contains("auth-dir: \"~/.cli-proxy-api\""));
    }

    #[test]
    fn keys_are_unique() {
        let a = generate_key();
        let b = generate_key();
        assert!(a.starts_with("annot-") && a.len() == 38);
        assert_ne!(a, b);
    }

    #[test]
    fn launch_needs_loopback_and_key() {
        let dir = std::env::temp_dir().join(format!("al-proxy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join(EXE_NAME);
        std::fs::write(&exe, b"").unwrap();
        let mut s = AutoLabelSettings { proxy_exe: exe.display().to_string(), ..Default::default() };
        assert!(launch_params(&s).unwrap_err().contains("API key"));
        s.api_key = "k".into();
        let p = launch_params(&s).unwrap();
        assert!(p.managed_config && p.port == 8317 && p.host == "127.0.0.1");
        s.base_url = "https://api.example.com/v1".into();
        assert!(launch_params(&s).unwrap_err().contains("not this computer"));
        s.proxy_config = dir.join("missing.yaml").display().to_string();
        assert!(launch_params(&s).unwrap_err().contains("does not exist"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

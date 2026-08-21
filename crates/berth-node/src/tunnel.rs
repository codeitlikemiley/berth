use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use tokio::sync::mpsc;

use crate::error::{Error, Result};

const URL_TIMEOUT: Duration = Duration::from_secs(30);
const TERM_WAIT: Duration = Duration::from_secs(2);
const EARLY_EXIT_WAIT: Duration = Duration::from_millis(500);

const MISSING_CLOUDFLARED: &str = "\
cloudflared is not installed; required for --tunnel cloudflare
  macOS: brew install cloudflared
  Linux: echo 'deb [signed-by=/usr/share/keyrings/cloudflare-main.gpg] https://pkg.cloudflare.com/cloudflared any main' | sudo tee /etc/apt/sources.list.d/cloudflared.list && sudo apt-get update && sudo apt-get install cloudflared";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TunnelKind {
    Cloudflare,
}

pub struct TunnelChild {
    child: Option<Child>,
    pid: u32,
}

impl TunnelChild {
    fn new(child: Child) -> Result<Self> {
        let pid = child
            .id()
            .ok_or_else(|| Error::Tunnel("cloudflared spawned without a pid".into()))?;
        if pid == 0 {
            return Err(Error::Tunnel("cloudflared pid is 0".into()));
        }
        Ok(Self {
            child: Some(child),
            pid,
        })
    }

    fn mark_reaped(&mut self) {
        self.pid = 0;
    }

    pub(crate) async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        let child = self
            .child
            .as_mut()
            .ok_or_else(|| std::io::Error::other("cloudflared child is gone"))?;
        let status = child.wait().await?;
        self.mark_reaped();
        Ok(status)
    }

    pub async fn shutdown(mut self) {
        if self.pid > 1 {
            terminate_group(self.pid);
        }
        if let Some(mut child) = self.child.take() {
            match tokio::time::timeout(TERM_WAIT, child.wait()).await {
                Ok(_) => {}
                Err(_) => {
                    if self.pid > 1 {
                        kill_group(self.pid);
                    }
                    let _ = tokio::time::timeout(TERM_WAIT, child.wait()).await;
                }
            }
        }
        self.mark_reaped();
    }
}

impl Drop for TunnelChild {
    fn drop(&mut self) {
        if self.pid > 1 {
            terminate_group(self.pid);
            kill_group(self.pid);
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
        self.pid = 0;
    }
}

pub(crate) fn resolve_cloudflared() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("BERTH_CLOUDFLARED") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(missing_cloudflared_error());
    }
    find_on_path("cloudflared").ok_or_else(missing_cloudflared_error)
}

pub(crate) fn missing_cloudflared_error() -> Error {
    Error::Tunnel(MISSING_CLOUDFLARED.into())
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        let candidate = dir.join(name);
        candidate.is_file().then_some(candidate)
    })
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub async fn start_cloudflare(local: SocketAddr) -> Result<(TunnelChild, Option<String>)> {
    let bin = resolve_cloudflared()?;
    let token = env_nonempty("TUNNEL_TOKEN");
    let advertised = env_nonempty("BERTH_PUBLIC_URL").or_else(|| env_nonempty("TUNNEL_HOSTNAME"));
    run_cloudflared(&bin, local, token.as_deref(), advertised.as_deref()).await
}

pub(crate) fn cloudflare_args(local: SocketAddr, named: bool) -> Vec<String> {
    let mut args = vec!["tunnel".into(), "--no-autoupdate".into()];
    if named {
        args.push("run".into());
    } else {
        args.push("--url".into());
        args.push(local_http_url(local));
    }
    args
}

fn local_http_url(bind: SocketAddr) -> String {
    let ip = if bind.ip().is_unspecified() {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    } else {
        bind.ip()
    };
    format!("http://{}", SocketAddr::new(ip, bind.port()))
}

pub(crate) fn normalize_public_origin(raw: &str) -> Result<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(Error::Tunnel("public origin is empty".into()));
    }
    if raw.contains('\n') || raw.contains('\r') {
        return Err(Error::Tunnel("public origin is invalid".into()));
    }
    if raw.contains('?') || raw.contains('#') {
        return Err(Error::Tunnel(
            "public origin must not include a query string".into(),
        ));
    }
    let rest = if let Some(rest) = raw.strip_prefix("https://") {
        rest
    } else if raw.to_ascii_lowercase().starts_with("http://") || raw.contains("://") {
        return Err(Error::Tunnel("public origin must be https".into()));
    } else {
        raw
    };
    let hostport = rest.split('/').next().unwrap_or("").trim();
    if hostport.is_empty() {
        return Err(Error::Tunnel("public origin is missing a host".into()));
    }
    Ok(format!("https://{hostport}"))
}

/// Quick-tunnel hostname from cloudflared logs. Never keeps a query string.
pub fn parse_trycloudflare_url(text: &str) -> Option<String> {
    const NEEDLE: &str = ".trycloudflare.com";
    let cleaned = strip_ansi(text);
    let mut search_from = 0;
    while let Some(idx) = cleaned[search_from..].find(NEEDLE) {
        let abs = search_from + idx;
        let prefix = &cleaned[..abs];
        if let Some(https) = prefix.rfind("https://") {
            let host = &cleaned[https + 8..abs];
            if host_ok(host) {
                return Some(format!("https://{host}{NEEDLE}"));
            }
        }
        search_from = abs + NEEDLE.len();
    }
    None
}

fn host_ok(host: &str) -> bool {
    !host.is_empty()
        && !host.contains('/')
        && !host.contains('?')
        && !host.contains('#')
        && !host.contains(' ')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

fn strip_ansi(s: &str) -> String {
    if !s.contains('\u{1b}') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.next() == Some('[') {
                for x in chars.by_ref() {
                    if x.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub(crate) async fn run_cloudflared(
    bin: &Path,
    local: SocketAddr,
    tunnel_token: Option<&str>,
    advertised: Option<&str>,
) -> Result<(TunnelChild, Option<String>)> {
    #[cfg(not(unix))]
    {
        let _ = (bin, local, tunnel_token, advertised);
        return Err(Error::Tunnel(
            "--tunnel cloudflare is not supported on this platform".into(),
        ));
    }

    #[cfg(unix)]
    {
        run_cloudflared_unix(bin, local, tunnel_token, advertised).await
    }
}

#[cfg(unix)]
async fn run_cloudflared_unix(
    bin: &Path,
    local: SocketAddr,
    tunnel_token: Option<&str>,
    advertised: Option<&str>,
) -> Result<(TunnelChild, Option<String>)> {
    let advertised = match advertised {
        Some(raw) => Some(normalize_public_origin(raw)?),
        None => None,
    };
    let named = tunnel_token.is_some();
    let args = cloudflare_args(local, named);
    debug_assert!(
        args.iter()
            .all(|a| !a.contains('?') && !a.to_ascii_lowercase().contains("token")),
        "cloudflared argv must not include a token or query string"
    );

    let mut cmd = tokio::process::Command::new(bin);
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .process_group(0);
    match tunnel_token {
        Some(token) => {
            cmd.env("TUNNEL_TOKEN", token);
        }
        None => {
            cmd.env_remove("TUNNEL_TOKEN");
        }
    }

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(missing_cloudflared_error());
        }
        Err(err) => return Err(err.into()),
    };
    let secrets: Vec<String> = tunnel_token.into_iter().map(str::to_string).collect();
    let mut lines = pump_stdio(&mut child, &secrets);
    let mut child = TunnelChild::new(child)?;

    let origin = if named {
        let early_exit = tokio::select! {
            status = child.wait() => Some(status),
            _ = tokio::time::sleep(EARLY_EXIT_WAIT) => None,
        };
        if let Some(status) = early_exit {
            let status = status?;
            let logs = drain_rx(&mut lines).await;
            return Err(Error::Tunnel(exit_message(status, &logs)));
        }
        spawn_log_pump(lines);
        match advertised {
            Some(origin) => {
                eprintln!("named tunnel; pair with {origin}");
                Some(origin)
            }
            None => {
                eprintln!("named tunnel; pair with your hostname");
                None
            }
        }
    } else {
        let parsed = match wait_for_url(&mut child, &mut lines).await {
            Ok(url) => url,
            Err(err) => {
                child.shutdown().await;
                return Err(err);
            }
        };
        spawn_log_pump(lines);
        let origin = match advertised {
            Some(origin) => origin,
            None => parsed,
        };
        eprintln!("quick tunnel; pair with {origin}");
        Some(origin)
    };

    Ok((child, origin))
}

fn pump_stdio(child: &mut Child, secrets: &[String]) -> mpsc::UnboundedReceiver<String> {
    let (tx, rx) = mpsc::unbounded_channel();
    if let Some(out) = child.stdout.take() {
        spawn_lines(out, tx.clone(), secrets.to_vec());
    }
    if let Some(err) = child.stderr.take() {
        spawn_lines(err, tx, secrets.to_vec());
    }
    rx
}

fn spawn_lines<R>(reader: R, tx: mpsc::UnboundedSender<String>, secrets: Vec<String>)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = tx.send(redact(&line, &secrets));
        }
    });
}

fn spawn_log_pump(mut rx: mpsc::UnboundedReceiver<String>) {
    tokio::spawn(async move {
        while let Some(line) = rx.recv().await {
            eprintln!("{line}");
        }
    });
}

async fn wait_for_url(
    child: &mut TunnelChild,
    rx: &mut mpsc::UnboundedReceiver<String>,
) -> Result<String> {
    let deadline = tokio::time::sleep(URL_TIMEOUT);
    tokio::pin!(deadline);
    let mut pipes_open = true;
    enum Event {
        Timeout,
        Exit(std::io::Result<std::process::ExitStatus>),
        Line(Option<String>),
    }
    loop {
        let event = {
            let inner = child
                .child
                .as_mut()
                .ok_or_else(|| Error::Tunnel("cloudflared child is gone".into()))?;
            tokio::select! {
                _ = &mut deadline => Event::Timeout,
                status = inner.wait() => Event::Exit(status),
                line = rx.recv(), if pipes_open => Event::Line(line),
            }
        };
        match event {
            Event::Timeout => {
                return Err(Error::Tunnel(
                    "cloudflared did not print a trycloudflare URL".into(),
                ));
            }
            Event::Exit(status) => {
                child.mark_reaped();
                let status = status?;
                let logs = drain_rx(rx).await;
                return Err(Error::Tunnel(if logs.trim().is_empty() {
                    format!("cloudflared exited before the tunnel URL was ready: {status}")
                } else {
                    format!("cloudflared exited before the tunnel URL was ready: {status}\n{logs}")
                }));
            }
            Event::Line(None) => pipes_open = false,
            Event::Line(Some(line)) => {
                eprintln!("{line}");
                if let Some(url) = parse_trycloudflare_url(&line) {
                    return Ok(url);
                }
            }
        }
    }
}

async fn drain_rx(rx: &mut mpsc::UnboundedReceiver<String>) -> String {
    let mut lines = Vec::new();
    let deadline = tokio::time::sleep(Duration::from_millis(250));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => break,
            next = rx.recv() => {
                match next {
                    Some(line) => {
                        eprintln!("{line}");
                        lines.push(line);
                    }
                    None => break,
                }
            }
        }
    }
    lines.join("\n")
}

fn exit_message(status: impl std::fmt::Display, logs: &str) -> String {
    let logs = logs.trim();
    if logs.is_empty() {
        format!("cloudflared exited: {status}")
    } else {
        format!("cloudflared exited: {status}\n{logs}")
    }
}

fn redact(line: &str, secrets: &[String]) -> String {
    let mut out = line.to_string();
    for secret in secrets {
        if !secret.is_empty() {
            out = out.replace(secret, "***");
        }
    }
    out
}

fn terminate_group(pid: u32) {
    signal_group(pid, libc::SIGTERM);
}

fn kill_group(pid: u32) {
    signal_group(pid, libc::SIGKILL);
}

fn signal_group(pid: u32, sig: i32) {
    if pid <= 1 {
        return;
    }
    // SAFETY: pid is a cloudflared child we spawned; negative pid is POSIX killpg.
    let _ = unsafe { libc::kill(-(pid as i32), sig) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;

    static ENV: Mutex<()> = Mutex::new(());

    const SAMPLE_LOG: &str = r#"
2024-06-01T12:00:00Z INF Requesting new quick Tunnel on trycloudflare.com...
2024-06-01T12:00:00Z INF +--------------------------------------------------------------------------------------------+
2024-06-01T12:00:00Z INF |  Your quick Tunnel has been created! Visit it at (it may take some time to be reachable):  |
2024-06-01T12:00:00Z INF |  https://random-words-here.trycloudflare.com                                               |
2024-06-01T12:00:00Z INF +--------------------------------------------------------------------------------------------+
"#;

    struct EnvGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        keys: Vec<String>,
    }

    impl EnvGuard {
        fn acquire(keys: &[&str]) -> Self {
            let lock = ENV.lock().unwrap_or_else(|p| p.into_inner());
            for key in keys {
                // SAFETY: ENV serializes process-env mutation in this module.
                unsafe { std::env::remove_var(key) };
            }
            Self {
                _lock: lock,
                keys: keys.iter().map(|s| (*s).to_string()).collect(),
            }
        }

        fn set(&self, key: &str, val: &str) {
            // SAFETY: ENV serializes process-env mutation in this module.
            unsafe { std::env::set_var(key, val) };
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for key in &self.keys {
                // SAFETY: ENV serializes process-env mutation in this module.
                unsafe { std::env::remove_var(key) };
            }
        }
    }

    fn write_fake(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("cloudflared");
        fs::write(&path, body).unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).unwrap();
        path
    }

    #[test]
    fn parse_trycloudflare_from_typical_log() {
        assert_eq!(
            parse_trycloudflare_url(SAMPLE_LOG).as_deref(),
            Some("https://random-words-here.trycloudflare.com")
        );
    }

    #[test]
    fn parse_trycloudflare_plain_line() {
        assert_eq!(
            parse_trycloudflare_url("https://abc.trycloudflare.com").as_deref(),
            Some("https://abc.trycloudflare.com")
        );
    }

    #[test]
    fn parse_trycloudflare_strips_ansi_and_ignores_query() {
        let line =
            "\u{1b}[32mINF\u{1b}[0m | https://ansi-test.trycloudflare.com?token=brt_secret |";
        assert_eq!(
            parse_trycloudflare_url(line).as_deref(),
            Some("https://ansi-test.trycloudflare.com")
        );
        assert!(!parse_trycloudflare_url(line).unwrap().contains('?'));
        assert!(!parse_trycloudflare_url(line).unwrap().contains("token"));
        assert!(!parse_trycloudflare_url(line).unwrap().contains("brt_"));
    }

    #[test]
    fn parse_trycloudflare_skips_non_https() {
        assert!(parse_trycloudflare_url("http://nope.trycloudflare.com").is_none());
        assert!(parse_trycloudflare_url("no url here").is_none());
    }

    #[test]
    fn public_origin_rejects_query_string() {
        let err = normalize_public_origin("https://x.trycloudflare.com?token=brt_secret")
            .unwrap_err()
            .to_string();
        assert!(err.contains("query string"));
        assert!(!err.contains("brt_secret"));
    }

    #[test]
    fn public_origin_strips_path_and_adds_https() {
        assert_eq!(
            normalize_public_origin("my-node.example.com/v1").unwrap(),
            "https://my-node.example.com"
        );
        assert_eq!(
            normalize_public_origin("https://my-node.example.com/").unwrap(),
            "https://my-node.example.com"
        );
    }

    #[test]
    fn public_origin_requires_https() {
        let err = normalize_public_origin("http://cleartext.example.com")
            .unwrap_err()
            .to_string();
        assert!(err.contains("https"), "{err}");
        assert!(!err.contains("cleartext.example.com"), "{err}");
        let ws = normalize_public_origin("ws://x.example")
            .unwrap_err()
            .to_string();
        assert!(ws.contains("https"), "{ws}");
        assert!(!ws.contains("ws://"), "{ws}");
    }

    #[test]
    fn cloudflare_args_never_include_token_or_bind_all() {
        let named = cloudflare_args("127.0.0.1:7432".parse().unwrap(), true);
        assert_eq!(named, ["tunnel", "--no-autoupdate", "run"]);
        assert!(
            named
                .iter()
                .all(|a| !a.to_ascii_lowercase().contains("token"))
        );

        let quick = cloudflare_args("0.0.0.0:7432".parse().unwrap(), false);
        assert_eq!(
            quick,
            [
                "tunnel",
                "--no-autoupdate",
                "--url",
                "http://127.0.0.1:7432"
            ]
        );
        assert!(quick.iter().all(|a| !a.contains("0.0.0.0")));
        assert!(quick.iter().all(|a| !a.contains('?')));
    }

    #[test]
    fn missing_binary_mentions_install() {
        let env = EnvGuard::acquire(&["BERTH_CLOUDFLARED"]);
        env.set("BERTH_CLOUDFLARED", "/no/such/berth-cloudflared");
        let err = resolve_cloudflared().unwrap_err().to_string();
        assert!(err.contains("brew install cloudflared"));
        assert!(err.contains("pkg.cloudflare.com/cloudflared"));
        assert!(err.contains("--tunnel cloudflare"));
    }

    const FAKE_SH: &str = r#"#!/bin/sh
if [ -n "${BERTH_TEST_ARGV-}" ]; then
  printf '%s\n' "$0" "$@" > "$BERTH_TEST_ARGV"
fi
if [ -n "${BERTH_TEST_PID-}" ]; then
  printf '%s\n' "$$" > "$BERTH_TEST_PID"
fi
printf '%s\n' "INF | https://unit-test.trycloudflare.com |" >&2
exec sleep 60
"#;

    #[tokio::test]
    async fn fake_quick_tunnel_parses_url_and_omits_token_from_argv() {
        let env = EnvGuard::acquire(&["BERTH_TEST_ARGV", "BERTH_TEST_PID"]);
        let dir = tempfile::tempdir().unwrap();
        let argv_path = dir.path().join("argv");
        env.set("BERTH_TEST_ARGV", argv_path.to_str().unwrap());
        let bin = write_fake(dir.path(), FAKE_SH);
        let token = "brt_must_not_leak";
        let local: SocketAddr = "127.0.0.1:7432".parse().unwrap();

        let (child, origin) = run_cloudflared(&bin, local, None, None).await.unwrap();
        assert_eq!(
            origin.as_deref(),
            Some("https://unit-test.trycloudflare.com")
        );
        let argv = wait_file(&argv_path).await;
        child.shutdown().await;
        assert!(argv.contains("tunnel"));
        assert!(argv.contains("--no-autoupdate"));
        assert!(argv.contains("--url"));
        assert!(argv.contains("http://127.0.0.1:7432"));
        assert!(!argv.contains(token));
        assert!(!argv.contains("--token"));
        assert!(!argv.contains('?'));
    }

    #[tokio::test]
    async fn named_tunnel_token_stays_out_of_argv() {
        let env = EnvGuard::acquire(&["BERTH_TEST_ARGV", "BERTH_TEST_PID"]);
        let dir = tempfile::tempdir().unwrap();
        let argv_path = dir.path().join("argv");
        env.set("BERTH_TEST_ARGV", argv_path.to_str().unwrap());
        let bin = write_fake(dir.path(), FAKE_SH);
        let token = "eyJ_named_tunnel_token_secret";
        let local: SocketAddr = "127.0.0.1:7432".parse().unwrap();
        let (child, origin) =
            run_cloudflared(&bin, local, Some(token), Some("https://berth.example.com"))
                .await
                .unwrap();
        assert_eq!(origin.as_deref(), Some("https://berth.example.com"));
        let argv = wait_file(&argv_path).await;
        child.shutdown().await;
        assert!(argv.contains("run"));
        assert!(!argv.contains("--url"));
        assert!(!argv.contains(token));
        assert!(!argv.contains("--token"));
        assert!(!argv.contains('?'));
    }

    #[tokio::test]
    async fn named_early_exit_includes_stderr() {
        let _env = EnvGuard::acquire(&["BERTH_TEST_ARGV", "BERTH_TEST_PID"]);
        let dir = tempfile::tempdir().unwrap();
        let bin = write_fake(
            dir.path(),
            r#"#!/bin/sh
printf '%s\n' "ERR failed to run tunnel: invalid token" >&2
exit 7
"#,
        );
        let probe = std::process::Command::new(&bin)
            .args(["tunnel", "--no-autoupdate", "run"])
            .output()
            .unwrap();
        assert!(
            !probe.status.success(),
            "fake must exit: status={:?} stderr={:?}",
            probe.status,
            String::from_utf8_lossy(&probe.stderr)
        );
        let err = match run_cloudflared(
            &bin,
            "127.0.0.1:7432".parse().unwrap(),
            Some("named_secret"),
            Some("https://berth.example.com"),
        )
        .await
        {
            Ok((child, _)) => {
                child.shutdown().await;
                panic!("expected named tunnel to fail");
            }
            Err(err) => err.to_string(),
        };
        assert!(err.contains("exited"), "{err}");
        assert!(err.contains("invalid token"), "{err}");
        assert!(!err.contains("named_secret"), "{err}");
    }

    #[tokio::test]
    async fn child_is_killed_on_shutdown() {
        let env = EnvGuard::acquire(&["BERTH_TEST_ARGV", "BERTH_TEST_PID"]);
        let dir = tempfile::tempdir().unwrap();
        let pid_path = dir.path().join("pid");
        env.set("BERTH_TEST_PID", pid_path.to_str().unwrap());
        let bin = write_fake(dir.path(), FAKE_SH);
        let local: SocketAddr = "127.0.0.1:7432".parse().unwrap();
        let (child, _) = run_cloudflared(&bin, local, None, None).await.unwrap();
        let raw = wait_file(&pid_path).await;
        let pid: u32 = raw.trim().parse().expect("pid");
        assert!(pid_alive(pid), "fake cloudflared should be running");
        child.shutdown().await;
        let mut gone = false;
        for _ in 0..50 {
            if !pid_alive(pid) {
                gone = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(gone, "cloudflared child {pid} still alive after shutdown");
    }

    fn pid_alive(pid: u32) -> bool {
        // SAFETY: signal 0 checks existence; pid is the child we spawned.
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }

    async fn wait_file(path: &Path) -> String {
        for _ in 0..100 {
            if let Ok(raw) = fs::read_to_string(path)
                && !raw.trim().is_empty()
            {
                return raw;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("timed out waiting for {}", path.display());
    }
}

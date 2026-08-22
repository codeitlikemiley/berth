use std::time::{Duration, SystemTime, UNIX_EPOCH};

use berthos_protocol::{
    Ack, AckKind, AckResult, Action, ActionBatch, Frame, FrameKind, LeaseRequest, ObjectStore,
    Point,
};
use bollard::Docker;
use bollard::exec::StartExecResults;
use bollard::models::{ContainerCreateBody, ExecConfig, HostConfig, Mount, MountType};
use bollard::query_parameters::{CreateContainerOptionsBuilder, RemoveContainerOptionsBuilder};
use futures_util::StreamExt;
use tokio::time::sleep;

use crate::action::{ACTION_BIN, PNG_MAGIC, action_argv, key_repeats, png_dimensions, skipped};
use crate::allowlist::csv_for_lease;
#[cfg(debug_assertions)]
use crate::docker::{GuestVolumes, OBJECT_MOUNT, assert_host_isolated};

/// Where the node's rclone config is mounted inside the helper, read-only.
const RCLONE_CONFIG_MOUNT: &str = "/berth/rclone.conf";
const RCLONE_WAIT: Duration = Duration::from_secs(300);

/// The node's rclone config. Credentials live here and nowhere else.
fn rclone_config_path() -> Result<std::path::PathBuf> {
    let dir =
        match std::env::var_os("BERTH_HOME") {
            Some(home) => std::path::PathBuf::from(home),
            None => std::path::PathBuf::from(std::env::var_os("HOME").ok_or_else(|| {
                Error::Internal("HOME is not set; cannot find rclone.conf".into())
            })?)
            .join(".berth"),
        };
    Ok(dir.join("rclone.conf"))
}
use crate::docker::{
    VIEWER_PORT, container_body, container_name, image_from_env, network_name, s3_volume_name,
    session_network_create, volume_create, volume_name,
};
use crate::error::{Error, Result};
use crate::id::new_id;

const READY_TIMEOUT: Duration = Duration::from_secs(30);
const READY_POLL: Duration = Duration::from_millis(200);
const EXEC_WAIT: Duration = Duration::from_secs(10);
const NETWORK_RM_TRIES: u32 = 10;
const NETWORK_RM_DELAY: Duration = Duration::from_millis(50);

/// Talks to the local Docker engine and starts isolated guest sessions.
#[derive(Clone, Debug)]
pub struct SessionManager {
    docker: Docker,
    image: String,
}

impl SessionManager {
    pub fn connect() -> Result<Self> {
        Ok(Self {
            docker: Docker::connect_with_local_defaults()?,
            image: image_from_env(),
        })
    }

    pub fn with_docker(docker: Docker, image: impl Into<String>) -> Self {
        Self {
            docker,
            image: image.into(),
        }
    }

    pub fn image(&self) -> &str {
        &self.image
    }

    pub async fn start(&self, req: &LeaseRequest) -> Result<Session> {
        req.validate_mvp()?;
        let session_id = new_id("s");
        let workspace_id = workspace_id_from_req(req);
        let volume = volume_name(&workspace_id);
        // A bucket gets its own volume, staged before the guest starts.
        let object_volume = req.object.as_ref().map(|_| s3_volume_name(&workspace_id));
        let network = network_name(&session_id);
        let name = container_name(&session_id);
        // Build HostConfig before allocating a network so InvalidResources
        // cannot leak a berth-net-*.
        let body = container_body(
            &self.image,
            &session_id,
            &workspace_id,
            &req.resources,
            GuestVolumes {
                workspace: &volume,
                object: object_volume.as_deref(),
            },
            &network,
            &csv_for_lease(req),
        )?;
        #[cfg(debug_assertions)]
        if let Some(host) = body.host_config.as_ref() {
            assert_host_isolated(host);
        }

        self.ensure_volume(&volume).await?;
        if let (Some(object), Some(vol)) = (req.object.as_ref(), object_volume.as_deref()) {
            // Before the guest exists, so it never sees a half-populated mount.
            self.stage_in(object, vol).await?;
        }
        self.create_session_network(&network, &session_id).await?;
        // Drop (cancel or error) removes the in-flight network/container.
        let mut guard = StartGuard::new(self.docker.clone(), network.clone());

        let created = self
            .docker
            .create_container(
                Some(CreateContainerOptionsBuilder::default().name(&name).build()),
                body,
            )
            .await?;
        let container_id = created.id;
        guard.arm_container(container_id.clone());

        self.boot(&container_id).await?;

        let viewer_port = self.viewer_port(&container_id).await;
        guard.disarm();
        Ok(Session {
            object: req.object.clone(),
            object_volume,
            image: self.image.clone(),
            docker: self.docker.clone(),
            container_id,
            container_name: name,
            session_id,
            workspace_id,
            volume,
            network,
            viewer_port,
            last_frame: None,
            stopped: false,
        })
    }

    /// Force-remove container + session network. Docker 404 is success (already gone).
    pub async fn reap(&self, session_id: &str, container_id: Option<&str>) -> Result<()> {
        if let Some(id) = container_id.filter(|id| !id.is_empty()) {
            force_remove_container(&self.docker, id).await?;
        }
        let name = container_name(session_id);
        if container_id.is_none_or(|id| id != name) {
            force_remove_container(&self.docker, &name).await?;
        }
        remove_session_network(&self.docker, &network_name(session_id)).await;
        Ok(())
    }

    /// Delete a workspace volume. Docker refuses while a container still uses
    /// it, and that refusal is load-bearing: it is the last guard against
    /// pulling the disk out from under a running guest.
    pub async fn remove_volume(&self, name: &str) -> Result<()> {
        self.docker
            .remove_volume(name, None::<bollard::query_parameters::RemoveVolumeOptions>)
            .await?;
        Ok(())
    }

    pub async fn volume_exists(&self, name: &str) -> bool {
        self.docker.inspect_volume(name).await.is_ok()
    }

    /// Stage a bucket into the guest's `/mnt/s3` volume before it starts.
    ///
    /// rclone runs in a short-lived helper container, not in the guest: it is
    /// the only thing that sees the node's rclone config, so an agent with a
    /// shell in the guest still cannot read the bucket credentials. The helper
    /// is on the default bridge rather than the guest's isolated network, so
    /// staging is unaffected by the guest's egress allowlist -- the node is
    /// fetching the bytes, not the agent.
    pub async fn stage_in(&self, object: &ObjectStore, volume: &str) -> Result<()> {
        self.ensure_volume(volume).await?;
        self.run_rclone(
            &["copy", &object.rclone_path(), OBJECT_MOUNT],
            volume,
            "stage in",
            true,
        )
        .await
    }

    /// Sync `/mnt/s3` back to the bucket after the guest has stopped.
    pub async fn stage_out(&self, object: &ObjectStore, volume: &str) -> Result<()> {
        if !self.volume_exists(volume).await {
            return Ok(());
        }
        self.run_rclone(
            &["sync", OBJECT_MOUNT, &object.rclone_path()],
            volume,
            "stage out",
            false,
        )
        .await
    }

    async fn run_rclone(
        &self,
        args: &[&str],
        volume: &str,
        what: &str,
        hand_to_guest: bool,
    ) -> Result<()> {
        let config = rclone_config_path()?;
        if !config.is_file() {
            return Err(Error::Guest(format!(
                "{} needs an rclone config at {}",
                what,
                config.display()
            )));
        }
        // rclone runs as root, so anything it stages in lands root-owned and the
        // guest -- which runs as `berth` -- could not write into it. Args go
        // through "$@" rather than being interpolated, so a bucket or prefix can
        // never be read as shell syntax.
        let script = if hand_to_guest {
            format!(
                "rclone --config {RCLONE_CONFIG_MOUNT} \"$@\" && chown -R berth:berth {OBJECT_MOUNT}"
            )
        } else {
            format!("rclone --config {RCLONE_CONFIG_MOUNT} \"$@\"")
        };
        let mut cmd = vec!["sh".to_string(), "-c".to_string(), script, "sh".to_string()];
        cmd.extend(args.iter().map(|a| (*a).to_string()));

        let name = format!("berth-rclone-{}", new_id("h").trim_start_matches("h_"));
        let body = ContainerCreateBody {
            image: Some(self.image.clone()),
            entrypoint: Some(vec![String::new()]),
            cmd: Some(cmd),
            user: Some("root".into()),
            host_config: Some(HostConfig {
                // Read-only so a bug here cannot rewrite the node's credentials.
                binds: Some(vec![format!(
                    "{}:{}:ro",
                    config.display(),
                    RCLONE_CONFIG_MOUNT
                )]),
                mounts: Some(vec![Mount {
                    target: Some(OBJECT_MOUNT.into()),
                    source: Some(volume.to_string()),
                    typ: Some(MountType::VOLUME),
                    read_only: Some(false),
                    ..Default::default()
                }]),
                auto_remove: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };

        let opts = CreateContainerOptionsBuilder::default().name(&name).build();
        let created = self.docker.create_container(Some(opts), body).await?;
        let id = created.id;
        let result = self.await_rclone(&id, what).await;
        let _ = force_remove_container(&self.docker, &id).await;
        result
    }

    async fn await_rclone(&self, id: &str, what: &str) -> Result<()> {
        self.docker
            .start_container(id, None::<bollard::query_parameters::StartContainerOptions>)
            .await?;
        let deadline = tokio::time::Instant::now() + RCLONE_WAIT;
        loop {
            let info = self
                .docker
                .inspect_container(
                    id,
                    None::<bollard::query_parameters::InspectContainerOptions>,
                )
                .await?;
            let state = info.state.as_ref();
            let running = state.and_then(|s| s.running).unwrap_or(false);
            if !running {
                let code = state.and_then(|s| s.exit_code).unwrap_or(0);
                if code == 0 {
                    return Ok(());
                }
                return Err(Error::Guest(format!("rclone {what} exited {code}")));
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(Error::Guest(format!("rclone {what} timed out")));
            }
            sleep(Duration::from_millis(200)).await;
        }
    }

    async fn ensure_volume(&self, name: &str) -> Result<()> {
        if self.docker.inspect_volume(name).await.is_ok() {
            return Ok(());
        }
        match self.docker.create_volume(volume_create(name)).await {
            Ok(_) => Ok(()),
            Err(err) => {
                if self.docker.inspect_volume(name).await.is_ok() {
                    Ok(())
                } else {
                    Err(err.into())
                }
            }
        }
    }

    async fn create_session_network(&self, name: &str, session_id: &str) -> Result<()> {
        self.docker
            .create_network(session_network_create(name, session_id))
            .await?;
        Ok(())
    }

    async fn boot(&self, container_id: &str) -> Result<()> {
        self.docker.start_container(container_id, None).await?;
        self.wait_ready(container_id).await
    }

    async fn wait_ready(&self, container_id: &str) -> Result<()> {
        let deadline = tokio::time::Instant::now() + READY_TIMEOUT;
        loop {
            let last_stderr = match exec_cmd(
                &self.docker,
                container_id,
                &["/usr/local/bin/berth-health".to_string()],
            )
            .await
            {
                Ok(out) if out.exit_code == 0 => return Ok(()),
                Ok(out) => out.stderr_lossy(),
                Err(err) => err.to_string(),
            };
            if tokio::time::Instant::now() >= deadline {
                return Err(Error::ReadyTimeout { last_stderr });
            }
            sleep(READY_POLL).await;
        }
    }

    async fn viewer_port(&self, container_id: &str) -> Option<u16> {
        let inspect = self
            .docker
            .inspect_container(container_id, None)
            .await
            .ok()?;
        let ports = inspect.network_settings?.ports?;
        let bindings = ports.get(VIEWER_PORT)?.as_ref()?;
        bindings
            .iter()
            .find_map(|b| b.host_port.as_deref().and_then(|p| p.parse().ok()))
    }
}

/// Ack plus any screenshot frames produced by the batch, in item order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOutput {
    pub ack: Ack,
    pub frames: Vec<Frame>,
}

/// One running guest desktop.
///
/// Callers must [`Session::stop`] the session. Drop best-effort removes the
/// container (keeps the volume) if a Tokio runtime is still available.
#[derive(Debug)]
pub struct Session {
    docker: Docker,
    container_id: String,
    container_name: String,
    session_id: String,
    workspace_id: String,
    volume: String,
    network: String,
    viewer_port: Option<u16>,
    last_frame: Option<Frame>,
    stopped: bool,
    /// Kept so stop() knows where to sync `/mnt/s3` back to, and which image
    /// the rclone helper that does it should run.
    object: Option<ObjectStore>,
    object_volume: Option<String>,
    image: String,
}

impl Session {
    pub async fn start(req: &LeaseRequest) -> Result<Self> {
        SessionManager::connect()?.start(req).await
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn workspace_id(&self) -> &str {
        &self.workspace_id
    }

    pub fn container_id(&self) -> &str {
        &self.container_id
    }

    pub fn container_name(&self) -> &str {
        &self.container_name
    }

    pub fn volume_name(&self) -> &str {
        &self.volume
    }

    pub fn network_name(&self) -> &str {
        &self.network
    }

    pub fn viewer_port(&self) -> Option<u16> {
        self.viewer_port
    }

    pub fn last_frame(&self) -> Option<&Frame> {
        self.last_frame.as_ref()
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped
    }

    /// Run `items` in order via `/usr/local/bin/action.sh`. First failure skips the rest.
    ///
    /// Docker/exec infrastructure errors fail that item (and skip the rest)
    /// rather than dropping a partial Ack. Screenshot items append a `Frame`
    /// whose width/height come from PNG IHDR — missing IHDR fails the item.
    pub async fn exec(&mut self, batch: ActionBatch) -> Result<ExecOutput> {
        if self.stopped {
            return Err(Error::Stopped);
        }
        let n = batch.items.len();
        let mut results = Vec::with_capacity(n);
        let mut frames = Vec::new();
        let mut skip = false;
        for (i, item) in batch.items.iter().enumerate() {
            let i = i as u32;
            if skip {
                results.push(skipped(i));
                continue;
            }
            match run_item(&self.docker, &self.container_id, &self.session_id, item).await {
                Ok(frame) => {
                    let has_frame = frame.is_some();
                    if let Some(frame) = frame {
                        self.last_frame = Some(frame.clone());
                        frames.push(frame);
                    }
                    results.push(AckResult {
                        i,
                        ok: true,
                        frame: has_frame,
                        error: None,
                    });
                }
                Err(ItemError::Failed(error)) => {
                    results.push(AckResult {
                        i,
                        ok: false,
                        frame: false,
                        error: Some(error),
                    });
                    skip = true;
                }
            }
        }
        Ok(ExecOutput {
            ack: Ack {
                kind: AckKind::Ack,
                id: batch.id,
                results,
            },
            frames,
        })
    }

    pub async fn screenshot(&mut self) -> Result<Frame> {
        if self.stopped {
            return Err(Error::Stopped);
        }
        let out = exec_cmd(
            &self.docker,
            &self.container_id,
            &[ACTION_BIN.to_string(), "screenshot".into()],
        )
        .await?;
        if out.exit_code != 0 {
            return Err(Error::Guest(format!(
                "screenshot exit {}: {}",
                out.exit_code,
                out.stderr_lossy()
            )));
        }
        let cursor = parse_cursor(&out.stderr_lossy());
        let frame = frame_from_png(&self.session_id, out.stdout, cursor)?;
        self.last_frame = Some(frame.clone());
        Ok(frame)
    }

    /// Remove the container (and its private network). The workspace volume is kept.
    ///
    /// Docker 404 is success so a restarted node can still close the ledger.
    /// On other failures the session stays usable so `stop` can be retried.
    pub async fn stop(&mut self) -> Result<()> {
        if self.stopped {
            return Ok(());
        }
        force_remove_container(&self.docker, &self.container_id).await?;
        self.stopped = true;
        remove_session_network(&self.docker, &self.network).await;
        // After the container is gone, so nothing can still be writing while
        // the sync runs. A failure here must not leave the caller thinking the
        // session is still up, so it is reported after teardown, not before.
        if let (Some(object), Some(volume)) = (self.object.clone(), self.object_volume.clone()) {
            let manager = SessionManager {
                docker: self.docker.clone(),
                image: self.image.clone(),
            };
            manager.stage_out(&object, &volume).await?;
        }
        Ok(())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        let docker = self.docker.clone();
        let container_id = self.container_id.clone();
        let network = self.network.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let _ = force_remove_container(&docker, &container_id).await;
                remove_session_network(&docker, &network).await;
            });
        }
    }
}

/// Owns an in-flight create/boot so request cancellation cannot leak a container.
struct StartGuard {
    docker: Docker,
    container_id: Option<String>,
    network: String,
    disarmed: bool,
}

impl StartGuard {
    fn new(docker: Docker, network: String) -> Self {
        Self {
            docker,
            container_id: None,
            network,
            disarmed: false,
        }
    }

    fn arm_container(&mut self, id: String) {
        self.container_id = Some(id);
    }

    fn disarm(&mut self) {
        self.disarmed = true;
    }
}

impl Drop for StartGuard {
    fn drop(&mut self) {
        if self.disarmed {
            return;
        }
        let docker = self.docker.clone();
        let container_id = self.container_id.take();
        let network = self.network.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                if let Some(id) = container_id {
                    let _ = force_remove_container(&docker, &id).await;
                }
                remove_session_network(&docker, &network).await;
            });
        }
    }
}

fn docker_not_found(err: &bollard::errors::Error) -> bool {
    matches!(
        err,
        bollard::errors::Error::DockerResponseServerError {
            status_code: 404,
            ..
        }
    )
}

async fn force_remove_container(docker: &Docker, id: &str) -> Result<()> {
    match docker
        .remove_container(
            id,
            Some(
                RemoveContainerOptionsBuilder::default()
                    .force(true)
                    .v(false)
                    .build(),
            ),
        )
        .await
    {
        Ok(()) => Ok(()),
        Err(err) if docker_not_found(&err) => Ok(()),
        Err(err) => Err(err.into()),
    }
}

/// Best-effort delete. 404 is success. "active endpoints" is retried.
async fn remove_session_network(docker: &Docker, name: &str) {
    for attempt in 0..NETWORK_RM_TRIES {
        match docker.remove_network(name).await {
            Ok(()) => return,
            Err(err) if docker_not_found(&err) => return,
            Err(_) if attempt + 1 == NETWORK_RM_TRIES => return,
            Err(_) => sleep(NETWORK_RM_DELAY).await,
        }
    }
}

enum ItemError {
    Failed(String),
}

struct ExecOut {
    exit_code: i64,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl ExecOut {
    fn stderr_lossy(&self) -> String {
        String::from_utf8_lossy(&self.stderr).trim().to_string()
    }
}

async fn run_item(
    docker: &Docker,
    container_id: &str,
    session_id: &str,
    item: &Action,
) -> std::result::Result<Option<Frame>, ItemError> {
    let argv = action_argv(item).map_err(ItemError::Failed)?;
    let repeats = key_repeats(item);
    let mut last_stdout = Vec::new();
    let mut last_stderr = String::new();
    for _ in 0..repeats {
        let out = exec_cmd(docker, container_id, &argv)
            .await
            .map_err(|err| ItemError::Failed(err.to_string()))?;
        if out.exit_code != 0 {
            let msg = if out.stderr_lossy().is_empty() {
                format!("driver exited {}", out.exit_code)
            } else {
                format!("driver exited {}: {}", out.exit_code, out.stderr_lossy())
            };
            return Err(ItemError::Failed(msg));
        }
        last_stderr = out.stderr_lossy();
        last_stdout = out.stdout;
    }
    // zoom and cursor_position answer with a frame too: the protocol has no
    // other way to carry a reply back to the caller.
    if matches!(
        item,
        Action::Screenshot {} | Action::Zoom { .. } | Action::CursorPosition {}
    ) {
        let frame = frame_from_png(session_id, last_stdout, parse_cursor(&last_stderr))
            .map_err(|err| ItemError::Failed(err.to_string()))?;
        Ok(Some(frame))
    } else {
        Ok(None)
    }
}

/// Pull `berth-cursor X Y` out of the driver's stderr.
///
/// stdout has to stay a clean PNG, so the pointer rides on stderr behind a
/// marker. Anything else on stderr is left alone -- it is still the channel the
/// driver reports real errors on.
fn parse_cursor(stderr: &str) -> Option<Point> {
    for line in stderr.lines() {
        if let Some(rest) = line.trim().strip_prefix("berth-cursor ") {
            let mut parts = rest.split_whitespace();
            let x = parts.next()?.parse().ok()?;
            let y = parts.next()?.parse().ok()?;
            return Some([x, y]);
        }
    }
    None
}

async fn exec_cmd(docker: &Docker, container_id: &str, cmd: &[String]) -> Result<ExecOut> {
    let created = docker
        .create_exec(
            container_id,
            ExecConfig {
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                cmd: Some(cmd.to_vec()),
                privileged: Some(false),
                user: Some("berth".into()),
                working_dir: Some("/workspace".into()),
                env: Some(vec!["DISPLAY=:99".into()]),
                ..Default::default()
            },
        )
        .await?;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    match docker.start_exec(&created.id, None).await? {
        StartExecResults::Attached { mut output, .. } => {
            while let Some(msg) = output.next().await {
                match msg? {
                    bollard::container::LogOutput::StdOut { message }
                    | bollard::container::LogOutput::Console { message } => {
                        stdout.extend_from_slice(&message);
                    }
                    bollard::container::LogOutput::StdErr { message } => {
                        stderr.extend_from_slice(&message);
                    }
                    bollard::container::LogOutput::StdIn { .. } => {}
                }
            }
        }
        StartExecResults::Detached => {}
    }
    let exit_code = wait_exec_exit(docker, &created.id).await?;
    Ok(ExecOut {
        exit_code,
        stdout,
        stderr,
    })
}

async fn wait_exec_exit(docker: &Docker, exec_id: &str) -> Result<i64> {
    let deadline = tokio::time::Instant::now() + EXEC_WAIT;
    loop {
        let inspect = docker.inspect_exec(exec_id).await?;
        let running = inspect.running.unwrap_or(false);
        if !running {
            return Ok(inspect.exit_code.unwrap_or(-1));
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(inspect.exit_code.unwrap_or(-1));
        }
        sleep(Duration::from_millis(20)).await;
    }
}

fn frame_from_png(session_id: &str, data: Vec<u8>, cursor: Option<Point>) -> Result<Frame> {
    if data.len() < PNG_MAGIC.len() || !data.starts_with(PNG_MAGIC) {
        return Err(Error::InvalidPng);
    }
    let (width, height) = png_dimensions(&data).ok_or(Error::InvalidPng)?;
    Ok(Frame {
        kind: FrameKind::Frame,
        session_id: session_id.to_string(),
        ts: now_ts(),
        width,
        height,
        mime: "image/png".into(),
        data,
        cursor,
    })
}

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn workspace_id_from_req(req: &LeaseRequest) -> String {
    req.workspace
        .as_ref()
        .map(|w| w.id.as_str())
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| new_id("ws"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::argv_batch;
    use berthos_protocol::{Class, Density, License, Os, Resources, Term};

    #[test]
    fn skip_rest_from_argv_errors() {
        let ack = argv_batch(
            "a_skip",
            &[
                Action::Wait { ms: 1 },
                Action::Shell {
                    cmd: "uname".into(),
                },
                Action::Wait { ms: 2 },
            ],
        );
        assert!(ack.results[0].ok);
        assert!(!ack.results[1].ok);
        assert_eq!(ack.results[2].error.as_deref(), Some("skipped"));
    }

    #[test]
    fn mvp_gate() {
        let mut req = LeaseRequest {
            os: Os::Linux,
            class: Class::Private,
            license: License::Linux,
            density: Density::Isolated,
            pooled: false,
            term: Term::OnDemand,
            resources: Resources {
                vcpu: 1,
                mem_gib: 1,
                disk_gib: 1,
            },
            workspace: None,
            object: None,
            cpu_overcommit: None,
            min_seconds: 60,
            max_seconds: 0,
            exclusive_hardware: false,
            capabilities: vec![],
            image: None,
            region: None,
            isolation: berthos_protocol::Isolation::Vm,
            network: None,
            recording: None,
            human_confirm: vec![],
            preemptible: None,
        };
        req.validate_mvp().unwrap();
        req.os = Os::Windows;
        assert!(req.validate_mvp().is_err());
    }

    #[test]
    fn missing_ihdr_is_invalid_png() {
        let mut data = vec![0_u8; 16];
        data[..8].copy_from_slice(PNG_MAGIC);
        assert!(matches!(
            frame_from_png("s_1", data, None),
            Err(Error::InvalidPng)
        ));
    }

    #[test]
    fn missing_docker_object_is_gone() {
        assert!(docker_not_found(
            &bollard::errors::Error::DockerResponseServerError {
                status_code: 404,
                message: "network not found".into(),
            }
        ));
        assert!(docker_not_found(
            &bollard::errors::Error::DockerResponseServerError {
                status_code: 404,
                message: "No such container".into(),
            }
        ));
        assert!(!docker_not_found(
            &bollard::errors::Error::DockerResponseServerError {
                status_code: 403,
                message: "active endpoints".into(),
            }
        ));
    }
}

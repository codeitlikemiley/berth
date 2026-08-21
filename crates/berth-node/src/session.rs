use std::time::{Duration, SystemTime, UNIX_EPOCH};

use berth_protocol::{
    Ack, AckKind, AckResult, Action, ActionBatch, Frame, FrameKind, LeaseRequest,
};
use bollard::Docker;
use bollard::exec::StartExecResults;
use bollard::models::{ExecConfig, HostConfig};
use bollard::query_parameters::{CreateContainerOptionsBuilder, RemoveContainerOptionsBuilder};
use futures_util::StreamExt;
use tokio::time::sleep;

use crate::action::{
    ACTION_BIN, FRAME_HEIGHT, FRAME_WIDTH, PNG_MAGIC, action_argv, key_repeats, png_dimensions,
};
use crate::docker::{
    VIEWER_PORT, container_body, container_name, image_from_env, volume_create, volume_name,
};
use crate::error::{Error, Result};

const READY_TIMEOUT: Duration = Duration::from_secs(30);
const READY_POLL: Duration = Duration::from_millis(200);

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
        let workspace_id = req
            .workspace
            .as_ref()
            .map(|w| w.id.as_str())
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| new_id("ws"));
        let volume = volume_name(&workspace_id);
        self.ensure_volume(&volume).await?;

        let name = container_name(&session_id);
        let body = container_body(
            &self.image,
            &session_id,
            &workspace_id,
            &req.resources,
            &volume,
        )?;
        debug_assert_isolated(body.host_config.as_ref());

        let created = self
            .docker
            .create_container(
                Some(CreateContainerOptionsBuilder::default().name(&name).build()),
                body,
            )
            .await?;
        let container_id = created.id;

        if let Err(err) = self.boot(&container_id).await {
            let _ = self.remove_container(&container_id).await;
            return Err(err);
        }

        let viewer_port = self.viewer_port(&container_id).await;
        Ok(Session {
            docker: self.docker.clone(),
            container_id,
            container_name: name,
            session_id,
            workspace_id,
            volume,
            viewer_port,
        })
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

    async fn remove_container(&self, container_id: &str) -> Result<()> {
        self.docker
            .remove_container(
                container_id,
                Some(
                    RemoveContainerOptionsBuilder::default()
                        .force(true)
                        .v(false)
                        .build(),
                ),
            )
            .await?;
        Ok(())
    }
}

/// One running guest desktop. Stop removes the container and keeps the volume.
#[derive(Debug)]
pub struct Session {
    docker: Docker,
    container_id: String,
    container_name: String,
    session_id: String,
    workspace_id: String,
    volume: String,
    viewer_port: Option<u16>,
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

    pub fn viewer_port(&self) -> Option<u16> {
        self.viewer_port
    }

    /// Run `items` in order via `/usr/local/bin/action.sh`. First failure skips the rest.
    pub async fn exec(&self, batch: ActionBatch) -> Result<Ack> {
        let n = batch.items.len();
        let mut results = Vec::with_capacity(n);
        let mut skip = false;
        for (i, item) in batch.items.iter().enumerate() {
            let i = i as u32;
            if skip {
                results.push(skipped(i));
                continue;
            }
            match run_item(&self.docker, &self.container_id, item).await {
                Ok(ok) => results.push(ok_result(i, item, ok)),
                Err(ItemError::Action(error)) => {
                    results.push(AckResult {
                        i,
                        ok: false,
                        frame: false,
                        error: Some(error),
                    });
                    skip = true;
                }
                Err(ItemError::Docker(err)) => return Err(err),
            }
        }
        Ok(Ack {
            kind: AckKind::Ack,
            id: batch.id,
            results,
        })
    }

    pub async fn screenshot(&self) -> Result<Frame> {
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
        frame_from_png(&self.session_id, out.stdout)
    }

    /// Remove the container. The workspace volume is kept.
    pub async fn stop(self) -> Result<()> {
        self.docker
            .remove_container(
                &self.container_id,
                Some(
                    RemoveContainerOptionsBuilder::default()
                        .force(true)
                        .v(false)
                        .build(),
                ),
            )
            .await?;
        Ok(())
    }
}

enum ItemError {
    Action(String),
    Docker(Error),
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
    item: &Action,
) -> std::result::Result<bool, ItemError> {
    let argv = action_argv(item).map_err(ItemError::Action)?;
    let repeats = key_repeats(item);
    if repeats == 0 {
        return Ok(false);
    }
    for _ in 0..repeats {
        let out = exec_cmd(docker, container_id, &argv)
            .await
            .map_err(ItemError::Docker)?;
        if out.exit_code != 0 {
            let msg = if out.stderr_lossy().is_empty() {
                format!("action.sh exited {}", out.exit_code)
            } else {
                format!("action.sh exited {}: {}", out.exit_code, out.stderr_lossy())
            };
            return Err(ItemError::Action(msg));
        }
    }
    Ok(matches!(item, Action::Screenshot {}))
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
    let inspect = docker.inspect_exec(&created.id).await?;
    Ok(ExecOut {
        exit_code: inspect.exit_code.unwrap_or(-1),
        stdout,
        stderr,
    })
}

fn frame_from_png(session_id: &str, data: Vec<u8>) -> Result<Frame> {
    if data.len() < PNG_MAGIC.len() || !data.starts_with(PNG_MAGIC) {
        return Err(Error::InvalidPng);
    }
    let (width, height) = png_dimensions(&data).unwrap_or((FRAME_WIDTH, FRAME_HEIGHT));
    Ok(Frame {
        kind: FrameKind::Frame,
        session_id: session_id.to_string(),
        ts: now_ts(),
        width,
        height,
        mime: "image/png".into(),
        data,
        cursor: None,
    })
}

fn ok_result(i: u32, item: &Action, has_frame: bool) -> AckResult {
    AckResult {
        i,
        ok: true,
        frame: has_frame || matches!(item, Action::Screenshot {}),
        error: None,
    }
}

fn skipped(i: u32) -> AckResult {
    AckResult {
        i,
        ok: false,
        frame: false,
        error: Some("skipped".into()),
    }
}

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn new_id(prefix: &str) -> String {
    let ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{prefix}_{ns:x}")
}

fn debug_assert_isolated(host: Option<&HostConfig>) {
    let Some(host) = host else {
        return;
    };
    debug_assert_ne!(host.network_mode.as_deref(), Some("host"));
    debug_assert_eq!(host.privileged, Some(false));
    debug_assert!(host.binds.as_ref().is_none_or(Vec::is_empty));
}

#[cfg(test)]
mod tests {
    use super::*;
    use berth_protocol::{Class, Density, License, Os, Resources, Term};

    #[test]
    fn skip_rest_shape() {
        let results = [
            AckResult {
                i: 0,
                ok: false,
                frame: false,
                error: Some("click mods are not supported by action.sh".into()),
            },
            skipped(1),
            skipped(2),
        ];
        assert_eq!(results[1].error.as_deref(), Some("skipped"));
        assert!(!results[1].ok);
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
            isolation: berth_protocol::Isolation::Vm,
            network: None,
            recording: None,
            human_confirm: vec![],
            preemptible: None,
        };
        req.validate_mvp().unwrap();
        req.os = Os::Windows;
        assert!(req.validate_mvp().is_err());
    }
}

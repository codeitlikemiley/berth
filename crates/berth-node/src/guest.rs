use berth_protocol::{ActionBatch, LeaseRequest};

use crate::error::Result;
use crate::session::{ExecOutput, Session, SessionManager};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GuestMode {
    Docker,
    #[cfg(test)]
    Stub,
}

pub(crate) enum Guest {
    Docker(Box<Session>),
    #[cfg(test)]
    Stub(StubSession),
}

#[cfg(test)]
pub(crate) struct StubSession {
    session_id: String,
    workspace_id: String,
    viewer_port: Option<u16>,
    stopped: bool,
}

impl Guest {
    pub(crate) async fn start(
        mode: GuestMode,
        docker: &tokio::sync::Mutex<Option<SessionManager>>,
        req: &LeaseRequest,
    ) -> Result<Self> {
        match mode {
            #[cfg(test)]
            GuestMode::Stub => Ok(Self::Stub(StubSession::start(req)?)),
            GuestMode::Docker => {
                let mgr = {
                    let mut slot = docker.lock().await;
                    if slot.is_none() {
                        *slot = Some(SessionManager::connect()?);
                    }
                    slot.as_ref().expect("docker slot").clone()
                };
                Ok(Self::Docker(Box::new(mgr.start(req).await?)))
            }
        }
    }

    pub(crate) fn session_id(&self) -> &str {
        match self {
            Self::Docker(s) => s.session_id(),
            #[cfg(test)]
            Self::Stub(s) => &s.session_id,
        }
    }

    pub(crate) fn workspace_id(&self) -> &str {
        match self {
            Self::Docker(s) => s.workspace_id(),
            #[cfg(test)]
            Self::Stub(s) => &s.workspace_id,
        }
    }

    pub(crate) fn viewer_port(&self) -> Option<u16> {
        match self {
            Self::Docker(s) => s.viewer_port(),
            #[cfg(test)]
            Self::Stub(s) => s.viewer_port,
        }
    }

    pub(crate) fn container_id(&self) -> Option<&str> {
        match self {
            Self::Docker(s) => Some(s.container_id()),
            #[cfg(test)]
            Self::Stub(_) => None,
        }
    }

    pub(crate) async fn exec(&mut self, batch: ActionBatch) -> Result<ExecOutput> {
        match self {
            Self::Docker(s) => s.exec(batch).await,
            #[cfg(test)]
            Self::Stub(s) => s.exec(batch),
        }
    }

    pub(crate) async fn stop(&mut self) -> Result<()> {
        match self {
            Self::Docker(s) => s.stop().await,
            #[cfg(test)]
            Self::Stub(s) => {
                s.stopped = true;
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod stub {
    use berth_protocol::{Ack, AckKind, AckResult, Action, ActionBatch, Frame, FrameKind};

    use super::StubSession;
    use crate::action::{action_argv, skipped};
    use crate::error::{Error, Result};
    use crate::id::new_id;
    use crate::session::{ExecOutput, workspace_id_from_req};

    /// 1×1 PNG so stub screenshot items still parse as a Frame.
    const TINY_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    impl StubSession {
        pub(crate) fn start(req: &berth_protocol::LeaseRequest) -> Result<Self> {
            req.validate_mvp()?;
            Ok(Self {
                session_id: new_id("s"),
                workspace_id: workspace_id_from_req(req),
                viewer_port: Some(6080),
                stopped: false,
            })
        }

        pub(crate) fn exec(&mut self, batch: ActionBatch) -> Result<ExecOutput> {
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
                match action_argv(item) {
                    Ok(_) => {
                        let frame = if matches!(item, Action::Screenshot {}) {
                            let frame = Frame {
                                kind: FrameKind::Frame,
                                session_id: self.session_id.clone(),
                                ts: 0,
                                width: 1,
                                height: 1,
                                mime: "image/png".into(),
                                data: TINY_PNG.to_vec(),
                                cursor: None,
                            };
                            frames.push(frame);
                            true
                        } else {
                            false
                        };
                        results.push(AckResult {
                            i,
                            ok: true,
                            frame,
                            error: None,
                        });
                    }
                    Err(error) => {
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
    }
}

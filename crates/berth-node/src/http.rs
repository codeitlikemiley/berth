use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as UrlPath, Request, State};
use axum::http::{HeaderMap, StatusCode, header::AUTHORIZATION};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Router, middleware};
use berth_protocol::{ActionBatch, LeaseRequest, Quote};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::db::{Db, LeaseRow, NewLease};
use crate::error::{Error, Result};
use crate::guest::{Guest, GuestMode};
use crate::id::{new_id, u64_from_i64};
use crate::session::SessionManager;

/// Container sessions boot in seconds; MATH.md isolated-VM 300s floor does not apply.
const CONTAINER_MIN_SECONDS: u64 = 60;

#[derive(Clone)]
pub struct AppState {
    inner: Arc<Inner>,
}

struct Inner {
    db: Db,
    bind: Mutex<SocketAddr>,
    live: tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<Guest>>>>,
    docker: tokio::sync::Mutex<Option<SessionManager>>,
    mode: GuestMode,
    shutting_down: watch::Sender<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaseView {
    pub lease_id: String,
    pub session_id: String,
    pub ws_url: String,
    pub viewer_url: Option<String>,
    pub quote: Quote,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billable_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_seconds: Option<u64>,
}

impl LeaseView {
    fn from_row(row: &LeaseRow) -> Result<Self> {
        let quote: Quote = serde_json::from_str(&row.quote_json)?;
        Ok(Self {
            lease_id: row.id.clone(),
            session_id: row.session_id.clone(),
            ws_url: row.ws_url.clone(),
            viewer_url: row.viewer_url.clone(),
            quote,
            status: row.status.clone(),
            billable_seconds: row.billable_seconds.map(u64_from_i64),
            elapsed_seconds: row.elapsed_seconds.map(u64_from_i64),
        })
    }
}

impl AppState {
    pub fn open(db_path: &Path, bind: SocketAddr) -> Result<Self> {
        Self::open_mode(db_path, bind, GuestMode::Docker)
    }

    #[cfg(test)]
    pub(crate) fn open_stub(db_path: &Path, bind: SocketAddr) -> Result<Self> {
        Self::open_mode(db_path, bind, GuestMode::Stub)
    }

    fn open_mode(db_path: &Path, bind: SocketAddr, mode: GuestMode) -> Result<Self> {
        let db = Db::open(db_path)?;
        db.ensure_pairing_code()?;
        let (shutting_down, _) = watch::channel(false);
        Ok(Self {
            inner: Arc::new(Inner {
                db,
                bind: Mutex::new(bind),
                live: tokio::sync::Mutex::new(HashMap::new()),
                docker: tokio::sync::Mutex::new(None),
                mode,
                shutting_down,
            }),
        })
    }

    fn set_bind(&self, bind: SocketAddr) {
        *self.inner.bind.lock().expect("bind lock") = bind;
    }

    fn pairing_code(&self) -> Result<String> {
        self.inner.db.pairing_code()
    }

    fn begin_shutdown(&self) {
        self.inner.shutting_down.send_replace(true);
    }

    fn is_shutting_down(&self) -> bool {
        *self.inner.shutting_down.borrow()
    }

    fn shutdown_rx(&self) -> watch::Receiver<bool> {
        self.inner.shutting_down.subscribe()
    }

    async fn docker_manager(&self) -> Result<SessionManager> {
        let mut slot = self.inner.docker.lock().await;
        if slot.is_none() {
            *slot = Some(SessionManager::connect()?);
        }
        Ok(slot.as_ref().expect("docker slot").clone())
    }

    async fn reap_session(&self, session_id: &str, container_id: Option<&str>) -> Result<()> {
        match self.inner.mode {
            GuestMode::Docker => {
                self.docker_manager()
                    .await?
                    .reap(session_id, container_id)
                    .await
            }
            #[cfg(test)]
            GuestMode::Stub => Ok(()),
        }
    }

    /// Stop live guests, reap Docker leftovers, write billable_seconds.
    async fn drain(&self) {
        let guests: Vec<_> = {
            let mut live = self.inner.live.lock().await;
            live.drain().map(|(_, g)| g).collect()
        };
        for guest in guests {
            let mut guest = guest.lock().await;
            let _ = guest.stop().await;
        }
        let Ok(active) = self.inner.db.active_leases() else {
            return;
        };
        for row in active {
            let _ = self
                .reap_session(&row.session_id, row.container_id.as_deref())
                .await;
            let _ = self.inner.db.stop_lease(&row.id);
        }
    }
}

pub fn default_db_path() -> Result<PathBuf> {
    let dir = match std::env::var_os("BERTH_HOME") {
        Some(home) => PathBuf::from(home),
        None => {
            let home = std::env::var_os("HOME").ok_or_else(|| {
                Error::Internal("HOME is not set; cannot open ~/.berth/node.db".into())
            })?;
            PathBuf::from(home).join(".berth")
        }
    };
    Ok(dir.join("node.db"))
}

pub fn router(state: AppState) -> Router {
    let authed = Router::new()
        .route("/v1/leases", post(create_lease))
        .route("/v1/leases/{id}", get(get_lease).delete(delete_lease))
        .route("/v1/sessions/{id}", get(session_ws))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/pair", post(pair))
        .merge(authed)
        .with_state(state)
}

pub async fn serve(bind: SocketAddr) -> Result<()> {
    let db_path = default_db_path()?;
    let state = AppState::open(&db_path, bind)?;
    let code = state.pairing_code()?;
    eprintln!("pairing code: {code}");
    let listener = tokio::net::TcpListener::bind(bind).await?;
    let actual = listener.local_addr()?;
    state.set_bind(actual);
    eprintln!("listening on {actual}");
    let shutdown_state = state.clone();
    let drain_state = state.clone();
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async move {
            wait_shutdown_signal().await;
            // Fail closed before axum stops accepting; drain after serve returns.
            shutdown_state.begin_shutdown();
        })
        .await?;
    drain_state.drain().await;
    Ok(())
}

async fn wait_shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut term =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(s) => s,
                Err(_) => {
                    let _ = ctrl_c.await;
                    return;
                }
            };
        tokio::select! {
            _ = ctrl_c => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = ctrl_c.await;
    }
}

pub fn serve_blocking(bind: SocketAddr) -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(serve(bind))
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = match &self {
            Error::Unauthorized => StatusCode::UNAUTHORIZED,
            Error::NotFound => StatusCode::NOT_FOUND,
            Error::Mvp(_) | Error::InvalidResources | Error::BadRequest(_) => {
                StatusCode::BAD_REQUEST
            }
            Error::Stopped => StatusCode::CONFLICT,
            Error::ShuttingDown => StatusCode::SERVICE_UNAVAILABLE,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = Json(serde_json::json!({ "error": self.to_string() }));
        (status, body).into_response()
    }
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    if token.is_empty() { None } else { Some(token) }
}

async fn auth_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response> {
    let token = bearer_token(req.headers()).ok_or(Error::Unauthorized)?;
    if !state.inner.db.bearer_valid(token)? {
        return Err(Error::Unauthorized);
    }
    Ok(next.run(req).await)
}

async fn healthz() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true }))
}

#[derive(Debug, Deserialize)]
struct PairRequest {
    code: String,
}

#[derive(Debug, Serialize)]
struct PairResponse {
    token: String,
}

async fn pair(
    State(state): State<AppState>,
    Json(body): Json<PairRequest>,
) -> Result<Json<PairResponse>> {
    if !state.inner.db.check_pair_code(&body.code)? {
        return Err(Error::Unauthorized);
    }
    let token = state.inner.db.issue_bearer()?;
    Ok(Json(PairResponse { token }))
}

async fn create_lease(
    State(state): State<AppState>,
    Json(mut req): Json<LeaseRequest>,
) -> Result<(StatusCode, Json<LeaseView>)> {
    if state.is_shutting_down() {
        return Err(Error::ShuttingDown);
    }
    req.min_seconds = req.min_seconds.max(CONTAINER_MIN_SECONDS);
    req.validate_mvp()?;
    let quote = Quote::from_request(&req)?;
    let mut guest = Guest::start(state.inner.mode, &state.inner.docker, &req).await?;
    if state.is_shutting_down() {
        let _ = guest.stop().await;
        return Err(Error::ShuttingDown);
    }
    let session_id = guest.session_id().to_string();
    let workspace_id = guest.workspace_id().to_string();
    let viewer_port = guest.viewer_port();
    let container_id = guest.container_id().map(str::to_string);
    let lease_id = new_id("l");
    let bind = *state.inner.bind.lock().expect("bind lock");
    let ws_url = format!("ws://{}/v1/sessions/{session_id}", advertise(bind));
    let viewer_url = viewer_port.map(|p| format!("http://127.0.0.1:{p}/vnc.html"));
    let persist = NewLease {
        lease_id: lease_id.clone(),
        session_id: session_id.clone(),
        workspace_id,
        request_json: serde_json::to_string(&req)?,
        quote_json: serde_json::to_string(&quote)?,
        ws_url: ws_url.clone(),
        viewer_url: viewer_url.clone(),
        min_seconds: quote.min_seconds,
        viewer_port,
        container_id,
    };
    if let Err(err) = state.inner.db.insert_lease(&persist) {
        let _ = guest.stop().await;
        return Err(err);
    }
    print_quote(&lease_id, &quote);
    state
        .inner
        .live
        .lock()
        .await
        .insert(session_id.clone(), Arc::new(tokio::sync::Mutex::new(guest)));
    Ok((
        StatusCode::CREATED,
        Json(LeaseView {
            lease_id,
            session_id,
            ws_url,
            viewer_url,
            quote,
            status: "active".into(),
            billable_seconds: None,
            elapsed_seconds: None,
        }),
    ))
}

async fn get_lease(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<LeaseView>> {
    let row = state.inner.db.get_lease(&id)?;
    Ok(Json(LeaseView::from_row(&row)?))
}

async fn delete_lease(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<LeaseView>> {
    let row = state.inner.db.get_lease(&id)?;
    if row.status != "stopped" {
        let handle = {
            let live = state.inner.live.lock().await;
            live.get(&row.session_id).cloned()
        };
        if let Some(handle) = handle {
            handle.lock().await.stop().await?;
        } else {
            state
                .reap_session(&row.session_id, row.container_id.as_deref())
                .await?;
        }
        state.inner.live.lock().await.remove(&row.session_id);
    }
    let row = state.inner.db.stop_lease(&id)?;
    Ok(Json(LeaseView::from_row(&row)?))
}

async fn session_ws(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
    ws: WebSocketUpgrade,
) -> Result<Response> {
    {
        let live = state.inner.live.lock().await;
        if !live.contains_key(&id) {
            return Err(Error::NotFound);
        }
    }
    Ok(ws.on_upgrade(move |socket| ws_loop(socket, id, state)))
}

async fn ws_loop(mut socket: WebSocket, session_id: String, state: AppState) {
    let mut shutdown = state.shutdown_rx();
    if *shutdown.borrow() {
        return;
    }
    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    break;
                }
            }
            msg = socket.recv() => {
                let Some(msg) = msg else { break; };
                let Ok(msg) = msg else { break; };
                match msg {
                    Message::Text(text) => {
                        if let Err(err) =
                            handle_text(&mut socket, &session_id, &state, text.as_str()).await
                        {
                            let payload = serde_json::json!({
                                "type": "error",
                                "error": err.to_string()
                            })
                            .to_string();
                            if socket.send(Message::Text(payload.into())).await.is_err() {
                                break;
                            }
                            if matches!(err, Error::NotFound | Error::Stopped) {
                                break;
                            }
                        }
                    }
                    Message::Binary(_) => {
                        let payload = serde_json::json!({
                            "type": "error",
                            "error": "expected text ActionBatch"
                        })
                        .to_string();
                        if socket.send(Message::Text(payload.into())).await.is_err() {
                            break;
                        }
                    }
                    Message::Close(_) => break,
                    Message::Ping(_) | Message::Pong(_) => {}
                }
            }
        }
    }
}

async fn handle_text(
    socket: &mut WebSocket,
    session_id: &str,
    state: &AppState,
    text: &str,
) -> Result<()> {
    let out = run_session_batch(state, session_id, serde_json::from_str(text)?).await?;
    let ack = serde_json::to_string(&out.ack)?;
    socket
        .send(Message::Text(ack.into()))
        .await
        .map_err(|err| Error::Internal(err.to_string()))?;
    for frame in out.frames {
        let json = serde_json::to_string(&frame)?;
        socket
            .send(Message::Text(json.into()))
            .await
            .map_err(|err| Error::Internal(err.to_string()))?;
    }
    Ok(())
}

async fn run_session_batch(
    state: &AppState,
    session_id: &str,
    batch: ActionBatch,
) -> Result<crate::session::ExecOutput> {
    if batch.session_id != session_id {
        return Err(Error::BadRequest("session_id mismatch".into()));
    }
    let handle = {
        let live = state.inner.live.lock().await;
        live.get(session_id).cloned().ok_or(Error::NotFound)?
    };
    let mut guest = handle.lock().await;
    guest.exec(batch).await
}

fn advertise(bind: SocketAddr) -> SocketAddr {
    if bind.ip().is_unspecified() {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), bind.port())
    } else {
        bind
    }
}

fn print_quote(lease_id: &str, quote: &Quote) {
    let usd = quote
        .usd_per_second()
        .ok()
        .map(|rate| rate * quote.min_seconds as f64)
        .unwrap_or(0.0);
    eprintln!(
        "lease {lease_id} quote ${usd:.6} USD for {}s min (not charged)",
        quote.min_seconds
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use berth_protocol::{Action, ActionBatchKind};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn sample_lease() -> serde_json::Value {
        serde_json::json!({
            "os": "linux",
            "class": "private",
            "license": "linux",
            "density": "isolated",
            "term": "on_demand",
            "resources": { "vcpu": 2, "mem_gib": 4, "disk_gib": 40 }
        })
    }

    fn test_state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let bind: SocketAddr = "127.0.0.1:7432".parse().unwrap();
        let state = AppState::open_stub(&dir.path().join("node.db"), bind).unwrap();
        (dir, state)
    }

    async fn body_json(res: Response) -> serde_json::Value {
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn pair_token(app: &Router, code: &str) -> String {
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/pair")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(r#"{{"code":"{code}"}}"#)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let json = body_json(res).await;
        json["token"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn healthz_no_auth() {
        let (_dir, state) = test_state();
        let app = router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let json = body_json(res).await;
        assert_eq!(json["ok"], true);
    }

    #[tokio::test]
    async fn pair_issues_bearer_and_rotates() {
        let (_dir, state) = test_state();
        let code = state.pairing_code().unwrap();
        let app = router(state);

        let bad = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/pair")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"code":"NOPE-NOPE"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(bad.status(), StatusCode::UNAUTHORIZED);

        let t1 = pair_token(&app, &code).await;
        assert!(t1.starts_with("brt_"));
        let t2 = pair_token(&app, &code).await;
        assert_ne!(t1, t2);

        let stale = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/leases/l_missing")
                    .header("authorization", format!("Bearer {t1}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(stale.status(), StatusCode::UNAUTHORIZED);

        let missing = app
            .oneshot(
                Request::builder()
                    .uri("/v1/leases/l_missing")
                    .header("authorization", format!("Bearer {t2}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn leases_require_bearer() {
        let (_dir, state) = test_state();
        let app = router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/leases")
                    .header("content-type", "application/json")
                    .body(Body::from(sample_lease().to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn mvp_gate_before_guest() {
        let (_dir, state) = test_state();
        let code = state.pairing_code().unwrap();
        let app = router(state);
        let token = pair_token(&app, &code).await;
        let body = serde_json::json!({
            "os": "windows",
            "class": "private",
            "license": "w365-agents",
            "density": "isolated",
            "term": "on_demand",
            "resources": { "vcpu": 2, "mem_gib": 4, "disk_gib": 40 }
        });
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/leases")
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let json = body_json(res).await;
        assert!(json["error"].as_str().unwrap().contains("Windows"));

        let mesh = serde_json::json!({
            "os": "linux",
            "class": "mesh",
            "license": "linux",
            "density": "shared",
            "term": "on_demand",
            "resources": { "vcpu": 2, "mem_gib": 4, "disk_gib": 40 }
        });
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/leases")
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(mesh.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let json = body_json(res).await;
        assert!(json["error"].as_str().unwrap().contains("mesh"));
    }

    #[tokio::test]
    async fn stub_lease_quote_ws_and_billable() {
        let (_dir, state) = test_state();
        let code = state.pairing_code().unwrap();
        let app = router(state.clone());
        let token = pair_token(&app, &code).await;

        let created = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/leases")
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(sample_lease().to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::CREATED);
        let lease = body_json(created).await;
        assert!(lease["lease_id"].as_str().unwrap().starts_with("l_"));
        let session_id = lease["session_id"].as_str().unwrap().to_string();
        assert!(session_id.starts_with("s_"));
        assert_eq!(
            lease["ws_url"],
            format!("ws://127.0.0.1:7432/v1/sessions/{session_id}")
        );
        assert_eq!(lease["viewer_url"], "http://127.0.0.1:6080/vnc.html");
        assert_eq!(lease["quote"]["min_seconds"], 60);
        assert_eq!(lease["quote"]["density_mult"], 1.0);
        assert_eq!(lease["status"], "active");
        assert!(lease.get("billable_seconds").is_none() || lease["billable_seconds"].is_null());

        let batch = ActionBatch {
            kind: ActionBatchKind::Actions,
            id: "a_1".into(),
            session_id: session_id.clone(),
            items: vec![Action::Wait { ms: 1 }, Action::Screenshot {}],
        };
        let out = run_session_batch(&state, &session_id, batch).await.unwrap();
        assert!(out.ack.results[0].ok);
        assert!(out.ack.results[1].ok && out.ack.results[1].frame);
        assert_eq!(out.frames.len(), 1);

        let lease_id = lease["lease_id"].as_str().unwrap().to_string();
        let deleted = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/v1/leases/{lease_id}"))
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(deleted.status(), StatusCode::OK);
        let stopped = body_json(deleted).await;
        assert_eq!(stopped["status"], "stopped");
        assert_eq!(stopped["billable_seconds"], 60);

        let got = app
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/leases/{lease_id}"))
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(got.status(), StatusCode::OK);
        let json = body_json(got).await;
        assert_eq!(json["billable_seconds"], 60);
    }

    #[tokio::test]
    async fn ws_requires_bearer() {
        let (_dir, state) = test_state();
        let app = router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/v1/sessions/s_1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn ws_action_batch_ack_and_frame() {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message as WsMsg;
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;

        let dir = tempfile::tempdir().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let state = AppState::open_stub(&dir.path().join("node.db"), addr).unwrap();
        let code = state.pairing_code().unwrap();
        let app = router(state.clone());
        let token = pair_token(&app, &code).await;

        let created = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/leases")
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(sample_lease().to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::CREATED);
        let lease = body_json(created).await;
        let session_id = lease["session_id"].as_str().unwrap().to_string();

        tokio::spawn(async move {
            axum::serve(listener, router(state)).await.unwrap();
        });

        let mut req = format!("ws://{addr}/v1/sessions/{session_id}")
            .into_client_request()
            .unwrap();
        req.headers_mut()
            .insert(AUTHORIZATION, format!("Bearer {token}").parse().unwrap());
        let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
        let batch = serde_json::json!({
            "type": "actions",
            "id": "a_ws",
            "session_id": session_id,
            "items": [{ "op": "screenshot" }]
        });
        ws.send(WsMsg::Text(batch.to_string().into()))
            .await
            .unwrap();

        let ack = ws.next().await.unwrap().unwrap();
        let ack_json: serde_json::Value =
            serde_json::from_str(ack.into_text().unwrap().as_str()).unwrap();
        assert_eq!(ack_json["type"], "ack");
        assert_eq!(ack_json["id"], "a_ws");
        assert_eq!(ack_json["results"][0]["ok"], true);
        assert_eq!(ack_json["results"][0]["frame"], true);

        let frame = ws.next().await.unwrap().unwrap();
        let frame_json: serde_json::Value =
            serde_json::from_str(frame.into_text().unwrap().as_str()).unwrap();
        assert_eq!(frame_json["type"], "frame");
        assert_eq!(frame_json["session_id"], session_id);
        assert_eq!(frame_json["mime"], "image/png");
    }

    #[tokio::test]
    async fn create_lease_rejected_while_shutting_down() {
        let (_dir, state) = test_state();
        let code = state.pairing_code().unwrap();
        let app = router(state.clone());
        let token = pair_token(&app, &code).await;
        state.begin_shutdown();
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/leases")
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(sample_lease().to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(state.inner.db.active_leases().unwrap().is_empty());
    }

    #[tokio::test]
    async fn drain_settles_active_leases() {
        let (_dir, state) = test_state();
        let code = state.pairing_code().unwrap();
        let app = router(state.clone());
        let token = pair_token(&app, &code).await;
        let created = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/leases")
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(sample_lease().to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let lease = body_json(created).await;
        let lease_id = lease["lease_id"].as_str().unwrap().to_string();

        state.begin_shutdown();
        state.drain().await;

        let got = app
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/leases/{lease_id}"))
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(got.status(), StatusCode::OK);
        let json = body_json(got).await;
        assert_eq!(json["status"], "stopped");
        assert_eq!(json["billable_seconds"], 60);
    }

    #[tokio::test]
    async fn delete_settles_without_live_handle() {
        let (_dir, state) = test_state();
        let code = state.pairing_code().unwrap();
        let app = router(state.clone());
        let token = pair_token(&app, &code).await;
        let created = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/leases")
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(sample_lease().to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let lease = body_json(created).await;
        let lease_id = lease["lease_id"].as_str().unwrap().to_string();
        state.inner.live.lock().await.clear();

        let deleted = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/v1/leases/{lease_id}"))
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(deleted.status(), StatusCode::OK);
        let json = body_json(deleted).await;
        assert_eq!(json["status"], "stopped");
        assert_eq!(json["billable_seconds"], 60);
    }
}

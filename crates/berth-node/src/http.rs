use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::Json;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as UrlPath, Request, State};
use axum::http::header::{AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE, HOST};
use axum::http::{HeaderMap, HeaderName, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Router, middleware};
use berth_protocol::{ActionBatch, LeaseRequest, Quote, parse_allowlist};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::db::{Db, EndReason, LeaseRow, NewLease};
use crate::docker::image_from_env;
use crate::error::{Error, Result};
use crate::guest::{Guest, GuestMode};
use crate::id::{new_id, u64_from_i64};
use crate::session::SessionManager;
use crate::tunnel::{self, TunnelKind};

/// Container sessions boot in seconds; MATH.md isolated-VM 300s floor does not apply.
const CONTAINER_MIN_SECONDS: u64 = 60;
const LIST_LEASES_MAX: usize = 500;

#[derive(Clone)]
pub struct AppState {
    inner: Arc<Inner>,
}

struct Inner {
    db: Db,
    data_dir: PathBuf,
    bind: Mutex<SocketAddr>,
    origin: Mutex<Option<String>>,
    live: tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<Guest>>>>,
    docker: tokio::sync::Mutex<Option<SessionManager>>,
    mode: GuestMode,
    shutting_down: watch::Sender<bool>,
    tunnel_cf: AtomicBool,
    tunnel_named: AtomicBool,
    tunnel_alive: AtomicBool,
    /// Tests pause after Guest::start so unpark can win the boot window.
    #[cfg(test)]
    create_hold: tokio::sync::Mutex<Option<CreateHold>>,
}

#[cfg(test)]
struct CreateHold {
    started: tokio::sync::oneshot::Sender<()>,
    resume: tokio::sync::oneshot::Receiver<()>,
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
    pub started_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped_at: Option<i64>,
    pub workspace_id: String,
    pub live: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_reason: Option<String>,
    pub forfeited: bool,
}

#[derive(Debug, Serialize)]
struct LeaseList {
    leases: Vec<LeaseView>,
    truncated: bool,
}

impl LeaseView {
    fn from_row(row: &LeaseRow, origin: Option<&str>, live: bool) -> Result<Self> {
        let quote: Quote = serde_json::from_str(&row.quote_json)?;
        Ok(Self {
            lease_id: row.id.clone(),
            session_id: row.session_id.clone(),
            ws_url: session_ws_url_stored(&row.ws_url, origin, &row.session_id),
            viewer_url: row.viewer_url.clone(),
            quote,
            status: row.status.clone(),
            billable_seconds: row.billable_seconds.map(u64_from_i64),
            elapsed_seconds: row.elapsed_seconds.map(u64_from_i64),
            started_at: row.started_at,
            stopped_at: row.stopped_at,
            workspace_id: row.workspace_id.clone(),
            live,
            end_reason: row.end_reason.clone(),
            forfeited: row.forfeited,
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
        let data_dir = db_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .to_path_buf();
        let (shutting_down, _) = watch::channel(false);
        Ok(Self {
            inner: Arc::new(Inner {
                db,
                data_dir,
                bind: Mutex::new(bind),
                origin: Mutex::new(None),
                live: tokio::sync::Mutex::new(HashMap::new()),
                docker: tokio::sync::Mutex::new(None),
                mode,
                shutting_down,
                tunnel_cf: AtomicBool::new(false),
                tunnel_named: AtomicBool::new(false),
                tunnel_alive: AtomicBool::new(false),
                #[cfg(test)]
                create_hold: tokio::sync::Mutex::new(None),
            }),
        })
    }

    fn set_bind(&self, bind: SocketAddr) {
        *self.inner.bind.lock().expect("bind lock") = bind;
    }

    fn set_origin(&self, origin: String) {
        *self.inner.origin.lock().expect("origin lock") = Some(origin);
    }

    fn origin(&self) -> Option<String> {
        self.inner.origin.lock().expect("origin lock").clone()
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

    #[cfg(test)]
    async fn after_guest_start_hold(&self) {
        let Some(hold) = self.inner.create_hold.lock().await.take() else {
            return;
        };
        let _ = hold.started.send(());
        let _ = hold.resume.await;
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
            let _ = self.inner.db.stop_lease(&row.id, EndReason::Graceful);
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
        .route("/v1/leases", get(list_leases).post(create_lease))
        .route("/v1/leases/{id}", get(get_lease).delete(delete_lease))
        .route("/v1/leases/{id}/force", post(force_lease))
        .route("/v1/node", get(get_node))
        .route("/v1/node/park", post(park_node))
        .route("/v1/node/unpark", post(unpark_node))
        .route("/v1/quote", post(quote_lease))
        .route("/v1/sessions/{id}", get(session_ws))
        .route("/v1/sessions/{id}/preview", get(session_preview))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/pair", post(pair))
        .route("/v1/pairing", get(get_pairing))
        .merge(authed)
        .fallback(console_fallback)
        .layer(middleware::from_fn(access_log))
        .with_state(state)
}

async fn console_fallback(State(state): State<AppState>, req: Request) -> Response {
    let origin = state.origin();
    let loopback = loopback_operator(req.headers(), origin.as_deref());
    crate::console::response(req.uri().path(), loopback)
}

pub async fn serve(bind: SocketAddr, tunnel: Option<TunnelKind>) -> Result<()> {
    if tunnel.is_some() && !bind.ip().is_loopback() {
        return Err(Error::Tunnel(
            "--tunnel requires a loopback bind (cloudflared is the public edge)".into(),
        ));
    }
    if matches!(tunnel, Some(TunnelKind::Cloudflare)) {
        tunnel::resolve_cloudflared()?;
    }
    let db_path = default_db_path()?;
    let state = AppState::open(&db_path, bind)?;
    let code = state.pairing_code()?;
    eprintln!("pairing code: {code}");
    let listener = tokio::net::TcpListener::bind(bind).await?;
    let actual = listener.local_addr()?;
    state.set_bind(actual);
    eprintln!("listening on {actual}");
    eprintln!("console: http://{actual}/");
    let tunnel_child = match tunnel {
        Some(TunnelKind::Cloudflare) => {
            let named = std::env::var("TUNNEL_TOKEN")
                .ok()
                .is_some_and(|s| !s.trim().is_empty());
            let (child, origin) = tunnel::start_cloudflare(actual).await?;
            if let Some(origin) = origin {
                state.set_origin(origin);
            }
            state.inner.tunnel_named.store(named, Ordering::Relaxed);
            state.inner.tunnel_cf.store(true, Ordering::Relaxed);
            state.inner.tunnel_alive.store(true, Ordering::Relaxed);
            Some(child)
        }
        None => None,
    };
    let shutdown_state = state.clone();
    let drain_state = state.clone();
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown_on_signal_or_tunnel(shutdown_state, tunnel_child))
        .await?;
    drain_state.drain().await;
    Ok(())
}

async fn shutdown_on_signal_or_tunnel(
    shutdown_state: AppState,
    mut tunnel_child: Option<tunnel::TunnelChild>,
) {
    if tunnel_child.is_none() {
        wait_shutdown_signal().await;
        shutdown_state.begin_shutdown();
        return;
    }
    {
        let child = tunnel_child.as_mut().expect("tunnel child");
        tokio::select! {
            _ = wait_shutdown_signal() => {}
            status = child.wait() => {
                match status {
                    Ok(st) => eprintln!("cloudflared exited ({st}); shutting down"),
                    Err(err) => eprintln!("cloudflared wait failed: {err}; shutting down"),
                }
                shutdown_state
                    .inner
                    .tunnel_alive
                    .store(false, Ordering::Relaxed);
                shutdown_state.begin_shutdown();
                return;
            }
        }
    }
    shutdown_state.begin_shutdown();
    if let Some(child) = tunnel_child.take() {
        child.shutdown().await;
    }
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

pub fn serve_blocking(bind: SocketAddr, tunnel: Option<TunnelKind>) -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(serve(bind, tunnel))
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = match &self {
            Error::Unauthorized => StatusCode::UNAUTHORIZED,
            Error::NotFound => StatusCode::NOT_FOUND,
            Error::Mvp(_) | Error::InvalidResources | Error::BadRequest(_) => {
                StatusCode::BAD_REQUEST
            }
            Error::Stopped | Error::TooManyBearers | Error::Unparked | Error::Occupied { .. } => {
                StatusCode::CONFLICT
            }
            Error::ShuttingDown => StatusCode::SERVICE_UNAVAILABLE,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = match &self {
            Error::Occupied { live_lease_id } => serde_json::json!({
                "error": self.to_string(),
                "live_lease_id": live_lease_id,
            }),
            _ => serde_json::json!({ "error": self.to_string() }),
        };
        (status, Json(body)).into_response()
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
    #[serde(default)]
    revoke_others: bool,
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
    let token = state.inner.db.issue_bearer(body.revoke_others)?;
    Ok(Json(PairResponse { token }))
}

async fn get_pairing(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<PairingView>> {
    let origin = state.origin();
    if !loopback_operator(&headers, origin.as_deref()) {
        return Err(Error::NotFound);
    }
    Ok(Json(PairingView {
        code: state.pairing_code()?,
    }))
}

#[derive(Debug, Serialize)]
struct PairingView {
    code: String,
}

async fn create_lease(
    State(state): State<AppState>,
    Json(mut req): Json<LeaseRequest>,
) -> Result<(StatusCode, Json<LeaseView>)> {
    if !state.inner.db.parked()? {
        return Err(Error::Unparked);
    }
    if state.is_shutting_down() {
        return Err(Error::ShuttingDown);
    }
    req.min_seconds = req.min_seconds.max(CONTAINER_MIN_SECONDS);
    req.validate_mvp()?;
    // Docker refuses an oversized container anyway, but only after a round trip
    // and in its own words. The node knows what it has; say so first.
    check_capacity(state.inner.mode, &req).await?;
    let quote = Quote::from_request(&req)?;
    let mut guest = Guest::start(state.inner.mode, &state.inner.docker, &req).await?;
    #[cfg(test)]
    state.after_guest_start_hold().await;
    if state.is_shutting_down() {
        let _ = guest.stop().await;
        return Err(Error::ShuttingDown);
    }
    if !state.inner.db.parked()? {
        let _ = guest.stop().await;
        return Err(Error::Unparked);
    }
    let session_id = guest.session_id().to_string();
    let workspace_id = guest.workspace_id().to_string();
    let viewer_port = guest.viewer_port();
    let container_id = guest.container_id().map(str::to_string);
    let lease_id = new_id("l");
    let bind = *state.inner.bind.lock().expect("bind lock");
    let origin = state.origin();
    let ws_url = session_ws_url(bind, origin.as_deref(), &session_id);
    let viewer_url = viewer_port.map(|p| format!("http://127.0.0.1:{p}/vnc.html"));
    let persist = NewLease {
        lease_id: lease_id.clone(),
        session_id: session_id.clone(),
        workspace_id,
        request_json: serde_json::to_string(&req)?,
        quote_json: serde_json::to_string(&quote)?,
        ws_url,
        viewer_url,
        min_seconds: quote.min_seconds,
        viewer_port,
        container_id,
    };
    if let Err(err) = state.inner.db.insert_lease(&persist) {
        let _ = guest.stop().await;
        return Err(err);
    }
    state
        .inner
        .live
        .lock()
        .await
        .insert(session_id, Arc::new(tokio::sync::Mutex::new(guest)));
    let row = state.inner.db.get_lease(&lease_id)?;
    let view = LeaseView::from_row(&row, origin.as_deref(), true)?;
    print_quote(&lease_id, &quote);
    Ok((StatusCode::CREATED, Json(view)))
}

async fn list_leases(State(state): State<AppState>) -> Result<Json<LeaseList>> {
    let rows = state.inner.db.list_leases()?;
    let truncated = rows.len() > LIST_LEASES_MAX;
    let origin = state.origin();
    let live = state.inner.live.lock().await;
    let leases = rows
        .iter()
        .take(LIST_LEASES_MAX)
        .map(|row| LeaseView::from_row(row, origin.as_deref(), live.contains_key(&row.session_id)))
        .collect::<Result<Vec<_>>>()?;
    Ok(Json(LeaseList { leases, truncated }))
}

async fn get_lease(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<LeaseView>> {
    let row = state.inner.db.get_lease(&id)?;
    let live = {
        let live = state.inner.live.lock().await;
        live.contains_key(&row.session_id)
    };
    Ok(Json(LeaseView::from_row(
        &row,
        state.origin().as_deref(),
        live,
    )?))
}

async fn delete_lease(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<LeaseView>> {
    stop_and_view(&state, &id, EndReason::Graceful).await
}

async fn force_lease(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<LeaseView>> {
    stop_and_view(&state, &id, EndReason::Forced).await
}

async fn stop_and_view(state: &AppState, id: &str, reason: EndReason) -> Result<Json<LeaseView>> {
    let row = state.inner.db.get_lease(id)?;
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
    let row = state.inner.db.stop_lease(id, reason)?;
    let live = {
        let live = state.inner.live.lock().await;
        live.contains_key(&row.session_id)
    };
    Ok(Json(LeaseView::from_row(
        &row,
        state.origin().as_deref(),
        live,
    )?))
}

async fn quote_lease(Json(mut req): Json<LeaseRequest>) -> Result<Json<Quote>> {
    req.min_seconds = req.min_seconds.max(CONTAINER_MIN_SECONDS);
    req.validate_mvp()?;
    Ok(Json(Quote::from_request(&req)?))
}

async fn park_node(State(state): State<AppState>) -> Result<Json<NodeView>> {
    state.inner.db.set_parked(true)?;
    Ok(Json(node_view(&state).await?))
}

async fn unpark_node(State(state): State<AppState>) -> Result<Json<NodeView>> {
    {
        let live = state.inner.live.lock().await;
        if let Some(session_id) = live.keys().next() {
            let live_lease_id = state
                .inner
                .db
                .lease_id_for_session(session_id)?
                .unwrap_or_else(|| session_id.clone());
            return Err(Error::Occupied { live_lease_id });
        }
    }
    state.inner.db.set_parked(false)?;
    Ok(Json(node_view(&state).await?))
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

async fn session_preview(
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Response> {
    let handle = {
        let live = state.inner.live.lock().await;
        live.get(&id).cloned()
    };
    let Some(handle) = handle else {
        return Err(Error::NotFound);
    };
    let png = {
        let guest = handle.lock().await;
        guest.last_frame().map(|frame| frame.data.clone())
    };
    match png {
        None => Ok(StatusCode::NO_CONTENT.into_response()),
        Some(data) => Ok((
            [
                (CONTENT_TYPE, "image/png"),
                (CACHE_CONTROL, "private, no-store"),
                (HeaderName::from_static("x-content-type-options"), "nosniff"),
            ],
            data,
        )
            .into_response()),
    }
}

async fn get_node(State(state): State<AppState>) -> Result<Json<NodeView>> {
    Ok(Json(node_view(&state).await?))
}

async fn node_view(state: &AppState) -> Result<NodeView> {
    let (docker, guest_image) = probe_docker_and_image(state.inner.mode).await;
    let home_writable = home_writable(&state.inner.data_dir);
    let (allowlist, allowlist_source) = node_allowlist();
    let live_sessions = state.inner.live.lock().await.len();
    let origin = state.origin();
    let capacity = docker_capacity(state.inner.mode).await;
    Ok(NodeView {
        capacity,
        ok: docker.ok && guest_image.ok && home_writable,
        parked: state.inner.db.parked()?,
        bind: state.inner.bind.lock().expect("bind lock").to_string(),
        origin,
        class: "private",
        image: image_from_env(),
        allowlist,
        allowlist_source,
        docker,
        guest_image,
        home_writable,
        tunnel: tunnel_view(state),
        active_bearers: state.inner.db.active_bearers()?,
        live_sessions,
        shutting_down: state.is_shutting_down(),
        host_desktop_driven: false,
    })
}

#[derive(Debug, Serialize)]
struct NodeView {
    ok: bool,
    parked: bool,
    bind: String,
    origin: Option<String>,
    class: &'static str,
    image: String,
    allowlist: Vec<String>,
    allowlist_source: &'static str,
    docker: DockerProbe,
    guest_image: GuestImageProbe,
    home_writable: bool,
    tunnel: TunnelView,
    active_bearers: u64,
    live_sessions: usize,
    shutting_down: bool,
    host_desktop_driven: bool,
    /// What this node can actually hand out, so a caller can size a lease
    /// before asking rather than finding out from Docker at container start.
    capacity: CapacityView,
}

#[derive(Debug, Serialize, Clone, Copy, Default)]
struct CapacityView {
    /// None when Docker could not be asked; callers must not treat that as zero.
    vcpu: Option<u32>,
    mem_gib: Option<u32>,
}

#[derive(Debug, Serialize)]
struct DockerProbe {
    ok: bool,
    detail: String,
}

#[derive(Debug, Serialize)]
struct GuestImageProbe {
    ok: bool,
    name: String,
    /// Why the image is or is not usable. An image that merely exists proves
    /// nothing about whether it filters egress, so say which it is.
    detail: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind")]
enum TunnelView {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "cloudflare")]
    Cloudflare { named: bool, child_alive: bool },
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

fn session_ws_url(bind: SocketAddr, origin: Option<&str>, session_id: &str) -> String {
    match origin {
        Some(origin) => session_ws_url_origin(origin, session_id),
        None => format!("ws://{}/v1/sessions/{session_id}", advertise(bind)),
    }
}

fn session_ws_url_stored(stored: &str, origin: Option<&str>, session_id: &str) -> String {
    match origin {
        Some(origin) => session_ws_url_origin(origin, session_id),
        None => stored.to_string(),
    }
}

fn session_ws_url_origin(origin: &str, session_id: &str) -> String {
    let ws = if let Some(rest) = origin.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = origin.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        format!("wss://{origin}")
    };
    format!("{ws}/v1/sessions/{session_id}")
}

fn format_access_log(method: &str, path: &str, status: u16, ms: u64) -> String {
    format!("{method} {path} {status} {ms}ms")
}

async fn access_log(req: Request, next: Next) -> Response {
    let method = req.method().as_str().to_owned();
    let path = req.uri().path().to_owned();
    let started = Instant::now();
    let res = next.run(req).await;
    let ms = started.elapsed().as_millis() as u64;
    eprintln!(
        "{}",
        format_access_log(&method, &path, res.status().as_u16(), ms)
    );
    res
}

fn hostname_from_host(host: &str) -> &str {
    let host = host.trim();
    if let Some(rest) = host.strip_prefix('[')
        && let Some(end) = rest.find(']')
    {
        return &rest[..end];
    }
    if host.parse::<IpAddr>().is_ok() {
        return host;
    }
    match host.rsplit_once(':') {
        Some((name, port)) if !name.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => name,
        _ => host,
    }
}

fn host_is_loopback(host: &str) -> bool {
    let host = hostname_from_host(host);
    host == "127.0.0.1" || host.eq_ignore_ascii_case("localhost") || host == "::1"
}

fn has_cdn_header(headers: &HeaderMap) -> bool {
    headers.keys().any(|name| {
        let n = name.as_str();
        n.eq_ignore_ascii_case("cf-ray")
            || n.eq_ignore_ascii_case("cf-connecting-ip")
            || n.eq_ignore_ascii_case("cdn-loop")
    })
}

fn origin_hostname(origin: &str) -> &str {
    let rest = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"))
        .unwrap_or(origin);
    hostname_from_host(rest.split('/').next().unwrap_or(rest))
}

pub(crate) fn loopback_operator(headers: &HeaderMap, origin: Option<&str>) -> bool {
    let Some(host) = headers.get(HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    if !host_is_loopback(host) {
        return false;
    }
    if has_cdn_header(headers) {
        return false;
    }
    if let Some(origin) = origin
        && hostname_from_host(host).eq_ignore_ascii_case(origin_hostname(origin))
    {
        return false;
    }
    true
}

fn node_allowlist() -> (Vec<String>, &'static str) {
    match std::env::var("BERTH_ALLOWLIST") {
        Err(_) => (parse_allowlist(None), "default"),
        Ok(raw) => {
            let list = parse_allowlist(Some(&raw));
            if list.is_empty() {
                (list, "deny-all")
            } else {
                (list, "env")
            }
        }
    }
}

fn home_writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(format!(".node-write.{}", std::process::id()));
    let ok = std::fs::write(&probe, b"ok\n").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

fn tunnel_view(state: &AppState) -> TunnelView {
    if !state.inner.tunnel_cf.load(Ordering::Relaxed) {
        return TunnelView::None;
    }
    TunnelView::Cloudflare {
        named: state.inner.tunnel_named.load(Ordering::Relaxed),
        child_alive: state.inner.tunnel_alive.load(Ordering::Relaxed),
    }
}

/// An image that exists is not necessarily an image that filters egress: the
/// guest built before the filter landed still inspects fine. Require the build
/// label instead, and hand back a line the operator can act on.
async fn inspect_guest_image(docker: &bollard::Docker, name: &str) -> (bool, String) {
    let rebuild = format!("rebuild: docker build -t {name} images/linux-xfce");
    let image = match docker.inspect_image(name).await {
        Ok(image) => image,
        Err(_) => return (false, format!("not found; {rebuild}")),
    };
    let label = image
        .config
        .as_ref()
        .and_then(|config| config.labels.as_ref())
        .and_then(|labels| labels.get(crate::docker::EGRESS_LABEL))
        .map(String::as_str);
    match label {
        Some(version) if version == crate::docker::EGRESS_VERSION => {
            (true, format!("egress filter v{version}"))
        }
        Some(version) => (
            false,
            format!(
                "egress contract v{version}, node expects v{}; {rebuild}",
                crate::docker::EGRESS_VERSION
            ),
        ),
        None => (
            false,
            format!(
                "no {} label; image predates the egress filter; {rebuild}",
                crate::docker::EGRESS_LABEL
            ),
        ),
    }
}

/// Host resources Docker reports. `None` for a field means "could not ask",
/// which must never be read as "zero available".
async fn docker_capacity(mode: GuestMode) -> CapacityView {
    match mode {
        #[cfg(test)]
        GuestMode::Stub => CapacityView {
            vcpu: Some(8),
            mem_gib: Some(16),
        },
        GuestMode::Docker => match bollard::Docker::connect_with_local_defaults() {
            Err(_) => CapacityView::default(),
            Ok(docker) => match docker.info().await {
                Err(_) => CapacityView::default(),
                Ok(info) => CapacityView {
                    vcpu: info.ncpu.and_then(|n| u32::try_from(n).ok()),
                    mem_gib: info
                        .mem_total
                        .and_then(|b| u32::try_from(b / (1024 * 1024 * 1024)).ok()),
                },
            },
        },
    }
}

/// Refuse a lease the node demonstrably cannot host. Silence when capacity is
/// unknown: a probe failure is not evidence the request is too big.
async fn check_capacity(mode: GuestMode, req: &LeaseRequest) -> Result<()> {
    let cap = docker_capacity(mode).await;
    if let Some(vcpu) = cap.vcpu
        && req.resources.vcpu > vcpu
    {
        return Err(Error::BadRequest(format!(
            "vcpu {} exceeds this node's {vcpu} available CPUs",
            req.resources.vcpu
        )));
    }
    if let Some(mem) = cap.mem_gib
        && req.resources.mem_gib > mem
    {
        return Err(Error::BadRequest(format!(
            "mem_gib {} exceeds this node's {mem} GiB of memory",
            req.resources.mem_gib
        )));
    }
    Ok(())
}

async fn probe_docker_and_image(mode: GuestMode) -> (DockerProbe, GuestImageProbe) {
    let name = image_from_env();
    match mode {
        #[cfg(test)]
        GuestMode::Stub => (
            DockerProbe {
                ok: true,
                detail: "bollard ping ok".into(),
            },
            GuestImageProbe {
                ok: true,
                name,
                detail: "stub".into(),
            },
        ),
        GuestMode::Docker => match bollard::Docker::connect_with_local_defaults() {
            Ok(docker) => {
                let (ok, detail) = match docker.ping().await {
                    Ok(_) => (true, "bollard ping ok".into()),
                    Err(err) => (false, format!("bollard ping failed: {err}")),
                };
                let (image_ok, image_detail) = inspect_guest_image(&docker, &name).await;
                (
                    DockerProbe { ok, detail },
                    GuestImageProbe {
                        ok: image_ok,
                        name,
                        detail: image_detail,
                    },
                )
            }
            Err(err) => (
                DockerProbe {
                    ok: false,
                    detail: format!("docker not reachable: {err}"),
                },
                GuestImageProbe {
                    ok: false,
                    name,
                    detail: "docker not reachable".into(),
                },
            ),
        },
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
    async fn pair_issues_bearer_and_keeps_previous() {
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

        let first = app
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
        assert_eq!(first.status(), StatusCode::NOT_FOUND);

        let second = app
            .oneshot(
                Request::builder()
                    .uri("/v1/leases/l_missing")
                    .header("authorization", format!("Bearer {t2}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(second.status(), StatusCode::NOT_FOUND);
    }

    async fn pair_token_revoke(app: &Router, code: &str) -> String {
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/pair")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"code":"{code}","revoke_others":true}}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let json = body_json(res).await;
        json["token"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn pair_revoke_others_invalidates_previous() {
        let (_dir, state) = test_state();
        let code = state.pairing_code().unwrap();
        let app = router(state);
        let t1 = pair_token(&app, &code).await;
        let t2 = pair_token_revoke(&app, &code).await;
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
    async fn pair_ninth_without_revoke_is_conflict() {
        let (_dir, state) = test_state();
        let code = state.pairing_code().unwrap();
        let app = router(state);
        for _ in 0..8 {
            let _ = pair_token(&app, &code).await;
        }
        let ninth = app
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
        assert_eq!(ninth.status(), StatusCode::CONFLICT);
        let json = body_json(ninth).await;
        let err = json["error"].as_str().unwrap();
        assert!(err.contains("too many paired clients"), "{err}");
        assert!(err.contains("revoke_others"), "{err}");

        let rotated = pair_token_revoke(&app, &code).await;
        assert!(rotated.starts_with("brt_"));
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
        let windows_err = json["error"].as_str().unwrap();
        assert!(windows_err.contains("Windows"), "{windows_err}");
        assert!(windows_err.contains("v0.1"), "{windows_err}");

        let macos = serde_json::json!({
            "os": "macos",
            "class": "private",
            "license": "apple-private",
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
                    .body(Body::from(macos.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let json = body_json(res).await;
        let macos_err = json["error"].as_str().unwrap();
        assert!(macos_err.contains("macOS"), "{macos_err}");
        assert!(macos_err.contains("v0.1"), "{macos_err}");

        let mesh = serde_json::json!({
            "os": "linux",
            "class": "mesh",
            "license": "linux",
            "density": "shared",
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
                    .body(Body::from(mesh.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let json = body_json(res).await;
        let mesh_err = json["error"].as_str().unwrap();
        assert!(mesh_err.contains("class=mesh"), "{mesh_err}");
        assert!(mesh_err.contains("not implemented"), "{mesh_err}");

        let zero = serde_json::json!({
            "os": "linux",
            "class": "private",
            "license": "linux",
            "density": "isolated",
            "term": "on_demand",
            "resources": { "vcpu": 0, "mem_gib": 4, "disk_gib": 40 }
        });
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/leases")
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(zero.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let json = body_json(res).await;
        let zero_err = json["error"].as_str().unwrap();
        assert!(zero_err.contains("greater than zero"), "{zero_err}");
        assert!(zero_err.contains("not unlimited"), "{zero_err}");
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
        assert_eq!(lease["live"], true);
        assert_eq!(lease["forfeited"], false);
        assert!(lease.get("end_reason").is_none() || lease["end_reason"].is_null());
        assert!(lease["started_at"].as_i64().unwrap() > 0);
        assert!(
            lease["workspace_id"].as_str().unwrap().starts_with("ws_"),
            "{}",
            lease["workspace_id"]
        );
        assert!(lease.get("stopped_at").is_none() || lease["stopped_at"].is_null());
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
        assert_eq!(stopped["live"], false);
        assert_eq!(stopped["end_reason"], "graceful");
        assert_eq!(stopped["forfeited"], false);
        assert!(stopped["stopped_at"].as_i64().unwrap() > 0);

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
        assert_eq!(json["live"], false);
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
    async fn create_aborts_if_unparked_after_guest_start() {
        let (_dir, state) = test_state();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (resume_tx, resume_rx) = tokio::sync::oneshot::channel();
        *state.inner.create_hold.lock().await = Some(CreateHold {
            started: started_tx,
            resume: resume_rx,
        });
        let code = state.pairing_code().unwrap();
        let app = router(state.clone());
        let token = pair_token(&app, &code).await;

        let app_create = app.clone();
        let token_create = token.clone();
        let create = tokio::spawn(async move {
            authed_json(
                &app_create,
                "POST",
                "/v1/leases",
                &token_create,
                Some(sample_lease()),
            )
            .await
        });
        started_rx.await.unwrap();
        let (status, json) = authed_json(&app, "POST", "/v1/node/unpark", &token, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["parked"], false);
        resume_tx.send(()).unwrap();
        let (status, json) = create.await.unwrap();
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(json["error"], "node is unparked");
        assert!(state.inner.db.list_leases().unwrap().is_empty());
        assert!(state.inner.live.lock().await.is_empty());
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
        assert_eq!(json["end_reason"], "graceful");
        assert_eq!(json["forfeited"], false);
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
        assert_eq!(json["end_reason"], "graceful");
        assert_eq!(json["forfeited"], false);
    }

    #[tokio::test]
    async fn advertised_ws_url_uses_public_origin() {
        let (_dir, state) = test_state();
        state.set_origin("https://random-words-here.trycloudflare.com".into());
        let code = state.pairing_code().unwrap();
        let app = router(state);
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
        let session_id = lease["session_id"].as_str().unwrap();
        assert_eq!(
            lease["ws_url"],
            format!("wss://random-words-here.trycloudflare.com/v1/sessions/{session_id}")
        );
        assert!(!lease["ws_url"].as_str().unwrap().contains("127.0.0.1"));
        assert!(!lease["ws_url"].as_str().unwrap().contains('?'));
        assert!(!lease["ws_url"].as_str().unwrap().contains("token"));
        assert_eq!(lease["viewer_url"], "http://127.0.0.1:6080/vnc.html");
    }

    #[tokio::test]
    async fn get_lease_rewrites_stored_loopback_ws_when_origin_set() {
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
        let session_id = lease["session_id"].as_str().unwrap().to_string();
        assert!(
            lease["ws_url"]
                .as_str()
                .unwrap()
                .starts_with("ws://127.0.0.1")
        );
        state.set_origin("https://older-node.trycloudflare.com".into());
        let lease_id = lease["lease_id"].as_str().unwrap();
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
        let json = body_json(got).await;
        assert_eq!(
            json["ws_url"],
            format!("wss://older-node.trycloudflare.com/v1/sessions/{session_id}")
        );
    }

    #[test]
    fn session_ws_url_https_to_wss() {
        let bind: SocketAddr = "127.0.0.1:7432".parse().unwrap();
        assert_eq!(
            session_ws_url(bind, Some("https://n.example"), "s_1"),
            "wss://n.example/v1/sessions/s_1"
        );
        assert_eq!(
            session_ws_url(bind, None, "s_1"),
            "ws://127.0.0.1:7432/v1/sessions/s_1"
        );
    }

    #[tokio::test]
    async fn serve_tunnel_rejects_non_loopback_bind() {
        let err = serve("0.0.0.0:0".parse().unwrap(), Some(TunnelKind::Cloudflare))
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("loopback"), "{msg}");
    }

    #[tokio::test]
    async fn get_leases_require_bearer() {
        let (_dir, state) = test_state();
        let app = router(state);
        let list = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/leases")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(list.status(), StatusCode::UNAUTHORIZED);
        let one = app
            .oneshot(
                Request::builder()
                    .uri("/v1/leases/l_missing")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(one.status(), StatusCode::UNAUTHORIZED);
    }

    async fn create_sample_lease(app: &Router, token: &str) -> serde_json::Value {
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
        body_json(created).await
    }

    #[tokio::test]
    async fn list_leases_empty_one_stopped() {
        let (_dir, state) = test_state();
        let code = state.pairing_code().unwrap();
        let app = router(state);
        let token = pair_token(&app, &code).await;

        let empty = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/leases")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(empty.status(), StatusCode::OK);
        let json = body_json(empty).await;
        assert_eq!(json["leases"].as_array().unwrap().len(), 0);
        assert_eq!(json["truncated"], false);

        let lease = create_sample_lease(&app, &token).await;
        let lease_id = lease["lease_id"].as_str().unwrap();
        assert_eq!(lease["live"], true);
        assert!(lease["started_at"].as_i64().unwrap() > 0);

        let one = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/leases")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(one).await;
        assert_eq!(json["leases"].as_array().unwrap().len(), 1);
        assert_eq!(json["truncated"], false);
        assert_eq!(json["leases"][0]["lease_id"], lease_id);
        assert_eq!(json["leases"][0]["live"], true);
        assert!(
            json["leases"][0].get("stopped_at").is_none()
                || json["leases"][0]["stopped_at"].is_null()
        );

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

        let stopped = app
            .oneshot(
                Request::builder()
                    .uri("/v1/leases")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(stopped).await;
        assert_eq!(json["leases"].as_array().unwrap().len(), 1);
        assert_eq!(json["leases"][0]["status"], "stopped");
        assert_eq!(json["leases"][0]["live"], false);
        assert!(json["leases"][0]["stopped_at"].as_i64().unwrap() > 0);
        assert_eq!(json["truncated"], false);
    }

    #[test]
    fn access_log_format_has_no_secrets() {
        let line = format_access_log("GET", "/v1/pairing", 200, 3);
        assert_eq!(line, "GET /v1/pairing 200 3ms");
        assert!(!line.contains("ABCD"));
        assert!(!line.contains("brt_"));
        assert_eq!(format_access_log("GET", "/", 200, 4), "GET / 200 4ms");
    }

    async fn body_text(res: Response) -> String {
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    fn html_asset_paths(html: &str) -> Vec<String> {
        let mut out = Vec::new();
        for attr in ["src=\"", "href=\""] {
            let mut rest = html;
            while let Some(i) = rest.find(attr) {
                rest = &rest[i + attr.len()..];
                if let Some(end) = rest.find('"') {
                    let p = &rest[..end];
                    if p.starts_with("/assets/") {
                        out.push(p.to_string());
                    }
                    rest = &rest[end + 1..];
                }
            }
        }
        out
    }

    #[tokio::test]
    async fn console_root_ok_no_secrets_loopback_csp() {
        let (_dir, state) = test_state();
        let code = state.pairing_code().unwrap();
        let app = router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header("host", "127.0.0.1:7432")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers()
                .get(CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("no-cache")
        );
        let ctype = res
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(ctype.contains("text/html"), "{ctype}");
        let csp = res
            .headers()
            .get("content-security-policy")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(
            csp.contains("frame-src http://127.0.0.1:* http://localhost:* http://[::1]:*"),
            "{csp}"
        );
        assert!(csp.contains("default-src 'self'"), "{csp}");
        assert!(csp.contains("script-src 'self'"), "{csp}");
        assert!(csp.contains("style-src 'self'"), "{csp}");
        assert!(!csp.contains("unsafe-eval"), "{csp}");
        assert!(!csp.contains("unsafe-inline"), "{csp}");
        let body = body_text(res).await;
        assert!(!body.contains(&code), "{body}");
        assert!(!body.contains("brt_"), "{body}");
        assert!(
            !body
                .to_ascii_lowercase()
                .contains("content-security-policy"),
            "{body}"
        );
        assert!(!body.contains("?code="), "{body}");
    }

    #[tokio::test]
    async fn console_csp_trycloudflare_frames_none() {
        let (_dir, state) = test_state();
        let app = router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header("host", "random-words-here.trycloudflare.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let csp = res
            .headers()
            .get("content-security-policy")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(csp.contains("frame-src 'none'"), "{csp}");
        assert!(!csp.contains("127.0.0.1:*"), "{csp}");
        let body = body_text(res).await;
        assert!(
            !body
                .to_ascii_lowercase()
                .contains("content-security-policy"),
            "{body}"
        );
    }

    #[tokio::test]
    async fn console_client_route_and_hashed_assets() {
        let (_dir, state) = test_state();
        let app = router(state);
        let index = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/pair")
                    .header("host", "127.0.0.1:7432")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(index.status(), StatusCode::OK);
        let html = body_text(index).await;
        for path in html_asset_paths(&html) {
            let asset = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(&path)
                        .header("host", "127.0.0.1:7432")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(asset.status(), StatusCode::OK, "{path}");
            let cache = asset
                .headers()
                .get(CACHE_CONTROL)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            assert!(cache.contains("immutable"), "{path} {cache}");
        }
    }

    #[test]
    fn host_is_loopback_strips_port() {
        assert!(host_is_loopback("127.0.0.1:7432"));
        assert!(host_is_loopback("localhost:7432"));
        assert!(host_is_loopback("[::1]:7432"));
        assert!(host_is_loopback("::1"));
        assert!(!host_is_loopback("random-words-here.trycloudflare.com"));
    }

    #[tokio::test]
    async fn pairing_loopback_operator_table() {
        struct Case {
            name: &'static str,
            host: &'static str,
            extra: &'static [(&'static str, &'static str)],
            origin: Option<&'static str>,
            ok: bool,
        }
        let cases = [
            Case {
                name: "loopback ipv4",
                host: "127.0.0.1:7432",
                extra: &[],
                origin: None,
                ok: true,
            },
            Case {
                name: "localhost",
                host: "localhost:7432",
                extra: &[],
                origin: None,
                ok: true,
            },
            Case {
                name: "ipv6 loopback",
                host: "[::1]:7432",
                extra: &[],
                origin: None,
                ok: true,
            },
            Case {
                name: "cf-ray",
                host: "127.0.0.1:7432",
                extra: &[("Cf-Ray", "1")],
                origin: None,
                ok: false,
            },
            Case {
                name: "cf-connecting-ip",
                host: "127.0.0.1:7432",
                extra: &[("CF-Connecting-IP", "1.2.3.4")],
                origin: None,
                ok: false,
            },
            Case {
                name: "cdn-loop",
                host: "127.0.0.1:7432",
                extra: &[("CDN-Loop", "cloudflare")],
                origin: None,
                ok: false,
            },
            Case {
                name: "tunnel host with origin",
                host: "random-words-here.trycloudflare.com",
                extra: &[],
                origin: Some("https://random-words-here.trycloudflare.com"),
                ok: false,
            },
            Case {
                name: "loopback with tunnel origin",
                host: "127.0.0.1:7432",
                extra: &[],
                origin: Some("https://random-words-here.trycloudflare.com"),
                ok: true,
            },
            Case {
                name: "tunnel host without origin",
                host: "random-words-here.trycloudflare.com",
                extra: &[],
                origin: None,
                ok: false,
            },
        ];
        for case in cases {
            let (_dir, state) = test_state();
            if let Some(origin) = case.origin {
                state.set_origin(origin.to_string());
            }
            let code = state.pairing_code().unwrap();
            let app = router(state);
            let mut builder = Request::builder()
                .uri("/v1/pairing")
                .header("host", case.host);
            for (k, v) in case.extra {
                builder = builder.header(*k, *v);
            }
            let res = app
                .oneshot(builder.body(Body::empty()).unwrap())
                .await
                .unwrap();
            if case.ok {
                assert_eq!(res.status(), StatusCode::OK, "{}", case.name);
                let json = body_json(res).await;
                assert_eq!(json["code"], code, "{}", case.name);
            } else {
                assert_eq!(res.status(), StatusCode::NOT_FOUND, "{}", case.name);
                let json = body_json(res).await;
                assert_eq!(json["error"], "not found", "{}", case.name);
                assert!(!json.to_string().contains(&code), "{}", case.name);
            }
        }
    }

    #[tokio::test]
    async fn node_requires_bearer_and_healthy_stub_has_ok() {
        let (_dir, state) = test_state();
        let code = state.pairing_code().unwrap();
        let app = router(state);
        let unauth = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/node")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);

        let token = pair_token(&app, &code).await;
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/v1/node")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let body = String::from_utf8_lossy(&bytes);
        assert!(!body.contains("brt_"), "{body}");
        assert!(!body.contains(&code), "{body}");
        assert!(!body.contains("TUNNEL_TOKEN"), "{body}");
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["ok"], true);
        assert_eq!(json["bind"], "127.0.0.1:7432");
        assert!(json["origin"].is_null());
        assert_eq!(json["class"], "private");
        assert_eq!(json["image"], image_from_env());
        assert_eq!(json["docker"]["ok"], true);
        assert_eq!(json["docker"]["detail"], "bollard ping ok");
        assert_eq!(json["guest_image"]["ok"], true);
        assert_eq!(json["guest_image"]["name"], image_from_env());
        assert_eq!(json["home_writable"], true);
        assert_eq!(json["tunnel"]["kind"], "none");
        assert_eq!(json["active_bearers"], 1);
        assert_eq!(json["live_sessions"], 0);
        assert_eq!(json["shutting_down"], false);
        assert_eq!(json["host_desktop_driven"], false);
        assert_eq!(json["parked"], true);
        let src = json["allowlist_source"].as_str().unwrap();
        assert!(matches!(src, "default" | "env" | "deny-all"), "{src}");
        assert!(json["allowlist"].is_array());
    }

    #[tokio::test]
    async fn session_preview_png_204_and_404() {
        let (_dir, state) = test_state();
        let code = state.pairing_code().unwrap();
        let app = router(state.clone());
        let token = pair_token(&app, &code).await;
        let lease = create_sample_lease(&app, &token).await;
        let session_id = lease["session_id"].as_str().unwrap().to_string();
        let lease_id = lease["lease_id"].as_str().unwrap().to_string();

        let empty = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/sessions/{session_id}/preview"))
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(empty.status(), StatusCode::NO_CONTENT);

        let batch = ActionBatch {
            kind: ActionBatchKind::Actions,
            id: "a_preview".into(),
            session_id: session_id.clone(),
            items: vec![Action::Screenshot {}],
        };
        run_session_batch(&state, &session_id, batch).await.unwrap();

        let png = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/sessions/{session_id}/preview"))
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(png.status(), StatusCode::OK);
        assert_eq!(png.headers().get(CONTENT_TYPE).unwrap(), "image/png");
        assert_eq!(
            png.headers().get(CACHE_CONTROL).unwrap(),
            "private, no-store"
        );
        assert_eq!(
            png.headers().get("x-content-type-options").unwrap(),
            "nosniff"
        );
        let bytes = png.into_body().collect().await.unwrap().to_bytes();
        assert!(bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]));

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

        let gone = app
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/sessions/{session_id}/preview"))
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(gone.status(), StatusCode::NOT_FOUND);
        let json = body_json(gone).await;
        assert_eq!(json["error"], "not found");
    }

    async fn authed_json(
        app: &Router,
        method: &str,
        uri: &str,
        token: &str,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, serde_json::Value) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("authorization", format!("Bearer {token}"));
        let req_body = match body {
            Some(v) => {
                builder = builder.header("content-type", "application/json");
                Body::from(v.to_string())
            }
            None => Body::empty(),
        };
        let res = app
            .clone()
            .oneshot(builder.body(req_body).unwrap())
            .await
            .unwrap();
        let status = res.status();
        (status, body_json(res).await)
    }

    #[tokio::test]
    async fn park_unpark_quote_force_require_bearer() {
        let (_dir, state) = test_state();
        let app = router(state);
        for (method, uri) in [
            ("POST", "/v1/node/park"),
            ("POST", "/v1/node/unpark"),
            ("POST", "/v1/quote"),
            ("POST", "/v1/leases/l_missing/force"),
        ] {
            let res = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .header("content-type", "application/json")
                        .body(Body::from("{}"))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{method} {uri}");
        }
    }

    #[tokio::test]
    async fn unpark_blocked_while_live_then_ok_after_end() {
        let (_dir, state) = test_state();
        let code = state.pairing_code().unwrap();
        let app = router(state.clone());
        let token = pair_token(&app, &code).await;

        let lease = create_sample_lease(&app, &token).await;
        let lease_id = lease["lease_id"].as_str().unwrap().to_string();

        let (status, json) = authed_json(&app, "POST", "/v1/node/unpark", &token, None).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(json["error"], "cannot unpark while a lease is live");
        assert_eq!(json["live_lease_id"], lease_id);

        let (status, json) = authed_json(&app, "POST", "/v1/node/park", &token, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["parked"], true);
        assert_eq!(json["live_sessions"], 1);

        let (status, stopped) = authed_json(
            &app,
            "DELETE",
            &format!("/v1/leases/{lease_id}"),
            &token,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(stopped["end_reason"], "graceful");
        assert_eq!(stopped["forfeited"], false);
        assert_eq!(stopped["billable_seconds"], 60);

        let (status, json) = authed_json(&app, "POST", "/v1/node/unpark", &token, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["parked"], false);
        assert_eq!(json["live_sessions"], 0);

        let before = state.inner.db.list_leases().unwrap().len();
        let (status, json) =
            authed_json(&app, "POST", "/v1/leases", &token, Some(sample_lease())).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(json["error"], "node is unparked");
        assert_eq!(state.inner.db.list_leases().unwrap().len(), before);
        assert!(state.inner.live.lock().await.is_empty());

        let (status, json) = authed_json(&app, "POST", "/v1/node/unpark", &token, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["parked"], false);

        let (status, json) = authed_json(&app, "POST", "/v1/node/park", &token, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["parked"], true);
        let created = create_sample_lease(&app, &token).await;
        assert_eq!(created["live"], true);
        assert_eq!(created["forfeited"], false);
    }

    #[tokio::test]
    async fn stale_sqlite_active_does_not_block_unpark() {
        let (_dir, state) = test_state();
        let code = state.pairing_code().unwrap();
        let app = router(state.clone());
        let token = pair_token(&app, &code).await;
        let _lease = create_sample_lease(&app, &token).await;
        state.inner.live.lock().await.clear();
        let (status, json) = authed_json(&app, "POST", "/v1/node/unpark", &token, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["parked"], false);
        assert_eq!(json["live_sessions"], 0);
        assert_eq!(state.inner.db.active_leases().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn force_forfeits_keeps_billable_noop_if_stopped() {
        let (_dir, state) = test_state();
        let code = state.pairing_code().unwrap();
        let app = router(state.clone());
        let token = pair_token(&app, &code).await;

        let missing = authed_json(&app, "POST", "/v1/leases/l_missing/force", &token, None).await;
        assert_eq!(missing.0, StatusCode::NOT_FOUND);

        let lease = create_sample_lease(&app, &token).await;
        let lease_id = lease["lease_id"].as_str().unwrap().to_string();
        let (status, forced) = authed_json(
            &app,
            "POST",
            &format!("/v1/leases/{lease_id}/force"),
            &token,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(forced["status"], "stopped");
        assert_eq!(forced["billable_seconds"], 60);
        assert_eq!(forced["end_reason"], "forced");
        assert_eq!(forced["forfeited"], true);
        assert_eq!(forced["live"], false);
        assert!(state.inner.live.lock().await.is_empty());

        let (status, again) = authed_json(
            &app,
            "POST",
            &format!("/v1/leases/{lease_id}/force"),
            &token,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(again["end_reason"], "forced");
        assert_eq!(again["forfeited"], true);
        assert_eq!(again["billable_seconds"], 60);

        let graceful = create_sample_lease(&app, &token).await;
        let gid = graceful["lease_id"].as_str().unwrap().to_string();
        let (status, deleted) =
            authed_json(&app, "DELETE", &format!("/v1/leases/{gid}"), &token, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(deleted["end_reason"], "graceful");
        assert_eq!(deleted["forfeited"], false);
        assert_eq!(deleted["billable_seconds"], 60);

        let (status, forced_after) = authed_json(
            &app,
            "POST",
            &format!("/v1/leases/{gid}/force"),
            &token,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(forced_after["end_reason"], "graceful");
        assert_eq!(forced_after["forfeited"], false);
    }

    #[tokio::test]
    async fn quote_does_not_start_guest_and_works_unparked() {
        let (_dir, state) = test_state();
        let code = state.pairing_code().unwrap();
        let app = router(state.clone());
        let token = pair_token(&app, &code).await;

        let lease = create_sample_lease(&app, &token).await;
        assert_eq!(state.inner.live.lock().await.len(), 1);
        let before = state.inner.db.list_leases().unwrap().len();

        let (status, quote) =
            authed_json(&app, "POST", "/v1/quote", &token, Some(sample_lease())).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(quote["min_seconds"], 60);
        assert_eq!(quote["os"], "linux");
        assert_eq!(quote["density_mult"], 1.0);
        assert!(quote.get("lease_id").is_none());
        assert_eq!(state.inner.live.lock().await.len(), 1);
        assert_eq!(state.inner.db.list_leases().unwrap().len(), before);
        assert_eq!(lease["quote"]["gas_per_second"], quote["gas_per_second"]);

        let lease_id = lease["lease_id"].as_str().unwrap();
        let (status, _) = authed_json(
            &app,
            "DELETE",
            &format!("/v1/leases/{lease_id}"),
            &token,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, json) = authed_json(&app, "POST", "/v1/node/unpark", &token, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["parked"], false);

        let live_before = state.inner.live.lock().await.len();
        let (status, quote) =
            authed_json(&app, "POST", "/v1/quote", &token, Some(sample_lease())).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(quote["min_seconds"], 60);
        assert_eq!(state.inner.live.lock().await.len(), live_before);
        assert!(state.inner.db.active_leases().unwrap().is_empty());

        let windows = serde_json::json!({
            "os": "windows",
            "class": "private",
            "license": "w365-agents",
            "density": "isolated",
            "term": "on_demand",
            "resources": { "vcpu": 2, "mem_gib": 4, "disk_gib": 40 }
        });
        let (status, json) = authed_json(&app, "POST", "/v1/quote", &token, Some(windows)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let err = json["error"].as_str().unwrap();
        assert!(err.contains("Windows"), "{err}");
    }
}

//! SHPH Queue: a small queue-calling server for a Thai sub-district health center.

mod api;
mod app;
mod db;
mod model;
mod queue;
mod thai;

use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path as UrlPath, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use rust_embed::Embed;
use serde::Deserialize;
use serde_json::json;
use tower_http::services::ServeDir;

use app::{now, today, App, Client, Paths, Shared};

#[derive(Embed)]
#[folder = "web/"]
struct Web;

fn asset(path: &str) -> Response {
    match Web::get(path) {
        Some(file) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                [(header::CONTENT_TYPE, mime.as_ref().to_string()), (header::CACHE_CONTROL, "no-cache".to_string())],
                file.data.into_owned(),
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "ไม่พบไฟล์").into_response(),
    }
}

async fn page_index() -> Response {
    asset("index.html")
}
async fn page_display() -> Response {
    asset("display.html")
}
async fn page_room() -> Response {
    asset("room.html")
}
async fn page_room_id(UrlPath(_id): UrlPath<String>) -> Response {
    asset("room.html")
}
async fn page_manage() -> Response {
    asset("manage.html")
}
async fn page_admin() -> Response {
    asset("admin.html")
}
async fn favicon() -> Response {
    asset("favicon.svg")
}
async fn static_asset(UrlPath(path): UrlPath<String>) -> Response {
    asset(&path)
}

#[derive(Deserialize)]
struct WsQuery {
    role: Option<String>,
    room: Option<u32>,
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(app): State<Shared>,
    Query(q): Query<WsQuery>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    ws.on_upgrade(move |socket| ws_session(socket, app, q, addr))
}

async fn ws_session(socket: WebSocket, app: Shared, q: WsQuery, addr: SocketAddr) {
    let id = app.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let role = q.role.unwrap_or_else(|| "other".into());
    app.clients.lock().unwrap().insert(
        id,
        Client { id, role: role.chars().take(20).collect(), room: q.room, ip: addr.ip().to_string(), since: now() },
    );
    let mut rx = app.tx.subscribe();
    let (mut sink, mut stream) = socket.split();
    let hello = {
        let inner = app.lock();
        json!({ "t": "state", "state": app.state_json(&inner) }).to_string()
    };
    if sink.send(Message::Text(hello)).await.is_ok() {
        let mut ping = tokio::time::interval(Duration::from_secs(25));
        loop {
            tokio::select! {
                msg = rx.recv() => match msg {
                    Ok(text) => if sink.send(Message::Text(text)).await.is_err() { break },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let text = {
                            let inner = app.lock();
                            json!({ "t": "state", "state": app.state_json(&inner) }).to_string()
                        };
                        if sink.send(Message::Text(text)).await.is_err() { break }
                    }
                    Err(_) => break,
                },
                incoming = stream.next() => match incoming {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                },
                _ = ping.tick() => if sink.send(Message::Ping(Vec::new())).await.is_err() { break },
            }
        }
    }
    app.clients.lock().unwrap().remove(&id);
}

/// The LAN address other machines should use. No packet is sent.
pub fn local_ip() -> Option<IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("8.8.8.8:80").ok()?;
    s.local_addr().ok().map(|a| a.ip())
}

/// New day → fresh queue; old no-show numbers → removed.
async fn housekeeping(app: Shared) {
    let mut tick = tokio::time::interval(Duration::from_secs(20));
    loop {
        tick.tick().await;
        let changed = {
            let mut guard = app.lock();
            let inner = &mut *guard;
            let ts = now();
            let day = today();
            let mut out = None;
            if inner.config.auto_reset_daily && inner.queue.core.day != day {
                out = Some(inner.queue.reset(day));
            } else if let Some(o) = inner.queue.expire_held(ts, inner.config.held_expire_minutes) {
                out = Some(o);
            }
            if let Some(o) = &out {
                app.persist(inner, o, ts);
            }
            out.is_some()
        };
        if changed {
            app.broadcast_state();
        }
    }
}

struct Args {
    port: u16,
    base: Option<PathBuf>,
}

fn parse_args() -> Args {
    let mut args = Args { port: 8000, base: None };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--port" | "-p" => {
                if let Some(p) = it.next().and_then(|v| v.parse().ok()) {
                    args.port = p;
                }
            }
            "--dir" => args.base = it.next().map(PathBuf::from),
            "--help" | "-h" => {
                println!("shph-queue [--port 8000] [--dir โฟลเดอร์เก็บข้อมูล]");
                std::process::exit(0);
            }
            _ => {}
        }
    }
    args
}

#[tokio::main]
async fn main() {
    let args = parse_args();
    let base = args.base.unwrap_or_else(|| {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."))
    });
    let paths = Paths { data: base.join("data"), media: base.join("data").join("media"), voice: base.join("voice") };
    for d in [&paths.data, &paths.media, &paths.voice] {
        if let Err(e) = std::fs::create_dir_all(d) {
            eprintln!("สร้างโฟลเดอร์ {} ไม่ได้: {e}", d.display());
        }
    }
    let media_dir = paths.media.clone();
    let voice_dir = paths.voice.clone();
    let app = App::new(paths, args.port);

    tokio::spawn(housekeeping(app.clone()));

    let upload_limit = DefaultBodyLimit::max(2 * 1024 * 1024 * 1024);
    let api = Router::new()
        .route("/state", get(api::state))
        .route("/info", get(api::info))
        .route("/log", get(api::log))
        .route("/rooms/:id/next", post(api::room_next))
        .route("/rooms/:id/repeat", post(api::room_repeat))
        .route("/rooms/:id/skip", post(api::room_skip))
        .route("/rooms/:id/done", post(api::room_done))
        .route("/rooms/:id/call", post(api::room_call))
        .route("/rooms/:id/undo", post(api::room_undo))
        .route("/held/:n/remove", post(api::held_remove))
        .route("/pharmacy/:n/remove", post(api::waiting_remove))
        .route("/queue/last", post(api::queue_set_last))
        .route("/queue/reset", post(api::queue_reset))
        .route("/admin/login", post(api::admin_login))
        .route("/admin/logout", post(api::admin_logout))
        .route("/admin/session", get(api::admin_session))
        .route("/admin/password", post(api::admin_password))
        .route("/admin/settings", get(api::admin_get))
        .route("/admin/config", put(api::admin_put_config))
        .route("/admin/rooms", post(api::admin_room_add))
        .route("/admin/rooms/:id", put(api::admin_room_update).delete(api::admin_room_delete))
        .route("/admin/rooms/:id/move", post(api::admin_room_move))
        .route("/admin/test-call", post(api::admin_test_call))
        .route("/admin/stats", get(api::admin_stats))
        .route("/admin/clients", get(api::admin_clients))
        .route("/admin/backup", get(api::admin_backup))
        .route("/admin/media", post(api::admin_upload_media).layer(upload_limit))
        .route("/admin/media/:name", delete(api::admin_delete_media))
        .route("/admin/voice", get(api::admin_voice_list).post(api::admin_voice_upload).layer(upload_limit))
        .route("/admin/voice/:name", delete(api::admin_voice_delete));

    let router = Router::new()
        .route("/", get(page_index))
        .route("/display", get(page_display))
        .route("/room", get(page_room))
        .route("/room/:id", get(page_room_id))
        .route("/manage", get(page_manage))
        .route("/admin", get(page_admin))
        .route("/favicon.ico", get(favicon))
        .route("/assets/*path", get(static_asset))
        .route("/ws", get(ws_handler))
        .nest("/api", api)
        .nest_service("/media", ServeDir::new(media_dir))
        .nest_service("/voice", ServeDir::new(voice_dir))
        .with_state(app.clone());

    let addr = SocketAddr::from(([0, 0, 0, 0], args.port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("เปิดพอร์ต {} ไม่ได้: {e}\nอาจมีโปรแกรมนี้เปิดอยู่แล้ว หรือใช้ --port เลขอื่น", args.port);
            wait_before_exit();
            return;
        }
    };

    let host = local_ip().map(|i| i.to_string()).unwrap_or_else(|| "localhost".into());
    let url = format!("http://{host}:{}", args.port);
    println!("============================================");
    println!(" SHPH Queue {} — ระบบเรียกคิว รพ.สต.", env!("CARGO_PKG_VERSION"));
    println!("============================================");
    println!(" หน้าแรก        {url}/");
    println!(" จอแสดงผล      {url}/display");
    println!(" ห้องตรวจ       {url}/room/1");
    println!(" จัดการคิว      {url}/manage");
    println!(" ผู้ดูแลระบบ    {url}/admin");
    println!();
    println!(" ข้อมูลเก็บที่: {}", base.join("data").display());
    println!(" ปิดหน้าต่างนี้ = ปิดระบบคิว");
    println!("============================================");

    let server = axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        });
    if let Err(e) = server.await {
        eprintln!("เซิร์ฟเวอร์หยุดทำงาน: {e}");
        wait_before_exit();
    }
}

/// Keep the console window open long enough to read an error on Windows.
fn wait_before_exit() {
    std::thread::sleep(Duration::from_secs(15));
}

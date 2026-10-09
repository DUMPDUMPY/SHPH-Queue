//! HTTP API, admin login, uploads.

use std::path::Path;

use axum::body::Body;
use axum::extract::{Multipart, Path as UrlPath, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use rand::Rng;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::app::{now, today, Shared};
use crate::model::{Config, Room, RoomKind};
use crate::thai;

pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "ok": false, "error": self.1 }))).into_response()
    }
}

impl From<String> for ApiError {
    fn from(s: String) -> Self {
        ApiError(StatusCode::BAD_REQUEST, s)
    }
}

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

type ApiResult = Result<Json<Value>, ApiError>;

// ---------------------------------------------------------------- public

pub async fn state(State(app): State<Shared>) -> Json<Value> {
    let inner = app.lock();
    Json(app.state_json(&inner))
}

pub async fn info(State(app): State<Shared>) -> Json<Value> {
    let ip = crate::local_ip().map(|i| i.to_string()).unwrap_or_else(|| "localhost".into());
    Json(
        json!({ "base": format!("http://{ip}:{}", app.port), "ip": ip, "port": app.port, "version": env!("CARGO_PKG_VERSION") }),
    )
}

pub async fn room_next(State(app): State<Shared>, UrlPath(id): UrlPath<u32>) -> ApiResult {
    Ok(Json(app.apply(|q, r, t| q.call_next(id, r, t))?))
}

pub async fn room_repeat(State(app): State<Shared>, UrlPath(id): UrlPath<u32>) -> ApiResult {
    Ok(Json(app.apply(|q, r, _| q.repeat(id, r))?))
}

pub async fn room_skip(State(app): State<Shared>, UrlPath(id): UrlPath<u32>) -> ApiResult {
    Ok(Json(app.apply(|q, r, t| q.skip(id, r, t))?))
}

pub async fn room_undo(State(app): State<Shared>, UrlPath(id): UrlPath<u32>) -> ApiResult {
    Ok(Json(app.apply(|q, r, t| q.undo(id, r, t))?))
}

#[derive(Deserialize)]
pub struct DoneBody {
    #[serde(default)]
    pharmacy: bool,
}

pub async fn room_done(State(app): State<Shared>, UrlPath(id): UrlPath<u32>, Json(b): Json<DoneBody>) -> ApiResult {
    Ok(Json(app.apply(|q, r, t| q.finish(id, b.pharmacy, r, t))?))
}

#[derive(Deserialize)]
pub struct NumberBody {
    number: u32,
}

pub async fn room_call(State(app): State<Shared>, UrlPath(id): UrlPath<u32>, Json(b): Json<NumberBody>) -> ApiResult {
    Ok(Json(app.apply(|q, r, t| q.call_number(id, b.number, r, t))?))
}

pub async fn held_remove(State(app): State<Shared>, UrlPath(n): UrlPath<u32>) -> ApiResult {
    Ok(Json(app.apply(|q, _, _| q.remove_held(n))?))
}

pub async fn waiting_remove(State(app): State<Shared>, UrlPath(n): UrlPath<u32>) -> ApiResult {
    Ok(Json(app.apply(|q, _, _| q.remove_waiting(n))?))
}

#[derive(Deserialize)]
pub struct LastBody {
    last: u32,
}

pub async fn queue_set_last(State(app): State<Shared>, Json(b): Json<LastBody>) -> ApiResult {
    Ok(Json(app.apply(|q, r, _| q.set_last(b.last, r))?))
}

pub async fn queue_reset(State(app): State<Shared>) -> ApiResult {
    Ok(Json(app.apply(|q, _, _| Ok(q.reset(today())))?))
}

#[derive(Deserialize)]
pub struct LogQuery {
    limit: Option<u32>,
}

pub async fn log(State(app): State<Shared>, Query(q): Query<LogQuery>) -> Json<Value> {
    let inner = app.lock();
    let day = inner.queue.core.day.clone();
    Json(json!({ "events": inner.db.recent_events(&day, q.limit.unwrap_or(100).min(1000)) }))
}

// ---------------------------------------------------------------- admin auth

const COOKIE: &str = "shph_admin";
const DEFAULT_PASSWORD: &str = "admin";

#[derive(serde::Serialize, Deserialize)]
struct Secret {
    salt: String,
    hash: String,
}

fn hash(salt: &str, pw: &str) -> String {
    let mut h = Sha256::new();
    h.update(salt.as_bytes());
    h.update(pw.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn random_hex(bytes: usize) -> String {
    let mut rng = rand::thread_rng();
    (0..bytes).map(|_| format!("{:02x}", rng.gen::<u8>())).collect()
}

fn check_password(app: &Shared, pw: &str) -> bool {
    let inner = app.lock();
    let secret: Secret = match inner.db.get("admin") {
        Some(s) => s,
        None => {
            let salt = random_hex(16);
            let s = Secret { hash: hash(&salt, DEFAULT_PASSWORD), salt };
            let _ = inner.db.set("admin", &s);
            s
        }
    };
    hash(&secret.salt, pw) == secret.hash
}

fn session_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == COOKIE)
        .map(|(_, v)| v.to_string())
}

fn require_admin(app: &Shared, headers: &HeaderMap) -> Result<(), ApiError> {
    let ok = session_token(headers).is_some_and(|t| app.sessions.lock().unwrap().contains(&t));
    if ok {
        Ok(())
    } else {
        Err(ApiError(StatusCode::UNAUTHORIZED, "กรุณาเข้าสู่ระบบ".into()))
    }
}

#[derive(Deserialize)]
pub struct LoginBody {
    password: String,
}

pub async fn admin_login(State(app): State<Shared>, Json(b): Json<LoginBody>) -> Result<Response, ApiError> {
    if !check_password(&app, &b.password) {
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        return Err(ApiError(StatusCode::UNAUTHORIZED, "รหัสผ่านไม่ถูกต้อง".into()));
    }
    let token = random_hex(24);
    app.sessions.lock().unwrap().insert(token.clone());
    let cookie = format!("{COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age=43200");
    Ok(([(header::SET_COOKIE, cookie)], Json(json!({ "ok": true }))).into_response())
}

pub async fn admin_logout(State(app): State<Shared>, headers: HeaderMap) -> Response {
    if let Some(t) = session_token(&headers) {
        app.sessions.lock().unwrap().remove(&t);
    }
    let cookie = format!("{COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0");
    ([(header::SET_COOKIE, cookie)], Json(json!({ "ok": true }))).into_response()
}

pub async fn admin_session(State(app): State<Shared>, headers: HeaderMap) -> Json<Value> {
    let logged_in = require_admin(&app, &headers).is_ok();
    let default_password = logged_in && check_password(&app, DEFAULT_PASSWORD);
    Json(json!({ "logged_in": logged_in, "default_password": default_password }))
}

#[derive(Deserialize)]
pub struct PasswordBody {
    current: String,
    new: String,
}

pub async fn admin_password(State(app): State<Shared>, headers: HeaderMap, Json(b): Json<PasswordBody>) -> ApiResult {
    require_admin(&app, &headers)?;
    if !check_password(&app, &b.current) {
        return Err(bad("รหัสผ่านเดิมไม่ถูกต้อง"));
    }
    if b.new.chars().count() < 4 {
        return Err(bad("รหัสผ่านใหม่ต้องยาวอย่างน้อย 4 ตัวอักษร"));
    }
    let salt = random_hex(16);
    let s = Secret { hash: hash(&salt, &b.new), salt };
    app.lock().db.set("admin", &s).map_err(|e| bad(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------- admin settings

fn admin_view(app: &Shared) -> Value {
    let inner = app.lock();
    let phrases: Vec<Value> = thai::PHRASES.iter().map(|(k, t)| json!({ "key": k, "text": t })).collect();
    json!({
        "config": inner.config,
        "rooms": inner.queue.core.rooms,
        "phrases": phrases,
    })
}

pub async fn admin_get(State(app): State<Shared>, headers: HeaderMap) -> ApiResult {
    require_admin(&app, &headers)?;
    Ok(Json(admin_view(&app)))
}

pub async fn admin_put_config(State(app): State<Shared>, headers: HeaderMap, Json(mut cfg): Json<Config>) -> ApiResult {
    require_admin(&app, &headers)?;
    cfg.sanitize();
    {
        let mut inner = app.lock();
        inner.db.set("config", &cfg).map_err(|e| bad(e.to_string()))?;
        inner.config = cfg;
    }
    app.broadcast_state();
    Ok(Json(admin_view(&app)))
}

#[derive(Deserialize)]
pub struct RoomBody {
    name: String,
    kind: RoomKind,
    #[serde(default = "yes")]
    enabled: bool,
    voice_phrase: String,
    voice_number: Option<u32>,
}

fn yes() -> bool {
    true
}

fn save_rooms(app: &Shared) {
    {
        let mut guard = app.lock();
        let inner = &mut *guard;
        let out = crate::queue::Outcome::default();
        app.persist(inner, &out, now());
    }
    app.broadcast_state();
}

fn check_room(b: &RoomBody) -> Result<(), ApiError> {
    if b.name.trim().is_empty() {
        return Err(bad("กรุณาใส่ชื่อห้อง"));
    }
    if b.voice_number.is_some_and(|n| n == 0 || n > 999) {
        return Err(bad("เลขห้องสำหรับเสียงต้องอยู่ระหว่าง 1–999"));
    }
    Ok(())
}

pub async fn admin_room_add(State(app): State<Shared>, headers: HeaderMap, Json(b): Json<RoomBody>) -> ApiResult {
    require_admin(&app, &headers)?;
    check_room(&b)?;
    app.lock().queue.add_room(Room {
        id: 0,
        name: b.name.trim().to_string(),
        kind: b.kind,
        enabled: b.enabled,
        voice_phrase: b.voice_phrase,
        voice_number: b.voice_number,
        current: None,
        called_at: None,
    });
    save_rooms(&app);
    Ok(Json(admin_view(&app)))
}

pub async fn admin_room_update(
    State(app): State<Shared>,
    headers: HeaderMap,
    UrlPath(id): UrlPath<u32>,
    Json(b): Json<RoomBody>,
) -> ApiResult {
    require_admin(&app, &headers)?;
    check_room(&b)?;
    app.lock().queue.update_room(id, b.name.trim().to_string(), b.kind, b.enabled, b.voice_phrase, b.voice_number)?;
    save_rooms(&app);
    Ok(Json(admin_view(&app)))
}

pub async fn admin_room_delete(State(app): State<Shared>, headers: HeaderMap, UrlPath(id): UrlPath<u32>) -> ApiResult {
    require_admin(&app, &headers)?;
    app.lock().queue.delete_room(id)?;
    save_rooms(&app);
    Ok(Json(admin_view(&app)))
}

#[derive(Deserialize)]
pub struct MoveBody {
    up: bool,
}

pub async fn admin_room_move(
    State(app): State<Shared>,
    headers: HeaderMap,
    UrlPath(id): UrlPath<u32>,
    Json(b): Json<MoveBody>,
) -> ApiResult {
    require_admin(&app, &headers)?;
    app.lock().queue.move_room(id, b.up)?;
    save_rooms(&app);
    Ok(Json(admin_view(&app)))
}

#[derive(Deserialize)]
pub struct TestCall {
    room_id: u32,
    number: Option<u32>,
}

pub async fn admin_test_call(State(app): State<Shared>, headers: HeaderMap, Json(b): Json<TestCall>) -> ApiResult {
    require_admin(&app, &headers)?;
    let msg = {
        let inner = app.lock();
        app.call_json(&inner, b.room_id, b.number.unwrap_or(12), true)
    };
    app.broadcast(msg.clone());
    Ok(Json(json!({ "ok": true, "call": msg })))
}

pub async fn admin_stats(State(app): State<Shared>, headers: HeaderMap) -> ApiResult {
    require_admin(&app, &headers)?;
    let inner = app.lock();
    let day = inner.queue.core.day.clone();
    Ok(Json(json!(inner.db.day_stats(&day))))
}

pub async fn admin_clients(State(app): State<Shared>, headers: HeaderMap) -> ApiResult {
    require_admin(&app, &headers)?;
    let mut list: Vec<_> = app.clients.lock().unwrap().values().cloned().collect();
    list.sort_by_key(|c| c.id);
    Ok(Json(json!({
        "clients": list,
        "data_dir": app.paths.data.to_string_lossy(),
        "voice_dir": app.paths.voice.to_string_lossy(),
    })))
}

pub async fn admin_backup(State(app): State<Shared>, headers: HeaderMap) -> Result<Response, ApiError> {
    require_admin(&app, &headers)?;
    let tmp = app.paths.data.join("backup-tmp.db");
    app.lock().db.backup_to(&tmp).map_err(|e| bad(format!("สำรองข้อมูลไม่สำเร็จ: {e}")))?;
    let bytes = tokio::fs::read(&tmp).await.map_err(|e| bad(e.to_string()))?;
    let _ = tokio::fs::remove_file(&tmp).await;
    let name = format!("shph-queue-backup-{}.db", chrono::Local::now().format("%Y%m%d-%H%M"));
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
        ],
        Body::from(bytes),
    )
        .into_response())
}

// ---------------------------------------------------------------- uploads

fn media_kind(ext: &str) -> Option<&'static str> {
    match ext {
        "jpg" | "jpeg" | "png" | "gif" | "webp" => Some("image"),
        "mp4" | "webm" | "m4v" | "ogv" => Some("video"),
        _ => None,
    }
}

fn extension(name: &str) -> String {
    Path::new(name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

async fn save_field(field: &mut axum::extract::multipart::Field<'_>, dest: &Path) -> Result<(), ApiError> {
    let mut f = tokio::fs::File::create(dest).await.map_err(|e| bad(format!("บันทึกไฟล์ไม่ได้: {e}")))?;
    while let Some(chunk) = field.chunk().await.map_err(|e| bad(format!("อัปโหลดไม่สำเร็จ: {e}")))?
    {
        f.write_all(&chunk).await.map_err(|e| bad(e.to_string()))?;
    }
    f.flush().await.map_err(|e| bad(e.to_string()))?;
    Ok(())
}

pub async fn admin_upload_media(State(app): State<Shared>, headers: HeaderMap, mut mp: Multipart) -> ApiResult {
    require_admin(&app, &headers)?;
    let mut saved = Vec::new();
    while let Some(mut field) = mp.next_field().await.map_err(|e| bad(e.to_string()))? {
        let original = field.file_name().unwrap_or("file").to_string();
        let ext = extension(&original);
        let Some(kind) = media_kind(&ext) else {
            return Err(bad(format!("ไฟล์ {original} ไม่รองรับ ใช้ได้: jpg, png, gif, webp, mp4, webm")));
        };
        let name = format!("{}_{}.{ext}", chrono::Local::now().format("%Y%m%d%H%M%S"), random_hex(3));
        let dest = app.paths.media.join(&name);
        if let Err(e) = save_field(&mut field, &dest).await {
            let _ = tokio::fs::remove_file(&dest).await;
            return Err(e);
        }
        let title = Path::new(&original).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        saved.push(json!({ "src": name, "kind": kind, "title": title }));
    }
    Ok(Json(json!({ "ok": true, "files": saved })))
}

pub async fn admin_delete_media(
    State(app): State<Shared>,
    headers: HeaderMap,
    UrlPath(name): UrlPath<String>,
) -> ApiResult {
    require_admin(&app, &headers)?;
    if name.contains(['/', '\\']) || name.starts_with('.') {
        return Err(bad("ชื่อไฟล์ไม่ถูกต้อง"));
    }
    let _ = tokio::fs::remove_file(app.paths.media.join(&name)).await;
    Ok(Json(json!({ "ok": true })))
}

fn valid_voice_name(app: &Shared, stem: &str) -> bool {
    app.voice_lines.contains_key(stem)
        || stem.strip_prefix("room_").is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

pub async fn admin_voice_list(State(app): State<Shared>, headers: HeaderMap) -> ApiResult {
    require_admin(&app, &headers)?;
    let lines: Vec<Value> = app
        .voice_lines
        .iter()
        .map(|(k, t)| json!({ "name": k, "text": t, "present": app.voice_file(k).is_some() }))
        .collect();
    let rooms: Vec<Value> = {
        let inner = app.lock();
        inner
            .queue
            .core
            .rooms
            .iter()
            .map(|r| {
                let name = format!("room_{}", r.id);
                json!({ "name": name, "room": r.name, "present": app.voice_file(&name).is_some() })
            })
            .collect()
    };
    Ok(Json(json!({ "lines": lines, "rooms": rooms })))
}

pub async fn admin_voice_upload(State(app): State<Shared>, headers: HeaderMap, mut mp: Multipart) -> ApiResult {
    require_admin(&app, &headers)?;
    let (mut ok, mut skipped) = (Vec::new(), Vec::new());
    while let Some(mut field) = mp.next_field().await.map_err(|e| bad(e.to_string()))? {
        let original = field.file_name().unwrap_or("").to_string();
        let ext = extension(&original);
        let stem = Path::new(&original).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if !["mp3", "wav", "ogg", "m4a"].contains(&ext.as_str()) || !valid_voice_name(&app, &stem) {
            skipped.push(original);
            continue;
        }
        for e in ["mp3", "wav", "ogg", "m4a"] {
            let _ = tokio::fs::remove_file(app.paths.voice.join(format!("{stem}.{e}"))).await;
        }
        save_field(&mut field, &app.paths.voice.join(format!("{stem}.{ext}"))).await?;
        ok.push(stem);
    }
    Ok(Json(json!({ "ok": true, "saved": ok, "skipped": skipped })))
}

pub async fn admin_voice_delete(
    State(app): State<Shared>,
    headers: HeaderMap,
    UrlPath(name): UrlPath<String>,
) -> ApiResult {
    require_admin(&app, &headers)?;
    if !valid_voice_name(&app, &name) {
        return Err(bad("ชื่อไฟล์เสียงไม่ถูกต้อง"));
    }
    for e in ["mp3", "wav", "ogg", "m4a"] {
        let _ = tokio::fs::remove_file(app.paths.voice.join(format!("{name}.{e}"))).await;
    }
    Ok(Json(json!({ "ok": true })))
}

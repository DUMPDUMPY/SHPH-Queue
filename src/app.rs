//! Shared application state: queue, settings, database, live connections.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use chrono::{Local, Timelike};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::broadcast;

use crate::db::Db;
use crate::model::{Config, Core, RoomKind};
use crate::queue::{Outcome, Queue, Rules};
use crate::thai;

pub struct Paths {
    pub data: PathBuf,
    pub media: PathBuf,
    pub voice: PathBuf,
}

pub struct Inner {
    pub db: Db,
    pub queue: Queue,
    pub config: Config,
}

#[derive(Clone, Serialize)]
pub struct Client {
    pub id: u64,
    pub role: String,
    pub room: Option<u32>,
    pub ip: String,
    pub since: i64,
}

pub struct App {
    pub inner: Mutex<Inner>,
    pub tx: broadcast::Sender<String>,
    pub paths: Paths,
    pub port: u16,
    pub sessions: Mutex<HashSet<String>>,
    pub clients: Mutex<HashMap<u64, Client>>,
    pub next_id: AtomicU64,
    /// Voice clip names known to the manifest (without extension).
    pub voice_lines: BTreeMap<String, String>,
}

pub type Shared = Arc<App>;

pub fn now() -> i64 {
    Local::now().timestamp()
}

pub fn today() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

pub const VOICE_MANIFEST: &str = include_str!("../voice/voice_lines.json");

impl App {
    pub fn new(paths: Paths, port: u16) -> Shared {
        let db = Db::open(&paths.data.join("queue.db")).expect("เปิดฐานข้อมูลไม่ได้");
        let mut config: Config = db.get("config").unwrap_or_default();
        config.sanitize();
        let core: Core = db.get("core").unwrap_or_else(|| Core::new(today()));
        let served: BTreeMap<u32, u32> = db.get("served").unwrap_or_default();
        let _ = db.set("config", &config);
        let (tx, _) = broadcast::channel(256);
        let voice_lines = serde_json::from_str(VOICE_MANIFEST).unwrap_or_default();
        Arc::new(App {
            inner: Mutex::new(Inner { db, queue: Queue::new(core, served), config }),
            tx,
            paths,
            port,
            sessions: Mutex::new(HashSet::new()),
            clients: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            voice_lines,
        })
    }

    pub fn lock(&self) -> MutexGuard<'_, Inner> {
        // A panic in a handler must not take the whole queue down with it.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn broadcast(&self, msg: Value) {
        let _ = self.tx.send(msg.to_string());
    }

    pub fn broadcast_state(&self) {
        let st = {
            let inner = self.lock();
            self.state_json(&inner)
        };
        self.broadcast(json!({ "t": "state", "state": st }));
    }

    /// Run a queue operation, persist it, and push the result to every screen.
    pub fn apply<F>(&self, f: F) -> Result<Value, String>
    where
        F: FnOnce(&mut Queue, &Rules, i64) -> Result<Outcome, String>,
    {
        let (call, state) = {
            let mut guard = self.lock();
            let inner = &mut *guard;
            let rules = rules(&inner.config);
            let ts = now();
            let out = f(&mut inner.queue, &rules, ts)?;
            self.persist(inner, &out, ts);
            let call = out.call.map(|(room, n)| self.call_json(inner, room, n, false));
            (call, self.state_json(inner))
        };
        self.broadcast(json!({ "t": "state", "state": state }));
        if let Some(c) = &call {
            self.broadcast(c.clone());
        }
        Ok(json!({ "ok": true, "state": state }))
    }

    pub fn persist(&self, inner: &mut Inner, out: &Outcome, ts: i64) {
        let rooms: HashMap<u32, String> = inner.queue.core.rooms.iter().map(|r| (r.id, r.name.clone())).collect();
        let local = Local::now();
        let day = inner.queue.core.day.clone();
        if let Err(e) =
            inner.db.save_queue(&inner.queue.core, &inner.queue.served, &out.log, ts, &day, local.hour(), |id| {
                rooms.get(&id).cloned().unwrap_or_else(|| {
                    if id == 0 {
                        "ระบบ".into()
                    } else {
                        format!("ห้อง {id}")
                    }
                })
            })
        {
            eprintln!("บันทึกข้อมูลไม่สำเร็จ: {e}");
        }
    }

    pub fn voice_file(&self, name: &str) -> Option<PathBuf> {
        ["mp3", "wav", "ogg", "m4a"]
            .iter()
            .map(|ext| self.paths.voice.join(format!("{name}.{ext}")))
            .find(|p| p.is_file())
    }

    fn voice_url(&self, name: &str) -> Option<String> {
        self.voice_file(name).map(|p| format!("/voice/{}", p.file_name().unwrap().to_string_lossy()))
    }

    /// The announcement for one call: clip URLs (empty when any is missing) and the spoken text.
    pub fn call_json(&self, inner: &Inner, room_id: u32, number: u32, test: bool) -> Value {
        let room = inner.queue.core.rooms.iter().find(|r| r.id == room_id);
        let (name, kind, phrase, vnum) = match room {
            Some(r) => (r.name.clone(), r.kind, r.voice_phrase.clone(), r.voice_number),
            None => (format!("ห้อง {room_id}"), RoomKind::Exam, "room".to_string(), None),
        };
        let mut text = format!("ขอเชิญหมายเลข {} {}", thai::number_words(number), thai::phrase_text(&phrase));
        if let Some(v) = vnum {
            text.push(' ');
            text.push_str(&thai::number_words(v));
        }
        text.push_str(" ค่ะ");

        let mut names = vec!["invite".to_string()];
        names.extend(thai::number_clips(number));
        if self.voice_file(&format!("room_{room_id}")).is_some() {
            names.push(format!("room_{room_id}"));
        } else {
            names.push(format!("phrase_{phrase}"));
            if let Some(v) = vnum {
                names.extend(thai::number_clips(v));
            }
        }
        names.push("end".into());
        let urls: Option<Vec<String>> = names.iter().map(|n| self.voice_url(n)).collect();

        json!({
            "t": "call",
            "id": self.next_id.fetch_add(1, Ordering::Relaxed),
            "room_id": room_id,
            "room_name": name,
            "kind": kind,
            "number": number,
            "text": text,
            "clips": urls.unwrap_or_default(),
            "test": test,
        })
    }

    pub fn state_json(&self, inner: &Inner) -> Value {
        let q = &inner.queue;
        let c = &inner.config;
        let r = rules(c);
        let mut upcoming = Vec::new();
        let mut last = q.core.last;
        for _ in 0..c.upcoming_count {
            last = if last < r.min || last >= r.max { r.min } else { last + 1 };
            upcoming.push(last);
        }
        let rooms: Vec<Value> = q
            .core
            .rooms
            .iter()
            .map(|room| {
                let active = room.enabled && (room.kind == RoomKind::Exam || c.pharmacy_enabled);
                json!({
                    "id": room.id,
                    "name": room.name,
                    "kind": room.kind,
                    "enabled": room.enabled,
                    "active": active,
                    "current": room.current,
                    "called_at": room.called_at,
                    "served": q.served.get(&room.id).copied().unwrap_or(0),
                    "can_undo": q.can_undo(room.id),
                })
            })
            .collect();
        json!({
            "day": q.core.day,
            "now": now(),
            "last": q.core.last,
            "next": q.next_number(&r),
            "upcoming": upcoming,
            "rooms": rooms,
            "held": q.core.held,
            "pharmacy": q.core.pharmacy,
            "config": public_config(c),
        })
    }
}

pub fn rules(c: &Config) -> Rules {
    Rules { min: c.number_min, max: c.number_max, pharmacy_enabled: c.pharmacy_enabled }
}

/// Settings every screen may see.
fn public_config(c: &Config) -> Value {
    json!({
        "org_prefix": c.org_prefix,
        "org_name": c.org_name,
        "number_min": c.number_min,
        "number_max": c.number_max,
        "pharmacy_enabled": c.pharmacy_enabled,
        "held_expire_minutes": c.held_expire_minutes,
        "sound": c.sound,
        "ticker": c.ticker,
        "callout_seconds": c.callout_seconds,
        "image_seconds": c.image_seconds,
        "media": c.media.iter().filter(|m| m.enabled).collect::<Vec<_>>(),
    })
}

//! End-to-end API tests: each test starts the real server binary with its own data folder and port.

use std::io::Read;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

struct Server {
    child: Option<Child>,
    dir: PathBuf,
    port: u16,
    cookie: Option<String>,
}

impl Server {
    fn start() -> Server {
        let dir = std::env::temp_dir().join(format!(
            "shph-queue-test-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Server::start_in(dir)
    }

    fn start_in(dir: PathBuf) -> Server {
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let child = Command::new(env!("CARGO_BIN_EXE_shph-queue"))
            .args(["--dir", dir.to_str().unwrap(), "--port", &port.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start server");
        let s = Server { child: Some(child), dir, port, cookie: None };
        let deadline = Instant::now() + Duration::from_secs(15);
        while ureq::get(&s.url("/")).call().is_err() {
            assert!(Instant::now() < deadline, "server did not start");
            std::thread::sleep(Duration::from_millis(50));
        }
        // Let the housekeeping task run its first tick (daily reset / held expiry).
        std::thread::sleep(Duration::from_millis(150));
        s
    }

    /// Stop the server and start it again on the same data folder.
    fn restart(mut self) -> Server {
        self.stop();
        let dir = std::mem::take(&mut self.dir);
        std::mem::forget(self);
        Server::start_in(dir)
    }

    fn stop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{}", self.port, path)
    }

    fn req(&self, method: &str, path: &str) -> ureq::Request {
        let r = ureq::request(method, &self.url(path));
        match &self.cookie {
            Some(c) => r.set("Cookie", c),
            None => r,
        }
    }

    /// Send a request; returns (status, JSON body).
    fn send(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let r = self.req(method, path);
        let res = match body {
            Some(b) => r.send_json(b),
            None => r.call(),
        };
        let resp = match res {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => panic!("{method} {path}: {e}"),
        };
        let status = resp.status();
        let text = resp.into_string().unwrap_or_default();
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    fn post(&self, path: &str, body: Value) -> Value {
        let (status, v) = self.send("POST", &format!("/api{path}"), Some(body));
        assert_eq!(status, 200, "POST {path} -> {v}");
        v
    }

    fn post_err(&self, path: &str, body: Value) -> (u16, String) {
        let (status, v) = self.send("POST", &format!("/api{path}"), Some(body));
        assert_ne!(status, 200, "POST {path} should fail");
        (status, v["error"].as_str().unwrap_or_default().to_string())
    }

    fn state(&self) -> Value {
        let (status, v) = self.send("GET", "/api/state", None);
        assert_eq!(status, 200);
        v
    }

    fn login(&mut self, password: &str) -> u16 {
        let res = ureq::post(&self.url("/api/admin/login")).send_json(json!({ "password": password }));
        match res {
            Ok(r) => {
                let c = r.header("set-cookie").unwrap().split(';').next().unwrap().to_string();
                self.cookie = Some(c);
                200
            }
            Err(ureq::Error::Status(code, _)) => code,
            Err(e) => panic!("login: {e}"),
        }
    }

    fn put_config(&self, f: impl FnOnce(&mut Value)) {
        let (status, mut v) = self.send("GET", "/api/admin/settings", None);
        assert_eq!(status, 200, "need admin login");
        let mut cfg = v["config"].take();
        f(&mut cfg);
        let (status, v) = self.send("PUT", "/api/admin/config", Some(cfg));
        assert_eq!(status, 200, "{v}");
    }

    fn upload(&self, path: &str, filename: &str, bytes: &[u8]) -> (u16, Value) {
        let boundary = "----shphtestboundary";
        let mut body = Vec::new();
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let res = self
            .req("POST", &format!("/api{path}"))
            .set("Content-Type", &format!("multipart/form-data; boundary={boundary}"))
            .send_bytes(&body);
        let resp = match res {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => panic!("upload: {e}"),
        };
        let status = resp.status();
        (status, serde_json::from_str(&resp.into_string().unwrap()).unwrap_or(Value::Null))
    }

    fn db(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.dir.join("data").join("queue.db")).unwrap()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
        if !self.dir.as_os_str().is_empty() {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

fn room(st: &Value, id: u64) -> &Value {
    st["rooms"].as_array().unwrap().iter().find(|r| r["id"] == id).unwrap()
}

fn current(st: &Value, id: u64) -> Option<u64> {
    room(st, id)["current"].as_u64()
}

fn numbers(list: &Value) -> Vec<u64> {
    list.as_array().unwrap().iter().map(|x| x["number"].as_u64().unwrap()).collect()
}

// ------------------------------------------------------------------ queue flow

#[test]
fn exam_rooms_share_one_counter() {
    let s = Server::start();
    s.post("/rooms/1/next", json!({}));
    s.post("/rooms/3/next", json!({}));
    let st = s.post("/rooms/2/next", json!({}))["state"].clone();
    assert_eq!(current(&st, 1), Some(1));
    assert_eq!(current(&st, 3), Some(2));
    assert_eq!(current(&st, 2), Some(3));
    assert_eq!(st["next"], 4);
    assert_eq!(st["upcoming"], json!([4, 5, 6]));
}

#[test]
fn counter_wraps_at_the_last_ticket() {
    let s = Server::start();
    s.post("/queue/last", json!({ "last": 98 }));
    let st = s.post("/rooms/1/next", json!({}))["state"].clone();
    assert_eq!(current(&st, 1), Some(99));
    assert_eq!(st["upcoming"], json!([1, 2, 3]));
    let st = s.post("/rooms/2/next", json!({}))["state"].clone();
    assert_eq!(current(&st, 2), Some(1));
}

#[test]
fn pharmacy_flow_and_finishing_without_medicine() {
    let s = Server::start();
    s.post("/rooms/1/next", json!({})); // 1
    s.post("/rooms/2/next", json!({})); // 2
    s.post("/rooms/1/done", json!({ "pharmacy": true }));
    let st = s.post("/rooms/2/done", json!({ "pharmacy": false }))["state"].clone();
    assert_eq!(numbers(&st["pharmacy"]), vec![1], "only room 1 sent its patient for medicine");
    assert_eq!(room(&st, 1)["served"], 1);
    assert_eq!(room(&st, 2)["served"], 1);

    let st = s.post("/rooms/4/next", json!({}))["state"].clone();
    assert_eq!(current(&st, 4), Some(1));
    assert!(st["pharmacy"].as_array().unwrap().is_empty());
    let (_, err) = s.post_err("/rooms/4/next", json!({}));
    assert_eq!(err, "ยังไม่มีคิวรอรับยา");
    let st = s.post("/rooms/4/done", json!({}))["state"].clone();
    assert_eq!(current(&st, 4), None);
    assert_eq!(room(&st, 4)["served"], 1);
}

#[test]
fn no_show_hold_recall_and_remove() {
    let s = Server::start();
    s.post("/rooms/1/next", json!({})); // 1
    let st = s.post("/rooms/1/skip", json!({}))["state"].clone();
    assert_eq!(current(&st, 1), None);
    assert_eq!(numbers(&st["held"]), vec![1]);

    // Recall from another room.
    let st = s.post("/rooms/2/call", json!({ "number": 1 }))["state"].clone();
    assert_eq!(current(&st, 2), Some(1));
    assert!(st["held"].as_array().unwrap().is_empty());

    // Manage page removes a held number.
    s.post("/rooms/2/skip", json!({}));
    s.post("/held/1/remove", json!({}));
    assert!(s.state()["held"].as_array().unwrap().is_empty());
    let (status, _) = s.post_err("/held/1/remove", json!({}));
    assert_eq!(status, 400);
}

#[test]
fn repeat_announces_without_changing_the_queue() {
    let s = Server::start();
    s.post("/rooms/1/next", json!({}));
    let before = s.state();
    s.post("/rooms/1/repeat", json!({}));
    let after = s.state();
    assert_eq!(before["last"], after["last"]);
    assert_eq!(current(&after, 1), Some(1));
    let (_, err) = s.post_err("/rooms/2/repeat", json!({}));
    assert_eq!(err, "ยังไม่มีคิวในห้องนี้");
}

#[test]
fn undo_reverses_each_kind_of_press() {
    let s = Server::start();
    s.post("/rooms/1/next", json!({})); // 1
    s.post("/rooms/1/next", json!({})); // 2, finishes 1
    let st = s.post("/rooms/1/undo", json!({}))["state"].clone();
    assert_eq!(current(&st, 1), Some(1));
    assert_eq!(st["last"], 1, "counter goes back when nobody called after");
    assert_eq!(room(&st, 1)["served"], 0);

    s.post("/rooms/1/skip", json!({}));
    let st = s.post("/rooms/1/undo", json!({}))["state"].clone();
    assert_eq!(current(&st, 1), Some(1));
    assert!(st["held"].as_array().unwrap().is_empty());

    s.post("/rooms/1/done", json!({ "pharmacy": true }));
    let st = s.post("/rooms/1/undo", json!({}))["state"].clone();
    assert_eq!(current(&st, 1), Some(1));
    assert!(st["pharmacy"].as_array().unwrap().is_empty());

    // Room 1 calls 2, room 2 calls 3, then room 1 undoes: 2 cannot go back on the counter.
    s.post("/rooms/1/next", json!({}));
    s.post("/rooms/2/next", json!({}));
    let st = s.post("/rooms/1/undo", json!({}))["state"].clone();
    assert_eq!(current(&st, 1), Some(1));
    assert_eq!(st["last"], 3);
    assert_eq!(numbers(&st["held"]), vec![2]);

    let (_, err) = s.post_err("/rooms/3/undo", json!({}));
    assert_eq!(err, "ไม่มีรายการให้ย้อนกลับ");
}

#[test]
fn bad_input_is_rejected_with_a_message() {
    let s = Server::start();
    for (path, body) in [
        ("/rooms/1/call", json!({ "number": 0 })),
        ("/rooms/1/call", json!({ "number": 1000 })),
        ("/rooms/99/next", json!({})),
        ("/rooms/1/skip", json!({})),
        ("/rooms/1/done", json!({ "pharmacy": true })),
        ("/queue/last", json!({ "last": 100 })),
        ("/pharmacy/5/remove", json!({})),
    ] {
        let (status, err) = s.post_err(path, body);
        assert_eq!(status, 400, "{path}");
        assert!(!err.is_empty(), "{path} should explain the error");
    }
    // Malformed JSON never reaches the queue.
    let (status, _) = s.send("POST", "/api/rooms/1/call", Some(json!({ "number": "abc" })));
    assert!(status >= 400);
    assert_eq!(s.state()["last"], 0);
}

#[test]
fn reset_clears_everything() {
    let s = Server::start();
    s.post("/rooms/1/next", json!({}));
    s.post("/rooms/2/next", json!({}));
    s.post("/rooms/2/skip", json!({}));
    s.post("/rooms/1/done", json!({ "pharmacy": true }));
    let st = s.post("/queue/reset", json!({}))["state"].clone();
    assert_eq!(st["last"], 0);
    assert_eq!(st["next"], 1);
    assert!(st["held"].as_array().unwrap().is_empty());
    assert!(st["pharmacy"].as_array().unwrap().is_empty());
    assert!(st["rooms"].as_array().unwrap().iter().all(|r| r["current"].is_null() && r["served"] == 0));
}

#[test]
fn activity_log_records_presses_newest_first() {
    let s = Server::start();
    s.post("/rooms/1/next", json!({}));
    s.post("/rooms/1/skip", json!({}));
    let (_, log) = s.send("GET", "/api/log?limit=10", None);
    let actions: Vec<&str> = log["events"].as_array().unwrap().iter().map(|e| e["action"].as_str().unwrap()).collect();
    assert_eq!(actions, vec!["skip", "call"]);
    assert_eq!(log["events"][0]["room_name"], "ห้องตรวจ 1");
}

#[test]
fn simultaneous_presses_never_hand_out_the_same_number() {
    let s = Server::start();
    let base = s.url("/api");
    let handles: Vec<_> = (0..30)
        .map(|i| {
            let url = format!("{base}/rooms/{}/next", 1 + i % 3);
            std::thread::spawn(move || {
                ureq::post(&url).send_json(json!({})).unwrap();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let (_, log) = s.send("GET", "/api/log?limit=100", None);
    let mut called: Vec<u64> = log["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["action"] == "call")
        .map(|e| e["number"].as_u64().unwrap())
        .collect();
    called.sort();
    assert_eq!(called, (1..=30).collect::<Vec<_>>());
}

// ------------------------------------------------------------------ persistence and timers

#[test]
fn queue_survives_a_restart() {
    let s = Server::start();
    s.post("/rooms/1/next", json!({}));
    s.post("/rooms/2/next", json!({}));
    s.post("/rooms/2/skip", json!({}));
    s.post("/rooms/1/done", json!({ "pharmacy": true }));
    s.post("/rooms/3/next", json!({}));
    let before = s.state();

    let s = s.restart();
    let after = s.state();
    for key in ["last", "next", "held", "pharmacy", "day"] {
        assert_eq!(before[key], after[key], "{key}");
    }
    assert_eq!(current(&after, 3), Some(3));
    assert_eq!(room(&after, 1)["served"], 1);
    // Undo history is memory-only by design.
    assert_eq!(room(&after, 3)["can_undo"], false);
}

#[test]
fn a_new_day_starts_a_fresh_queue() {
    let s = Server::start();
    s.post("/rooms/1/next", json!({}));
    s.post("/rooms/1/skip", json!({}));
    let mut s = s;
    s.stop();
    {
        let db = s.db();
        let core: String = db.query_row("SELECT value FROM kv WHERE key='core'", [], |r| r.get(0)).unwrap();
        let mut core: Value = serde_json::from_str(&core).unwrap();
        core["day"] = json!("2000-01-01");
        db.execute("UPDATE kv SET value=?1 WHERE key='core'", [core.to_string()]).unwrap();
    }
    let s = s.restart();
    let st = s.state();
    assert_ne!(st["day"], "2000-01-01");
    assert_eq!(st["last"], 0);
    assert!(st["held"].as_array().unwrap().is_empty());
}

#[test]
fn held_numbers_expire_after_the_configured_minutes() {
    let mut s = Server::start();
    assert_eq!(s.login("admin"), 200);
    s.put_config(|c| c["held_expire_minutes"] = json!(30));
    for _ in 0..2 {
        s.post("/rooms/1/next", json!({}));
        s.post("/rooms/1/skip", json!({}));
    }
    s.stop();
    {
        // Age number 1 by 31 minutes; leave number 2 fresh.
        let db = s.db();
        let core: String = db.query_row("SELECT value FROM kv WHERE key='core'", [], |r| r.get(0)).unwrap();
        let mut core: Value = serde_json::from_str(&core).unwrap();
        let since = core["held"][0]["since"].as_i64().unwrap();
        core["held"][0]["since"] = json!(since - 31 * 60);
        db.execute("UPDATE kv SET value=?1 WHERE key='core'", [core.to_string()]).unwrap();
    }
    let s = s.restart();
    assert_eq!(numbers(&s.state()["held"]), vec![2]);
    let (_, log) = s.send("GET", "/api/log?limit=5", None);
    assert_eq!(log["events"][0]["action"], "expire");
    assert_eq!(log["events"][0]["number"], 1);
}

// ------------------------------------------------------------------ admin

#[test]
fn admin_pages_need_a_login() {
    let mut s = Server::start();
    for path in
        ["/api/admin/settings", "/api/admin/stats", "/api/admin/clients", "/api/admin/voice", "/api/admin/backup"]
    {
        let (status, _) = s.send("GET", path, None);
        assert_eq!(status, 401, "{path}");
    }
    let (status, _) = s.send("PUT", "/api/admin/config", Some(json!({})));
    assert_eq!(status, 401);
    let (status, _) = s.send(
        "POST",
        "/api/admin/rooms",
        Some(json!({ "name": "x", "kind": "exam", "voice_phrase": "exam", "voice_number": null })),
    );
    assert_eq!(status, 401);

    assert_eq!(s.login("wrong"), 401);
    assert_eq!(s.login("admin"), 200);
    let (_, session) = s.send("GET", "/api/admin/session", None);
    assert_eq!(session["logged_in"], true);
    assert_eq!(session["default_password"], true);
    let (status, _) = s.send("GET", "/api/admin/settings", None);
    assert_eq!(status, 200);

    s.send("POST", "/api/admin/logout", Some(json!({})));
    let (status, _) = s.send("GET", "/api/admin/settings", None);
    assert_eq!(status, 401, "old session cookie stops working after logout");
}

#[test]
fn password_change() {
    let mut s = Server::start();
    s.login("admin");
    let (status, v) = s.send("POST", "/api/admin/password", Some(json!({ "current": "nope", "new": "secret1" })));
    assert_eq!((status, v["error"].as_str().unwrap()), (400, "รหัสผ่านเดิมไม่ถูกต้อง"));
    let (status, _) = s.send("POST", "/api/admin/password", Some(json!({ "current": "admin", "new": "abc" })));
    assert_eq!(status, 400, "too short");
    let (status, _) = s.send("POST", "/api/admin/password", Some(json!({ "current": "admin", "new": "secret1" })));
    assert_eq!(status, 200);

    let mut s = s.restart();
    assert_eq!(s.login("admin"), 401);
    assert_eq!(s.login("secret1"), 200);
    let (_, session) = s.send("GET", "/api/admin/session", None);
    assert_eq!(session["default_password"], false);
}

#[test]
fn settings_are_saved_and_clamped() {
    let mut s = Server::start();
    s.login("admin");
    s.put_config(|c| {
        c["org_name"] = json!("บ้านทดสอบ");
        c["number_min"] = json!(5);
        c["number_max"] = json!(3); // below min: clamped to min + 1
        c["sound"]["repeat"] = json!(50);
        c["upcoming_count"] = json!(9);
    });
    let s = s.restart();
    let cfg = &s.state()["config"];
    assert_eq!(cfg["org_name"], "บ้านทดสอบ");
    assert_eq!(cfg["number_min"], 5);
    assert_eq!(cfg["number_max"], 6);
    assert_eq!(cfg["sound"]["repeat"], 5);
    assert_eq!(s.state()["upcoming"].as_array().unwrap().len(), 3);
    assert_eq!(s.state()["next"], 5);
}

#[test]
fn rooms_can_be_added_disabled_moved_and_deleted() {
    let mut s = Server::start();
    s.login("admin");
    let room = |name: &str, kind: &str, enabled: bool| json!({ "name": name, "kind": kind, "enabled": enabled, "voice_phrase": "dental", "voice_number": null });
    let (status, v) = s.send("POST", "/api/admin/rooms", Some(room("ห้องทันตกรรม", "exam", true)));
    assert_eq!(status, 200);
    let id = v["rooms"].as_array().unwrap().last().unwrap()["id"].as_u64().unwrap();
    assert_eq!(id, 5);
    assert_eq!(s.post("/rooms/5/next", json!({}))["state"]["rooms"][4]["current"], 1);

    let (status, _) = s.send("POST", "/api/admin/rooms", Some(room("  ", "exam", true)));
    assert_eq!(status, 400, "blank name");

    // Disabling clears the room and blocks calls.
    s.send("PUT", "/api/admin/rooms/5", Some(room("ห้องทันตกรรม", "exam", false)));
    let st = s.state();
    assert!(!room_active(&st, 5));
    assert_eq!(current(&st, 5), None);
    assert_eq!(s.post_err("/rooms/5/next", json!({})).0, 400);

    let (_, v) = s.send("POST", "/api/admin/rooms/5/move", Some(json!({ "up": true })));
    let order: Vec<u64> = v["rooms"].as_array().unwrap().iter().map(|r| r["id"].as_u64().unwrap()).collect();
    assert_eq!(order, vec![1, 2, 3, 5, 4]);

    s.send("DELETE", "/api/admin/rooms/5", None);
    assert_eq!(s.state()["rooms"].as_array().unwrap().len(), 4);
}

fn room_active(st: &Value, id: u64) -> bool {
    room(st, id)["active"].as_bool().unwrap()
}

#[test]
fn turning_the_pharmacy_off() {
    let mut s = Server::start();
    s.login("admin");
    s.put_config(|c| c["pharmacy_enabled"] = json!(false));
    let st = s.state();
    assert!(!room_active(&st, 4));
    s.post("/rooms/1/next", json!({}));
    let st = s.post("/rooms/1/done", json!({ "pharmacy": true }))["state"].clone();
    assert!(st["pharmacy"].as_array().unwrap().is_empty(), "nothing is sent to a closed pharmacy");
    assert_eq!(s.post_err("/rooms/4/next", json!({})).0, 400);
}

#[test]
fn media_and_voice_uploads() {
    let mut s = Server::start();
    s.login("admin");
    let png = b"\x89PNG\r\n\x1a\n fake image";
    let (status, v) = s.upload("/admin/media", "โปสเตอร์.png", png);
    assert_eq!(status, 200, "{v}");
    let file = &v["files"][0];
    assert_eq!(file["kind"], "image");
    assert_eq!(file["title"], "โปสเตอร์");
    let src = file["src"].as_str().unwrap().to_string();
    let mut body = Vec::new();
    ureq::get(&s.url(&format!("/media/{src}"))).call().unwrap().into_reader().read_to_end(&mut body).unwrap();
    assert_eq!(body, png);

    let (status, v) = s.upload("/admin/media", "virus.exe", b"MZ");
    assert_eq!(status, 400);
    assert!(v["error"].as_str().unwrap().contains("ไม่รองรับ"));

    let (status, _) = s.send("DELETE", &format!("/api/admin/media/{src}"), None);
    assert_eq!(status, 200);
    assert_eq!(ureq::get(&s.url(&format!("/media/{src}"))).call().map(|r| r.status()).unwrap_or(404), 404);
    let (status, _) = s.send("DELETE", "/api/admin/media/..%2Fqueue.db", None);
    assert_eq!(status, 400, "path traversal");

    // Voice: only names from the manifest (or room_<id>) are accepted.
    let (_, v) = s.upload("/admin/voice", "num_12.mp3", b"ID3 fake");
    assert_eq!(v["saved"], json!(["num_12"]));
    let (_, v) = s.upload("/admin/voice", "hello.mp3", b"ID3 fake");
    assert_eq!(v["skipped"], json!(["hello.mp3"]));
    let (_, list) = s.send("GET", "/api/admin/voice", None);
    let present: Vec<&str> = list["lines"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["present"] == true)
        .map(|l| l["name"].as_str().unwrap())
        .collect();
    assert_eq!(present, vec!["num_12"]);
}

#[test]
fn stats_and_backup() {
    let mut s = Server::start();
    s.login("admin");
    s.post("/rooms/1/next", json!({}));
    s.post("/rooms/1/done", json!({ "pharmacy": true }));
    s.post("/rooms/2/next", json!({}));
    s.post("/rooms/2/skip", json!({}));
    let (_, stats) = s.send("GET", "/api/admin/stats", None);
    let r1 = &stats["rooms"][0];
    assert_eq!(
        (r1["calls"].as_u64(), r1["finished"].as_u64(), r1["to_pharmacy"].as_u64()),
        (Some(1), Some(1), Some(1))
    );
    assert_eq!(stats["rooms"][1]["skipped"], 1);
    assert_eq!(stats["hourly"].as_array().unwrap().iter().map(|h| h.as_u64().unwrap()).sum::<u64>(), 2);

    let resp = s.req("GET", "/api/admin/backup").call().unwrap();
    assert!(resp.header("content-disposition").unwrap().contains(".db"));
    let mut bytes = Vec::new();
    resp.into_reader().read_to_end(&mut bytes).unwrap();
    assert!(bytes.starts_with(b"SQLite format 3"));
}

// ------------------------------------------------------------------ live updates

fn ws_next(
    ws: &mut tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
    t: &str,
) -> Value {
    loop {
        match ws.read().expect("websocket read") {
            tungstenite::Message::Text(txt) => {
                let v: Value = serde_json::from_str(&txt).unwrap();
                if v["t"] == t {
                    return v;
                }
            }
            _ => continue,
        }
    }
}

fn copy_voice(repo_voice: &Path, dest: &Path) -> usize {
    let mut n = 0;
    for e in std::fs::read_dir(repo_voice).unwrap().flatten() {
        if e.path().extension().is_some_and(|x| x == "mp3") {
            std::fs::copy(e.path(), dest.join(e.file_name())).unwrap();
            n += 1;
        }
    }
    n
}

#[test]
fn screens_get_state_and_calls_over_websocket() {
    let s = Server::start();
    let (mut ws, _) = tungstenite::connect(format!("ws://127.0.0.1:{}/ws?role=display", s.port)).unwrap();
    let hello = ws_next(&mut ws, "state");
    assert_eq!(hello["state"]["next"], 1);

    s.post("/rooms/2/next", json!({}));
    let st = ws_next(&mut ws, "state");
    assert_eq!(current(&st["state"], 2), Some(1));
    let call = ws_next(&mut ws, "call");
    assert_eq!(call["number"], 1);
    assert_eq!(call["room_name"], "ห้องตรวจ 2");
    assert_eq!(call["text"], "ขอเชิญหมายเลข หนึ่ง ที่ห้องตรวจ สอง ค่ะ");
    assert_eq!(call["clips"], json!([]), "no clips installed yet, so the display uses browser TTS");
}

#[test]
fn announcement_uses_voice_clips_when_present() {
    let s = Server::start();
    let repo_voice = Path::new(env!("CARGO_MANIFEST_DIR")).join("voice");
    if copy_voice(&repo_voice, &s.dir.join("voice")) == 0 {
        eprintln!("voice/*.mp3 not in the repo; skipping clip check");
        return;
    }
    let (mut ws, _) = tungstenite::connect(format!("ws://127.0.0.1:{}/ws?role=display", s.port)).unwrap();
    ws_next(&mut ws, "state");
    s.post("/queue/last", json!({ "last": 11 }));
    s.post("/rooms/2/next", json!({}));
    let call = ws_next(&mut ws, "call");
    assert_eq!(
        call["clips"],
        json!([
            "/voice/invite.mp3",
            "/voice/num_12.mp3",
            "/voice/phrase_exam.mp3",
            "/voice/num_2.mp3",
            "/voice/end.mp3"
        ])
    );
    for clip in call["clips"].as_array().unwrap() {
        let resp = ureq::get(&s.url(clip.as_str().unwrap())).call().unwrap();
        assert_eq!(resp.status(), 200);
    }
    // Pharmacy phrase has no room number.
    s.post("/rooms/2/done", json!({ "pharmacy": true }));
    s.post("/rooms/4/next", json!({}));
    let call = ws_next(&mut ws, "call");
    assert_eq!(call["text"], "ขอเชิญหมายเลข สิบสอง รับยาที่ห้องยา ค่ะ");
    assert_eq!(call["clips"].as_array().unwrap().len(), 4);
}

#[test]
fn every_page_and_asset_is_served() {
    let s = Server::start();
    for path in [
        "/",
        "/display",
        "/room",
        "/room/1",
        "/manage",
        "/admin",
        "/favicon.ico",
        "/assets/common.js",
        "/assets/common.css",
        "/assets/display.js",
        "/assets/display.css",
        "/assets/admin.js",
        "/assets/fonts/fonts.css",
        "/assets/fonts/Anuphan-thai.woff2",
    ] {
        let resp = ureq::get(&s.url(path)).call().unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!(resp.status(), 200, "{path}");
    }
    assert_eq!(ureq::get(&s.url("/assets/nope.js")).call().map(|r| r.status()).unwrap_or(404), 404);
}

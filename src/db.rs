//! SQLite storage: settings and queue state as JSON rows, plus an activity log.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};

use crate::queue::LogEntry;

pub struct Db {
    conn: Connection,
}

#[derive(Debug, Serialize)]
pub struct EventRow {
    pub ts: i64,
    pub room_id: u32,
    pub room_name: String,
    pub number: u32,
    pub action: String,
}

#[derive(Debug, Serialize, Default)]
pub struct RoomStat {
    pub room_id: u32,
    pub room_name: String,
    pub calls: u32,
    pub finished: u32,
    pub to_pharmacy: u32,
    pub skipped: u32,
}

#[derive(Debug, Serialize, Default)]
pub struct DayStats {
    pub day: String,
    pub rooms: Vec<RoomStat>,
    /// Calls per hour, index 0–23.
    pub hourly: Vec<u32>,
    pub expired: u32,
}

impl Db {
    pub fn open(path: &Path) -> rusqlite::Result<Db> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                ts INTEGER NOT NULL,
                day TEXT NOT NULL,
                hour INTEGER NOT NULL,
                room_id INTEGER NOT NULL,
                room_name TEXT NOT NULL,
                number INTEGER NOT NULL,
                action TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS events_day ON events(day);",
        )?;
        Ok(Db { conn })
    }

    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let s: Option<String> =
            self.conn.query_row("SELECT value FROM kv WHERE key = ?1", [key], |r| r.get(0)).optional().ok().flatten();
        s.and_then(|s| serde_json::from_str(&s).ok())
    }

    pub fn set<T: Serialize>(&self, key: &str, value: &T) -> rusqlite::Result<()> {
        let s = serde_json::to_string(value).expect("serializable");
        self.conn.execute(
            "INSERT INTO kv(key, value) VALUES(?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, s],
        )?;
        Ok(())
    }

    /// Save queue state and its log entries in one transaction.
    #[allow(clippy::too_many_arguments)]
    pub fn save_queue<C: Serialize, S: Serialize>(
        &mut self,
        core: &C,
        served: &S,
        entries: &[LogEntry],
        ts: i64,
        day: &str,
        hour: u32,
        room_name: impl Fn(u32) -> String,
    ) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        for (key, value) in [("core", serde_json::to_string(core)), ("served", serde_json::to_string(served))] {
            tx.execute(
                "INSERT INTO kv(key, value) VALUES(?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value.expect("serializable")],
            )?;
        }
        for e in entries {
            tx.execute(
                "INSERT INTO events(ts, day, hour, room_id, room_name, number, action) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![ts, day, hour, e.room_id, room_name(e.room_id), e.number, e.action],
            )?;
        }
        tx.commit()
    }

    pub fn recent_events(&self, day: &str, limit: u32) -> Vec<EventRow> {
        let mut st = match self.conn.prepare(
            "SELECT ts, room_id, room_name, number, action FROM events WHERE day = ?1 ORDER BY id DESC LIMIT ?2",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        st.query_map(params![day, limit], |r| {
            Ok(EventRow {
                ts: r.get(0)?,
                room_id: r.get(1)?,
                room_name: r.get(2)?,
                number: r.get(3)?,
                action: r.get(4)?,
            })
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    }

    pub fn day_stats(&self, day: &str) -> DayStats {
        let mut stats = DayStats { day: day.to_string(), hourly: vec![0; 24], ..Default::default() };
        let Ok(mut st) = self.conn.prepare(
            "SELECT room_id, room_name, action, hour, COUNT(*) FROM events WHERE day = ?1 GROUP BY room_id, action, hour",
        ) else {
            return stats;
        };
        let rows = st.query_map([day], |r| {
            Ok((
                r.get::<_, u32>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, u32>(3)?,
                r.get::<_, u32>(4)?,
            ))
        });
        let Ok(rows) = rows else { return stats };
        for (room_id, room_name, action, hour, count) in rows.flatten() {
            if action == "expire" {
                stats.expired += count;
            }
            if room_id == 0 {
                continue;
            }
            let i = match stats.rooms.iter().position(|s| s.room_id == room_id) {
                Some(i) => i,
                None => {
                    stats.rooms.push(RoomStat { room_id, room_name: room_name.clone(), ..Default::default() });
                    stats.rooms.len() - 1
                }
            };
            let s = &mut stats.rooms[i];
            match action.as_str() {
                "call" | "call_manual" | "recall" => {
                    s.calls += count;
                    if let Some(h) = stats.hourly.get_mut(hour as usize) {
                        *h += count;
                    }
                }
                "done" | "dispensed" | "auto_done" => s.finished += count,
                "to_pharmacy" => {
                    s.finished += count;
                    s.to_pharmacy += count;
                }
                "skip" => s.skipped += count,
                _ => {}
            }
        }
        stats.rooms.sort_by_key(|s| s.room_id);
        stats
    }

    /// Write a consistent copy of the database to `dest`.
    pub fn backup_to(&self, dest: &Path) -> rusqlite::Result<()> {
        let _ = std::fs::remove_file(dest);
        self.conn.execute("VACUUM INTO ?1", [dest.to_string_lossy()])?;
        Ok(())
    }
}

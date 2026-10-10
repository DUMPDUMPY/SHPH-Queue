//! Settings and queue state. Both are stored as JSON rows in SQLite.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoomKind {
    /// Calls the next paper-ticket number.
    Exam,
    /// Calls numbers that an exam room sent for medicine.
    Pharmacy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Room {
    pub id: u32,
    pub name: String,
    pub kind: RoomKind,
    pub enabled: bool,
    /// Key into `thai::PHRASES`, e.g. "exam" → "ที่ห้องตรวจ".
    pub voice_phrase: String,
    /// Spoken after the phrase, e.g. 2 → "ที่ห้องตรวจ สอง". None for a single pharmacy.
    pub voice_number: Option<u32>,
    pub current: Option<u32>,
    /// Unix seconds of the last call in this room.
    pub called_at: Option<i64>,
}

/// A number that was called but the patient did not come.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Held {
    pub number: u32,
    pub kind: RoomKind,
    pub room_id: u32,
    pub since: i64,
}

/// A number waiting at the pharmacy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Waiting {
    pub number: u32,
    pub room_id: u32,
    pub since: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Core {
    /// Day (YYYY-MM-DD) this queue belongs to; a new day resets the queue.
    pub day: String,
    /// Last paper-ticket number called. 0 means none yet.
    pub last: u32,
    pub rooms: Vec<Room>,
    pub held: Vec<Held>,
    pub pharmacy: Vec<Waiting>,
    pub next_room_id: u32,
}

impl Core {
    pub fn new(day: String) -> Self {
        let mut rooms = Vec::new();
        for i in 1..=3 {
            rooms.push(Room {
                id: i,
                name: format!("ห้องตรวจ {i}"),
                kind: RoomKind::Exam,
                enabled: true,
                voice_phrase: "exam".into(),
                voice_number: Some(i),
                current: None,
                called_at: None,
            });
        }
        rooms.push(Room {
            id: 4,
            name: "ห้องยา".into(),
            kind: RoomKind::Pharmacy,
            enabled: true,
            voice_phrase: "pharmacy".into(),
            voice_number: None,
            current: None,
            called_at: None,
        });
        Core { day, last: 0, rooms, held: vec![], pharmacy: vec![], next_room_id: 5 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundConfig {
    pub enabled: bool,
    /// How many times one call is announced.
    pub repeat: u32,
    /// Pause between repeats, in seconds.
    pub gap_seconds: f32,
    pub chime: bool,
    /// 0–100
    pub volume: u32,
    /// Use the browser's Thai voice when clip files are missing.
    pub browser_tts_fallback: bool,
}

impl Default for SoundConfig {
    fn default() -> Self {
        SoundConfig { enabled: true, repeat: 2, gap_seconds: 1.5, chime: true, volume: 100, browser_tts_fallback: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaItem {
    pub id: String,
    /// "youtube" | "video" | "image"
    pub kind: String,
    /// YouTube URL, or a file name under /media/.
    pub src: String,
    #[serde(default)]
    pub title: String,
    /// Display time for images, in seconds.
    #[serde(default)]
    pub seconds: Option<u32>,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Play this video or YouTube clip without sound.
    #[serde(default)]
    pub muted: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub org_prefix: String,
    pub org_name: String,
    pub number_min: u32,
    pub number_max: u32,
    pub auto_reset_daily: bool,
    pub pharmacy_enabled: bool,
    /// Remove a held (no-show) number after this many minutes. 0 keeps it until the day ends.
    pub held_expire_minutes: u32,
    /// How many upcoming numbers the TV shows under "เตรียมตัว".
    pub upcoming_count: u32,
    pub sound: SoundConfig,
    pub ticker: String,
    /// Minimum seconds the big call banner stays on the TV.
    pub callout_seconds: u32,
    pub image_seconds: u32,
    pub media: Vec<MediaItem>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            org_prefix: "โรงพยาบาลส่งเสริมสุขภาพตำบล".into(),
            org_name: "บ้านตัวอย่าง".into(),
            number_min: 1,
            number_max: 99,
            auto_reset_daily: true,
            pharmacy_enabled: true,
            held_expire_minutes: 60,
            upcoming_count: 3,
            sound: SoundConfig::default(),
            ticker: "กรุณาเตรียมบัตรประชาชนและบัตรคิวให้พร้อม · หากเรียกแล้วไม่อยู่ กรุณาติดต่อเจ้าหน้าที่".into(),
            callout_seconds: 5,
            image_seconds: 10,
            media: vec![],
        }
    }
}

impl Config {
    /// Clamp values an admin could set out of range.
    pub fn sanitize(&mut self) {
        self.number_min = self.number_min.clamp(1, 998);
        self.number_max = self.number_max.clamp(self.number_min + 1, 999);
        self.upcoming_count = self.upcoming_count.min(3);
        self.sound.repeat = self.sound.repeat.clamp(1, 5);
        self.sound.gap_seconds = self.sound.gap_seconds.clamp(0.0, 10.0);
        self.sound.volume = self.sound.volume.min(100);
        self.callout_seconds = self.callout_seconds.clamp(2, 30);
        self.image_seconds = self.image_seconds.clamp(3, 600);
        self.held_expire_minutes = self.held_expire_minutes.min(24 * 60);
    }
}

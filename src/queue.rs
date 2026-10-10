//! Queue operations. Every button on the room and manage pages ends up here.

use std::collections::{BTreeMap, HashMap};

use crate::model::{Core, Held, Room, RoomKind, Waiting};

/// Settings the queue logic needs from `Config`.
#[derive(Debug, Clone, Copy)]
pub struct Rules {
    pub min: u32,
    pub max: u32,
    pub pharmacy_enabled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LogEntry {
    pub room_id: u32,
    pub number: u32,
    pub action: &'static str,
}

#[derive(Debug, Default)]
pub struct Outcome {
    /// (room_id, number) to announce on the TV.
    pub call: Option<(u32, u32)>,
    pub log: Vec<LogEntry>,
}

impl Outcome {
    fn log(&mut self, room_id: u32, number: u32, action: &'static str) {
        self.log.push(LogEntry { room_id, number, action });
    }
}

#[derive(Debug, Clone)]
enum Source {
    Counter { prev_last: u32 },
    Held(Held),
    Pharmacy(Waiting, usize),
    Manual,
}

#[derive(Debug, Clone)]
enum Undo {
    Called { number: u32, prev_current: Option<u32>, prev_called_at: Option<i64>, prev_finished: bool, source: Source },
    Skipped { number: u32, called_at: Option<i64> },
    Finished { number: u32, called_at: Option<i64>, to_pharmacy: bool },
}

const UNDO_DEPTH: usize = 10;

pub type Result<T> = std::result::Result<T, String>;

pub struct Queue {
    pub core: Core,
    /// Rooms served today, by room id.
    pub served: BTreeMap<u32, u32>,
    undo: HashMap<u32, Vec<Undo>>,
}

impl Queue {
    pub fn new(core: Core, served: BTreeMap<u32, u32>) -> Self {
        Queue { core, served, undo: HashMap::new() }
    }

    pub fn next_number(&self, r: &Rules) -> u32 {
        let last = self.core.last;
        if last < r.min || last >= r.max {
            r.min
        } else {
            last + 1
        }
    }

    pub fn can_undo(&self, room_id: u32) -> bool {
        self.undo.get(&room_id).is_some_and(|v| !v.is_empty())
    }

    fn room_index(&self, room_id: u32, r: &Rules) -> Result<usize> {
        let i = self.core.rooms.iter().position(|x| x.id == room_id).ok_or_else(|| "ไม่พบห้องนี้".to_string())?;
        let room = &self.core.rooms[i];
        if !room.enabled || (room.kind == RoomKind::Pharmacy && !r.pharmacy_enabled) {
            return Err(format!("{} ปิดใช้งานอยู่", room.name));
        }
        Ok(i)
    }

    fn push_undo(&mut self, room_id: u32, u: Undo) {
        let v = self.undo.entry(room_id).or_default();
        v.push(u);
        if v.len() > UNDO_DEPTH {
            v.remove(0);
        }
    }

    fn bump_served(&mut self, room_id: u32, delta: i32) {
        let e = self.served.entry(room_id).or_insert(0);
        *e = (*e as i32 + delta).max(0) as u32;
    }

    /// Put `number` in the room, finishing whoever was there.
    fn place(&mut self, i: usize, number: u32, source: Source, action: &'static str, now: i64, out: &mut Outcome) {
        let room_id = self.core.rooms[i].id;
        let prev_current = self.core.rooms[i].current;
        let prev_called_at = self.core.rooms[i].called_at;
        if let Some(p) = prev_current {
            self.bump_served(room_id, 1);
            out.log(room_id, p, "auto_done");
        }
        let room = &mut self.core.rooms[i];
        room.current = Some(number);
        room.called_at = Some(now);
        self.push_undo(
            room_id,
            Undo::Called { number, prev_current, prev_called_at, prev_finished: prev_current.is_some(), source },
        );
        out.log(room_id, number, action);
        out.call = Some((room_id, number));
    }

    pub fn call_next(&mut self, room_id: u32, r: &Rules, now: i64) -> Result<Outcome> {
        let i = self.room_index(room_id, r)?;
        let mut out = Outcome::default();
        match self.core.rooms[i].kind {
            RoomKind::Exam => {
                let n = self.next_number(r);
                let prev_last = self.core.last;
                self.core.last = n;
                self.place(i, n, Source::Counter { prev_last }, "call", now, &mut out);
            }
            RoomKind::Pharmacy => {
                if self.core.pharmacy.is_empty() {
                    return Err("ยังไม่มีคิวรอรับยา".into());
                }
                let w = self.core.pharmacy.remove(0);
                let n = w.number;
                self.place(i, n, Source::Pharmacy(w, 0), "call", now, &mut out);
            }
        }
        Ok(out)
    }

    pub fn repeat(&mut self, room_id: u32, r: &Rules) -> Result<Outcome> {
        let i = self.room_index(room_id, r)?;
        let n = self.core.rooms[i].current.ok_or("ยังไม่มีคิวในห้องนี้")?;
        let mut out = Outcome::default();
        out.log(room_id, n, "repeat");
        out.call = Some((room_id, n));
        Ok(out)
    }

    /// Patient did not come: move the number to the held list.
    pub fn skip(&mut self, room_id: u32, r: &Rules, now: i64) -> Result<Outcome> {
        let i = self.room_index(room_id, r)?;
        let room = &mut self.core.rooms[i];
        let n = room.current.take().ok_or("ยังไม่มีคิวในห้องนี้")?;
        let kind = room.kind;
        let called_at = room.called_at;
        self.core.held.push(Held { number: n, kind, room_id, since: now });
        self.push_undo(room_id, Undo::Skipped { number: n, called_at });
        let mut out = Outcome::default();
        out.log(room_id, n, "skip");
        Ok(out)
    }

    /// Finish the current number. Exam rooms may send it on to the pharmacy.
    pub fn finish(&mut self, room_id: u32, to_pharmacy: bool, r: &Rules, now: i64) -> Result<Outcome> {
        let i = self.room_index(room_id, r)?;
        let room = &mut self.core.rooms[i];
        let n = room.current.take().ok_or("ยังไม่มีคิวในห้องนี้")?;
        let kind = room.kind;
        let called_at = room.called_at;
        let send = kind == RoomKind::Exam && to_pharmacy && r.pharmacy_enabled;
        let mut out = Outcome::default();
        if send {
            self.core.pharmacy.push(Waiting { number: n, room_id, since: now });
            out.log(room_id, n, "to_pharmacy");
        } else if kind == RoomKind::Pharmacy {
            out.log(room_id, n, "dispensed");
        } else {
            out.log(room_id, n, "done");
        }
        self.bump_served(room_id, 1);
        self.push_undo(room_id, Undo::Finished { number: n, called_at, to_pharmacy: send });
        Ok(out)
    }

    /// Call a specific number: a held one, one waiting for medicine, or any number typed in.
    pub fn call_number(&mut self, room_id: u32, number: u32, r: &Rules, now: i64) -> Result<Outcome> {
        let i = self.room_index(room_id, r)?;
        if number == 0 || number > 999 {
            return Err("หมายเลขไม่ถูกต้อง".into());
        }
        let kind = self.core.rooms[i].kind;
        let held_pos = self
            .core
            .held
            .iter()
            .position(|h| h.number == number && h.kind == kind)
            .or_else(|| self.core.held.iter().position(|h| h.number == number));
        let (source, action) = if let Some(p) = held_pos {
            (Source::Held(self.core.held.remove(p)), "recall")
        } else if let Some(p) =
            (kind == RoomKind::Pharmacy).then(|| self.core.pharmacy.iter().position(|w| w.number == number)).flatten()
        {
            (Source::Pharmacy(self.core.pharmacy.remove(p), p), "call")
        } else {
            (Source::Manual, "call_manual")
        };
        let mut out = Outcome::default();
        self.place(i, number, source, action, now, &mut out);
        Ok(out)
    }

    /// Reverse the last button pressed in this room.
    pub fn undo(&mut self, room_id: u32, r: &Rules, now: i64) -> Result<Outcome> {
        let i = self.room_index(room_id, r)?;
        let u = self.undo.get_mut(&room_id).and_then(|v| v.pop()).ok_or("ไม่มีรายการให้ย้อนกลับ")?;
        let mut out = Outcome::default();
        match u {
            Undo::Called { number, prev_current, prev_called_at, prev_finished, source } => {
                if self.core.rooms[i].current != Some(number) {
                    return Err("คิวในห้องเปลี่ยนไปแล้ว ย้อนกลับไม่ได้ ใช้หน้าจัดการคิวแทน".into());
                }
                let room = &mut self.core.rooms[i];
                room.current = prev_current;
                room.called_at = prev_called_at;
                let kind = room.kind;
                if prev_finished {
                    self.bump_served(room_id, -1);
                }
                match source {
                    Source::Counter { prev_last } => {
                        if self.core.last == number {
                            self.core.last = prev_last;
                        } else {
                            // Later numbers were already called; keep this one where staff can find it.
                            self.core.held.push(Held { number, kind, room_id, since: now });
                        }
                    }
                    Source::Held(h) => self.core.held.push(h),
                    Source::Pharmacy(w, idx) => {
                        let idx = idx.min(self.core.pharmacy.len());
                        self.core.pharmacy.insert(idx, w);
                    }
                    Source::Manual => {}
                }
                out.log(room_id, number, "undo");
            }
            Undo::Skipped { number, called_at } => {
                if self.core.rooms[i].current.is_some() {
                    return Err("ห้องนี้มีคิวอื่นอยู่แล้ว ย้อนกลับไม่ได้".into());
                }
                let p = self
                    .core
                    .held
                    .iter()
                    .position(|h| h.number == number && h.room_id == room_id)
                    .ok_or("หมายเลขนี้ไม่อยู่ในพักคิวแล้ว")?;
                self.core.held.remove(p);
                let room = &mut self.core.rooms[i];
                room.current = Some(number);
                room.called_at = called_at;
                out.log(room_id, number, "undo");
            }
            Undo::Finished { number, called_at, to_pharmacy } => {
                if self.core.rooms[i].current.is_some() {
                    return Err("ห้องนี้มีคิวอื่นอยู่แล้ว ย้อนกลับไม่ได้".into());
                }
                if to_pharmacy {
                    let p = self
                        .core
                        .pharmacy
                        .iter()
                        .position(|w| w.number == number && w.room_id == room_id)
                        .ok_or("ห้องยาเรียกหมายเลขนี้ไปแล้ว ย้อนกลับไม่ได้")?;
                    self.core.pharmacy.remove(p);
                }
                self.bump_served(room_id, -1);
                let room = &mut self.core.rooms[i];
                room.current = Some(number);
                room.called_at = called_at;
                out.log(room_id, number, "undo");
            }
        }
        Ok(out)
    }

    // ----- manage page -----

    pub fn remove_held(&mut self, number: u32) -> Result<Outcome> {
        let p = self.core.held.iter().position(|h| h.number == number).ok_or("ไม่พบหมายเลขนี้ในพักคิว")?;
        let h = self.core.held.remove(p);
        let mut out = Outcome::default();
        out.log(h.room_id, number, "remove_held");
        Ok(out)
    }

    pub fn remove_waiting(&mut self, number: u32) -> Result<Outcome> {
        let p = self.core.pharmacy.iter().position(|w| w.number == number).ok_or("ไม่พบหมายเลขนี้ในคิวรอรับยา")?;
        let w = self.core.pharmacy.remove(p);
        let mut out = Outcome::default();
        out.log(w.room_id, number, "remove_waiting");
        Ok(out)
    }

    /// Set the last called ticket number; the next exam call gets the one after it.
    pub fn set_last(&mut self, last: u32, r: &Rules) -> Result<Outcome> {
        if last > r.max {
            return Err(format!("ตั้งได้ 0–{}", r.max));
        }
        self.core.last = last;
        for v in self.undo.values_mut() {
            v.retain(|u| !matches!(u, Undo::Called { source: Source::Counter { .. }, .. }));
        }
        let mut out = Outcome::default();
        out.log(0, last, "set_last");
        Ok(out)
    }

    /// Start a fresh queue (new day or manual reset).
    pub fn reset(&mut self, day: String) -> Outcome {
        self.core.day = day;
        self.core.last = 0;
        self.core.held.clear();
        self.core.pharmacy.clear();
        for room in &mut self.core.rooms {
            room.current = None;
            room.called_at = None;
        }
        self.served.clear();
        self.undo.clear();
        let mut out = Outcome::default();
        out.log(0, 0, "reset");
        out
    }

    /// Drop held numbers older than `minutes`. Returns None when nothing changed.
    pub fn expire_held(&mut self, now: i64, minutes: u32) -> Option<Outcome> {
        if minutes == 0 {
            return None;
        }
        let limit = now - i64::from(minutes) * 60;
        let mut out = Outcome::default();
        self.core.held.retain(|h| {
            let keep = h.since > limit;
            if !keep {
                out.log.push(LogEntry { room_id: h.room_id, number: h.number, action: "expire" });
            }
            keep
        });
        (!out.log.is_empty()).then_some(out)
    }

    // ----- admin: rooms -----

    pub fn add_room(&mut self, mut room: Room) -> u32 {
        room.id = self.core.next_room_id;
        room.current = None;
        room.called_at = None;
        self.core.next_room_id += 1;
        let id = room.id;
        self.core.rooms.push(room);
        id
    }

    pub fn update_room(
        &mut self,
        id: u32,
        name: String,
        kind: RoomKind,
        enabled: bool,
        phrase: String,
        number: Option<u32>,
    ) -> Result<()> {
        let room = self.core.rooms.iter_mut().find(|r| r.id == id).ok_or("ไม่พบห้องนี้")?;
        if room.kind != kind || !enabled {
            room.current = None;
            self.undo.remove(&id);
        }
        room.name = name;
        room.kind = kind;
        room.enabled = enabled;
        room.voice_phrase = phrase;
        room.voice_number = number;
        Ok(())
    }

    pub fn delete_room(&mut self, id: u32) -> Result<()> {
        let p = self.core.rooms.iter().position(|r| r.id == id).ok_or("ไม่พบห้องนี้")?;
        self.core.rooms.remove(p);
        self.undo.remove(&id);
        self.served.remove(&id);
        Ok(())
    }

    pub fn move_room(&mut self, id: u32, up: bool) -> Result<()> {
        let p = self.core.rooms.iter().position(|r| r.id == id).ok_or("ไม่พบห้องนี้")?;
        let q = if up { p.checked_sub(1) } else { (p + 1 < self.core.rooms.len()).then_some(p + 1) };
        if let Some(q) = q {
            self.core.rooms.swap(p, q);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: Rules = Rules { min: 1, max: 99, pharmacy_enabled: true };

    fn q() -> Queue {
        Queue::new(Core::new("2026-10-09".into()), BTreeMap::new())
    }
    fn cur(q: &Queue, id: u32) -> Option<u32> {
        q.core.rooms.iter().find(|r| r.id == id).unwrap().current
    }

    #[test]
    fn shared_counter_and_wrap() {
        let mut q = q();
        assert_eq!(q.call_next(1, &R, 0).unwrap().call, Some((1, 1)));
        assert_eq!(q.call_next(3, &R, 0).unwrap().call, Some((3, 2)));
        q.core.last = 99;
        assert_eq!(q.call_next(2, &R, 0).unwrap().call, Some((2, 1)));
    }

    #[test]
    fn calling_next_finishes_previous() {
        let mut q = q();
        q.call_next(1, &R, 0).unwrap();
        let out = q.call_next(1, &R, 0).unwrap();
        assert_eq!(out.log[0], LogEntry { room_id: 1, number: 1, action: "auto_done" });
        assert_eq!(q.served[&1], 1);
    }

    #[test]
    fn undo_call_returns_number_to_counter() {
        let mut q = q();
        q.call_next(1, &R, 0).unwrap();
        q.call_next(1, &R, 0).unwrap();
        q.undo(1, &R, 0).unwrap();
        assert_eq!(cur(&q, 1), Some(1));
        assert_eq!(q.core.last, 1);
        assert_eq!(q.served.get(&1).copied().unwrap_or(0), 0);
    }

    #[test]
    fn undo_call_after_other_room_moved_on_keeps_number_held() {
        let mut q = q();
        q.call_next(1, &R, 0).unwrap(); // 1
        q.call_next(2, &R, 0).unwrap(); // 2
        q.undo(1, &R, 5).unwrap();
        assert_eq!(cur(&q, 1), None);
        assert_eq!(q.core.last, 2);
        assert_eq!(q.core.held[0].number, 1);
    }

    #[test]
    fn skip_recall_and_undo() {
        let mut q = q();
        q.call_next(1, &R, 0).unwrap();
        q.skip(1, &R, 10).unwrap();
        assert_eq!(cur(&q, 1), None);
        assert_eq!(q.core.held.len(), 1);
        q.undo(1, &R, 0).unwrap();
        assert_eq!(cur(&q, 1), Some(1));
        assert!(q.core.held.is_empty());

        q.skip(1, &R, 10).unwrap();
        let out = q.call_number(2, 1, &R, 20).unwrap();
        assert_eq!(out.log.last().unwrap().action, "recall");
        assert_eq!(cur(&q, 2), Some(1));
        assert!(q.core.held.is_empty());
        q.undo(2, &R, 0).unwrap();
        assert_eq!(q.core.held.len(), 1);
    }

    #[test]
    fn pharmacy_flow() {
        let mut q = q();
        q.call_next(1, &R, 0).unwrap(); // 1
        q.call_next(2, &R, 0).unwrap(); // 2
        q.finish(1, true, &R, 0).unwrap();
        q.finish(2, false, &R, 0).unwrap();
        assert_eq!(q.core.pharmacy.len(), 1);
        assert_eq!(q.served[&2], 1);
        assert_eq!(q.call_next(4, &R, 0).unwrap().call, Some((4, 1)));
        assert!(q.call_next(4, &R, 0).is_err(), "nothing left at the pharmacy");
        // Room 1 cannot undo "send to pharmacy" once the pharmacy has called the number.
        assert!(q.undo(1, &R, 0).is_err());
        // Pharmacy undo puts it back in line.
        q.undo(4, &R, 0).unwrap();
        assert_eq!(q.core.pharmacy[0].number, 1);
        assert_eq!(cur(&q, 4), None);
    }

    #[test]
    fn pharmacy_disabled() {
        let mut q = q();
        let off = Rules { pharmacy_enabled: false, ..R };
        q.call_next(1, &off, 0).unwrap();
        q.finish(1, true, &off, 0).unwrap();
        assert!(q.core.pharmacy.is_empty());
        assert!(q.call_next(4, &off, 0).is_err());
    }

    #[test]
    fn held_expiry() {
        let mut q = q();
        q.call_next(1, &R, 0).unwrap();
        q.skip(1, &R, 0).unwrap();
        assert!(q.expire_held(59 * 60, 60).is_none());
        let out = q.expire_held(61 * 60, 60).unwrap();
        assert_eq!(out.log[0].action, "expire");
        assert!(q.core.held.is_empty());
        assert!(q.expire_held(1_000_000, 0).is_none());
    }

    #[test]
    fn reset_clears_day() {
        let mut q = q();
        q.call_next(1, &R, 0).unwrap();
        q.reset("2026-10-10".into());
        assert_eq!(q.core.last, 0);
        assert_eq!(cur(&q, 1), None);
        assert!(!q.can_undo(1));
    }
}

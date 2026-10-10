# CLAUDE.md

Notes for picking this project up in a new session. The README (Thai) is the user-facing manual; this file is for whoever changes the code.

## What this is

A queue-calling system for one รพ.สต. (sub-district health center). One Windows 11 PC runs `shph-queue.exe`; every other device on the LAN opens a page in a browser:

- `/display` on a Google TV (Fully Kiosk Browser), 1920×1080
- `/room/{id}` on the PC in each exam room or the pharmacy
- `/manage` for staff to control every room from one page
- `/admin` for settings (the only page with a password)

The owner talks in Thai. Answer in Thai; keep code, comments and commit messages in English. All UI text is Thai.

## Decisions already made (do not reopen without asking)

- **Stack:** Rust (Axum 0.7, Tokio, rusqlite with bundled SQLite, rust-embed). The owner chose Rust over Python/Go/Node for a single stable `.exe`. Frontend is plain HTML/CSS/JS with no build step.
- **Paper tickets.** The system never issues tickets. One shared counter: any exam room pressing "เรียกคิวถัดไป" gets the next number. Numbers wrap from `number_max` back to `number_min` (default 1–99).
- **Exam room "done" has two outcomes:** send to pharmacy, or finish without medicine. The pharmacy can be switched off in admin, and then only "เสร็จสิ้น" shows.
- **No-shows (พักคิว):** shown on the TV 5 per page, flipping every 5 s. Removed automatically after `held_expire_minutes` (default 60, 0 = keep until the day ends).
- **Login only on `/admin`.** `/room` and `/manage` are open on the LAN by the owner's choice. `/manage` includes "เริ่มคิวใหม่" behind an in-page confirm.
- **Voice:** announcements are joined clips ("ขอเชิญหมายเลข" + number + phrase + room number + "ค่ะ"), female voice. Numbers are read as Thai words (12 = สิบสอง, 21 = ยี่สิบเอ็ด, 101 = หนึ่งร้อยเอ็ด). When clips are missing, the display falls back to the browser's Thai TTS.
- **TV design:** dark green with a saffron call highlight, fonts Bai Jamjuree (numbers, headings) and Anuphan (text), embedded under `web/fonts/`. The approved mockup is the published artifact "จอเรียกคิว รพ.สต.".

## Layout

```
src/main.rs    routes, WebSocket, housekeeping task (daily reset, held expiry every 20 s), CLI args
src/app.rs     shared state (Mutex<Inner>), persist(), broadcast, call_json() builds clip list + spoken text
src/queue.rs   all queue logic and per-room undo (pure, unit-tested)
src/api.rs     HTTP handlers, admin login (sha256+salt, cookie session in memory), uploads
src/db.rs      SQLite: kv table (config/core/served/admin as JSON) + events log
src/model.rs   Config, Core, Room, Held, Waiting + defaults and sanitize()
src/thai.rs    Thai number words, clip names, built-in phrases
web/           pages; common.js/common.css are shared by staff pages, display.* is the TV
voice/voice_lines.json   clip name -> Thai text. Must match thai::PHRASES and thai::number_clips()
tools/make_voice.py      generates clips with edge-tts (th-TH-PremwadeeNeural)
packaging/     .bat helpers and README-Windows.txt copied into the Windows zip
.github/workflows/build.yml   cargo test (Linux) -> release build + voice clips + zip (Windows)
```

Runtime files sit next to the exe: `data/queue.db`, `data/media/` (uploads), `voice/` (clips).

## How things flow

- Every button POSTs to `/api/...`. Handlers call `App::apply(|queue, rules, now| ...)`, which runs the queue op, saves core + served + log rows in one transaction, then broadcasts `{"t":"state"}` and, for calls, `{"t":"call"}` over `/ws`.
- Pages render only from the `state` message. The display queues `call` messages and skips one whose room no longer holds that number (stale after a fast skip).
- Undo is per room (last 10 actions) and kept in memory only, so it is lost on restart. Undoing a call after another room has moved the counter on puts the number into พักคิว instead of rewinding the counter.
- `served` per room is a counter in core; the stats page counts from the `events` table instead, so the two can differ slightly after undo.
- Config changes go through `PUT /api/admin/config` with the whole Config object. Rooms have their own endpoints because they live in `core`.

## Working on it

```bash
cargo test                                  # queue + Thai number tests
cargo run -- --dir ./dev-data --port 8000   # data, media and voice go under ./dev-data
cargo clippy && cargo fmt                   # rustfmt.toml sets max_width = 120
cargo build --release                       # ~4.8 MB binary
```

- In debug builds rust-embed reads `web/` from disk, so a page refresh shows HTML/JS edits without rebuilding. Release builds embed the files.
- Axum 0.7 route syntax is `/:id` and `/*path`, not `{id}`.
- To test announcements locally without network TTS, make dummy clips with ffmpeg for every name in `voice/voice_lines.json` into `<dir>/voice/`.
- Browser checks: Playwright for Python can be pip-installed into the scratchpad, with Chromium at `/opt/pw-browsers/chromium-1194/chrome-linux/chrome`. Launch it with `--autoplay-policy=no-user-gesture-required` for the display.
- Stop the dev server with `kill $(pidof shph-queue)`. `pkill -f` with a pattern that also appears in your own command line kills your shell.
- Python that prints Thai must force UTF-8 output: the Windows runner's console is cp1252. This broke voice generation once.

## Release

1. Update `CHANGELOG.md` and `version` in `Cargo.toml`.
2. Push a tag `vX.Y.Z`. The workflow attaches `SHPH-Queue-windows.zip` to a GitHub Release.
3. Check the Windows job log for `voice clips: 118`. A warning there means the zip has no voice files.

Every push also uploads the zip as a 90-day Actions artifact.

## Not verified yet

- Never run on real hardware: the Windows 11 server, Google TV with Fully Kiosk Browser (autoplay, keep-awake), and YouTube playback in the TV browser.
- Nobody has listened to the generated voice clips. Joined clips may sound choppy; the display trims silence at both ends of each clip.

## Ideas mentioned but not built

- Password for `/manage`, which was offered as an option.
- Separate volume setting for the media playlist; it is currently fixed at 100% and ducked to 12% during calls.
- Running as a Windows Service; startup currently uses a shortcut in the Startup folder.

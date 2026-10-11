"""Browser tests for the four pages. Each test gets a fresh server (see conftest.py)."""

import pathlib
import shutil
import struct
import subprocess
import zlib

import pytest
from playwright.sync_api import expect

ROOT = pathlib.Path(__file__).resolve().parents[2]


def png_bytes(w=64, h=36, rgb=(46, 127, 106)):
    raw = b"".join(b"\x00" + bytes(rgb) * w for _ in range(h))

    def chunk(tag, data):
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")


def card_num(tv, room_id):
    return tv.locator(f'.card[data-id="{room_id}"] .card-num')


def login(page, password="admin"):
    page.fill("#pw", password)
    page.click("#loginForm button")
    if password == "admin":
        expect(page.locator("#app")).to_be_visible()


def confirm(page):
    page.locator(".confirm [data-a=yes]").click()


# ---------------------------------------------------------------- room page + TV


def test_call_next_updates_room_and_tv(open_page):
    tv = open_page("/display", 1920, 1080)
    room = open_page("/room/1")
    expect(room.locator("#bNextSub")).to_have_text("หมายเลข 1")

    room.click("#bNext")
    expect(room.locator("#current")).to_have_text("1")
    expect(room.locator("#bNextSub")).to_have_text("หมายเลข 2")
    expect(room.locator("#speech")).to_contain_text("ขอเชิญหมายเลข หนึ่ง ที่ห้องตรวจ หนึ่ง ค่ะ")

    expect(tv.locator("#callout")).to_have_class("callout on")
    expect(tv.locator("#cNum")).to_have_text("1")
    expect(tv.locator("#cRoom")).to_have_text("ที่ ห้องตรวจ 1")
    expect(card_num(tv, 1)).to_have_text("1")
    expect(tv.locator('.card[data-id="1"]')).to_have_class("card hot flash")
    expect(tv.locator("#upNext .chip")).to_have_text(["2", "3", "4"])


def test_keyboard_shortcuts(open_page, server):
    room = open_page("/room/2")
    room.keyboard.press("Space")
    expect(room.locator("#current")).to_have_text("1")
    room.keyboard.press("r")
    room.wait_for_timeout(300)
    actions = [e["action"] for e in server.api("/log?limit=5")["events"]]
    assert actions[:2] == ["repeat", "call"]


def test_no_show_appears_on_tv_and_can_be_recalled(open_page):
    tv = open_page("/display", 1920, 1080)
    room = open_page("/room/1")
    room.click("#bNext")
    room.click("#bSkip")
    expect(room.locator("#current")).to_have_text("—")
    expect(tv.locator("#heldTv .chip")).to_have_text(["1"])

    room.locator('#heldList [data-call="1"]').click()
    expect(room.locator("#current")).to_have_text("1")
    expect(tv.locator("#heldTv")).to_have_text("ไม่มี")


def test_call_a_typed_number(open_page):
    room = open_page("/room/3")
    room.fill("#spec", "42")
    room.click("#specForm button")
    expect(room.locator("#current")).to_have_text("42")
    room.fill("#spec", "abc")
    room.click("#specForm button")
    expect(room.locator(".toast.error")).to_have_text("ใส่หมายเลขให้ถูกต้อง")


def test_send_to_pharmacy_or_finish_without_medicine(open_page, server):
    tv = open_page("/display", 1920, 1080)
    exam = open_page("/room/1")
    exam.click("#bNext")  # 1
    exam.click("#bDonePh")
    exam.click("#bNext")  # 2
    exam.click("#bDone")  # no medicine
    expect(exam.locator("#pharmList .pill")).to_have_text(["1"])
    expect(tv.locator('.card.pharm .card-sub')).to_have_text("รอรับยา 1")

    pharmacy = open_page("/room/4")
    expect(pharmacy.locator("#bNextSub")).to_have_text("หมายเลข 1")
    expect(pharmacy.locator("#bDonePh")).to_be_hidden()
    expect(pharmacy.locator("#bDone")).to_have_text("จ่ายยาแล้ว")
    pharmacy.click("#bNext")
    expect(pharmacy.locator("#current")).to_have_text("1")
    expect(card_num(tv, 4)).to_have_text("1")
    expect(pharmacy.locator("#bNext")).to_be_disabled()
    assert server.state()["pharmacy"] == []


def test_undo_last_press(open_page):
    room = open_page("/room/1")
    expect(room.locator("#bUndo")).to_be_disabled()
    room.click("#bNext")
    room.click("#bNext")
    expect(room.locator("#current")).to_have_text("2")
    room.click("#bUndo")
    expect(room.locator("#current")).to_have_text("1")
    expect(room.locator("#bNextSub")).to_have_text("หมายเลข 2")


def test_room_chooser_and_missing_room(open_page):
    page = open_page("/room")
    expect(page.locator("#chooser a")).to_have_count(4)
    page = open_page("/room/99")
    expect(page.locator("#missing")).to_be_visible()


# ---------------------------------------------------------------- TV details


def test_held_numbers_page_on_tv(open_page, server):
    for _ in range(7):
        server.api("/rooms/1/next", {})
        server.api("/rooms/1/skip", {})
    tv = open_page("/display", 1920, 1080)
    expect(tv.locator("#heldMeta")).to_have_text("7 คิว · หน้า 1/2")
    expect(tv.locator("#heldTv .chip")).to_have_count(5)
    expect(tv.locator("#heldMeta")).to_have_text("7 คิว · หน้า 2/2", timeout=7000)
    expect(tv.locator("#heldTv .chip")).to_have_text(["6", "7"])


def test_tv_plays_voice_clips(open_page, server):
    clips = list((ROOT / "voice").glob("*.mp3"))
    assert clips, "voice/*.mp3 missing from the repo"
    for c in clips:
        shutil.copy(c, server.dir / "voice" / c.name)
    tv = open_page("/display", 1920, 1080)
    fetched = []
    tv.on("response", lambda r: fetched.append((r.url.split("/voice/")[1], r.status)) if "/voice/" in r.url else None)
    tv.click("body")  # stands in for the remote's OK button that unlocks audio
    server.api("/queue/last", {"last": 11})
    server.api("/rooms/2/next", {})
    tv.wait_for_timeout(1500)
    assert sorted(fetched) == sorted([(n, 200) for n in ["invite.mp3", "num_12.mp3", "phrase_exam.mp3", "num_2.mp3", "end.mp3"]])


def test_tv_reconnects_after_server_restart(open_page, server):
    tv = open_page("/display", 1920, 1080)
    expect(tv.locator("#offline")).to_be_hidden()
    server.stop()
    expect(tv.locator("#offline")).to_be_visible()
    server.start()
    expect(tv.locator("#offline")).to_be_hidden(timeout=15000)
    server.api("/rooms/1/next", {})
    expect(card_num(tv, 1)).to_have_text("1")


# ---------------------------------------------------------------- manage page


def test_manage_controls_every_room(open_page, server):
    tv = open_page("/display", 1920, 1080)
    m = open_page("/manage")
    m.click('[data-act=next][data-id="3"]')
    expect(card_num(tv, 3)).to_have_text("1")
    m.click('[data-act=skip][data-id="3"]')
    expect(m.locator("#heldRows tr")).to_have_count(1)

    # Recall the held number into room 2.
    m.locator("#heldRows select").select_option("2")
    m.click('[data-act=recall][data-n="1"]')
    expect(card_num(tv, 2)).to_have_text("1")

    m.fill("#lastInput", "20")
    m.click("#lastForm button")
    confirm(m)
    expect(m.locator("#sNext")).to_have_text("21")

    m.click('[data-act=skip][data-id="2"]')
    m.click('[data-act=del-held][data-n="1"]')
    confirm(m)
    expect(m.locator("#heldRows")).to_contain_text("ไม่มีคิวที่พักไว้")
    expect(m.locator("#logRows")).to_contain_text("ลบจากพักคิว")

    m.click("#resetBtn")
    confirm(m)
    expect(m.locator("#sLast")).to_have_text("—")
    expect(m.locator("#sNext")).to_have_text("1")


def test_manage_confirm_can_be_cancelled(open_page, server):
    server.api("/rooms/1/next", {})
    m = open_page("/manage")
    m.click("#resetBtn")
    m.locator(".confirm [data-a=no]").click()
    expect(m.locator(".confirm")).to_have_count(0)
    assert server.state()["last"] == 1


# ---------------------------------------------------------------- admin page


def test_admin_login(open_page):
    open_page.allow.append("401 (Unauthorized)")  # the wrong-password request
    a = open_page("/admin")
    expect(a.locator("#app")).to_be_hidden()
    login(a, "wrong")
    expect(a.locator(".toast.error")).to_have_text("รหัสผ่านไม่ถูกต้อง")
    login(a)
    expect(a.locator("#app")).to_be_visible()
    expect(a.locator("#defaultPw")).to_be_visible()
    a.click("#logout")
    expect(a.locator("#login")).to_be_visible()


def test_admin_rooms_and_pharmacy_switch(open_page):
    tv = open_page("/display", 1920, 1080)
    room = open_page("/room/1")
    a = open_page("/admin")
    login(a)

    a.fill("#newName", "ห้องตรวจ 4")
    a.click("#addRoom button")
    expect(tv.locator(".card")).to_have_count(5)
    expect(a.locator("#roomRows tr")).to_have_count(5)
    expect(a.locator('#roomRows tr[data-id="5"] [name=vnum]')).to_have_value("4")

    a.uncheck("#pharmacyOn")
    expect(tv.locator(".card.pharm")).to_have_count(0)
    expect(room.locator("#bDonePh")).to_be_hidden()
    expect(room.locator("#bDone")).to_have_text("เสร็จสิ้น")


def test_admin_settings_reach_the_tv(open_page):
    tv = open_page("/display", 1920, 1080)
    a = open_page("/admin")
    login(a)
    a.fill("[data-k=org_name]", "บ้านทดสอบ")
    a.fill("[data-k=ticker]", "")
    a.locator("#screen [data-save]").click()
    expect(tv.locator("#orgName")).to_have_text("บ้านทดสอบ")
    expect(tv.locator("#tick")).to_be_hidden()


def test_admin_media_upload_plays_on_tv(open_page, server, tmp_path):
    tv = open_page("/display", 1920, 1080)
    expect(tv.locator("#placeholder")).to_be_visible()
    a = open_page("/admin")
    login(a)
    img = tmp_path / "poster.png"
    img.write_bytes(png_bytes())
    a.set_input_files("#mediaFiles", str(img))
    a.click("#mediaUpload")
    expect(a.locator("#mediaRows tr[data-i]")).to_have_count(1)
    expect(tv.locator(".layer img")).to_have_count(1)
    expect(tv.locator("#placeholder")).to_be_hidden()


def test_admin_sound_test_shows_on_tv(open_page):
    tv = open_page("/display", 1920, 1080)
    a = open_page("/admin")
    login(a)
    a.click("#testForm button")
    expect(a.locator("#testText")).to_contain_text("ขอเชิญหมายเลข สิบสอง ที่ห้องตรวจ หนึ่ง ค่ะ")
    expect(tv.locator("#cTest")).to_be_visible()
    expect(card_num(tv, 1)).to_have_text("—")  # a test call does not touch the queue


# ---------------------------------------------------------------- layout


def test_staff_pages_fit_a_phone(open_page):
    for path in ["/", "/room", "/room/1", "/manage", "/admin"]:
        page = open_page(path, 390, 844)
        if path == "/admin":
            login(page)
        overflow = page.evaluate("document.documentElement.scrollWidth - window.innerWidth")
        assert overflow <= 1, f"{path} scrolls sideways by {overflow}px"


def test_tv_fills_any_screen_size(open_page):
    for w, h in [(1920, 1080), (1280, 720), (3840, 2160), (1366, 768)]:
        tv = open_page("/display", w, h)
        box = tv.locator("#tv").bounding_box()
        assert abs(box["width"] / box["height"] - 16 / 9) < 0.01
        assert box["width"] <= w + 1 and box["height"] <= h + 1
        assert max(box["width"] / w, box["height"] / h) > 0.99, "stage should touch two edges"


# ---------------------------------------------------------------- Thai text and media sound


def _clipped(page, selector):
    """True when the glyphs (tone marks above, vowels below) reach outside the element's clipping box."""
    return page.locator(selector).first.evaluate(
        """el => {
            const cs = getComputedStyle(el);
            const rect = el.getBoundingClientRect();
            const scale = rect.height / el.offsetHeight;  // the TV stage is CSS-scaled
            const probe = document.createElement('span');
            probe.style.cssText = 'display:inline-block;width:0;height:0;vertical-align:baseline';
            el.appendChild(probe);
            const baseline = (probe.getBoundingClientRect().top - rect.top) / scale;
            probe.remove();
            const ctx = document.createElement('canvas').getContext('2d');
            ctx.font = `${cs.fontWeight} ${cs.fontSize} ${cs.fontFamily}`;
            const m = ctx.measureText(el.textContent);
            return baseline - m.actualBoundingBoxAscent < 0 || baseline + m.actualBoundingBoxDescent > el.offsetHeight;
        }"""
    )


def test_thai_tone_marks_are_not_clipped_on_tv(open_page, server):
    server.api("/admin/login", {"password": "admin"})
    settings = server.api("/admin/settings")
    cfg = settings["config"]
    cfg["org_name"] = "บ้านน้ำมวบ"
    server.api("/admin/config", cfg, method="PUT")
    server.api("/admin/rooms/1", {"name": "ห้องผู้สูงอายุ ปู่ย่า", "kind": "exam", "enabled": True, "voice_phrase": "room", "voice_number": None}, method="PUT")
    server.api("/rooms/1/next", {})
    server.api("/rooms/1/done", {"pharmacy": True})
    tv = open_page("/display", 1920, 1080)
    expect(tv.locator("#orgName")).to_have_text("บ้านน้ำมวบ")
    tv.wait_for_timeout(500)
    assert not _clipped(tv, "#orgName"), "organisation name loses its tone marks"
    assert not _clipped(tv, '.card[data-id="1"] .card-name'), "room name loses its tone marks"
    assert not _clipped(tv, ".card.pharm .card-sub")


def _webm(path):
    if not shutil.which("ffmpeg"):
        pytest.skip("ffmpeg not installed")
    subprocess.run(
        ["ffmpeg", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "color=c=green:s=320x180:d=20",
         "-f", "lavfi", "-i", "sine=frequency=440:duration=20", "-shortest", "-c:v", "libvpx", "-c:a", "libvorbis", str(path)],
        check=True,
    )


def test_media_sound_switch_mutes_one_clip_without_restarting(open_page, tmp_path):
    clip = tmp_path / "clip.webm"
    _webm(clip)
    tv = open_page("/display", 1920, 1080)
    tv.click("body")
    a = open_page("/admin")
    login(a)
    a.set_input_files("#mediaFiles", str(clip))
    a.click("#mediaUpload")
    expect(a.locator("#mediaRows [data-m=sound]")).to_be_checked()
    video = tv.locator(".layer video")
    expect(video).to_have_count(1)
    assert video.evaluate("v => v.muted") is False
    video.evaluate("v => v.dataset.mark = 'same'")

    a.locator("#mediaRows [data-m=sound]").uncheck()
    expect(a.locator(".toast").last).to_have_text("บันทึกรายการสื่อแล้ว")
    tv.wait_for_function("() => document.querySelector('.layer video').muted === true")
    assert video.evaluate("v => v.dataset.mark") == "same", "the clip kept playing; it was not restarted"

    a.locator("#mediaRows [data-m=sound]").check()
    tv.wait_for_function("() => document.querySelector('.layer video').muted === false")


def test_images_have_no_sound_switch(open_page, tmp_path):
    a = open_page("/admin")
    login(a)
    img = tmp_path / "poster.png"
    img.write_bytes(png_bytes())
    a.set_input_files("#mediaFiles", str(img))
    a.click("#mediaUpload")
    expect(a.locator("#mediaRows tr[data-i]")).to_have_count(1)
    expect(a.locator("#mediaRows [data-m=sound]")).to_have_count(0)


def test_long_room_names_fit_when_there_are_many_rooms(open_page, server):
    server.api("/admin/login", {"password": "admin"})
    names = ["ห้องทำแผล", "ห้องส่งเสริมสุขภาพ", "ห้องทันตกรรม", "ห้องส่งเสริมสุขภาพและวางแผนครอบครัว"]
    for name in names:
        server.api("/admin/rooms", {"name": name, "kind": "exam", "enabled": True, "voice_phrase": "room", "voice_number": None})
    for rid in [1, 5, 6, 7, 8]:
        server.api(f"/rooms/{rid}/next", {})
    tv = open_page("/display", 1920, 1080)
    expect(tv.locator(".card")).to_have_count(8)
    tv.wait_for_timeout(500)
    report = tv.evaluate(
        """() => [...document.querySelectorAll('.card')].map(c => {
            const n = c.querySelector('.card-name'), num = c.querySelector('.card-num');
            const cb = c.getBoundingClientRect(), nb = num.getBoundingClientRect();
            return {
                name: n.textContent,
                cut: n.scrollWidth > n.clientWidth + 1 || n.scrollHeight > n.clientHeight + 1,
                numInside: nb.left >= cb.left && nb.right <= cb.right && nb.top >= cb.top && nb.bottom <= cb.bottom + 1,
            };
        })"""
    )
    for r in report:
        assert not r["cut"], f"{r['name']} is cut off"
        assert r["numInside"], f"number spills out of the {r['name']} card"
    for i in range(8):
        sel = f".card >> nth={i} >> .card-name"
        if "wrap" not in (tv.locator(sel).get_attribute("class") or ""):
            assert not _clipped(tv, sel), f"tone marks clipped in card {i}"


def png_from(w, h, pixel):
    """Build an RGB PNG where pixel(x, y) returns (r, g, b)."""
    raw = b"".join(b"\x00" + b"".join(bytes(pixel(x, y)) for x in range(w)) for y in range(h))

    def chunk(tag, data):
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")


def test_logo_upload_trims_paper_and_shows_everywhere(open_page, server, tmp_path):
    # A "scanned page": a green disc with a white centre on white paper, far from the edges.
    def pixel(x, y):
        r2 = (x - 300) ** 2 + (y - 300) ** 2
        if r2 < 40**2:
            return (255, 255, 255)
        return (14, 91, 73) if r2 < 120**2 else (255, 255, 255)

    scan = tmp_path / "scan.png"
    scan.write_bytes(png_from(600, 850, pixel))

    tv = open_page("/display", 1920, 1080)
    expect(tv.locator("#logoDefault")).to_be_visible()
    a = open_page("/admin")
    login(a)
    a.set_input_files("#logoFile", str(scan))
    a.click("#logoUpload")
    expect(a.locator("#logoPreview img")).to_have_count(1)

    expect(tv.locator("#logoImg")).to_be_visible()
    expect(tv.locator("#logoDefault")).to_be_hidden()
    tv.wait_for_function("() => document.getElementById('logoImg').complete && document.getElementById('logoImg').naturalWidth > 0")
    info = tv.evaluate(
        """() => {
            const img = document.getElementById('logoImg');
            const c = document.createElement('canvas'); c.width = img.naturalWidth; c.height = img.naturalHeight;
            const ctx = c.getContext('2d'); ctx.drawImage(img, 0, 0);
            const px = (x, y) => [...ctx.getImageData(x, y, 1, 1).data];
            let minX = c.width, maxX = -1;
            const row = ctx.getImageData(0, c.height / 2, c.width, 1).data;
            for (let x = 0; x < c.width; x++) if (row[x * 4 + 3] > 128) { minX = Math.min(minX, x); maxX = Math.max(maxX, x); }
            return { w: c.width, h: c.height, corner: px(5, 5), ring: px(c.width / 2, Math.round(c.height * 0.2)), centre: px(c.width / 2, c.height / 2), span: (maxX - minX) / c.width };
        }"""
    )
    assert (info["w"], info["h"]) == (512, 512)
    assert info["corner"][3] == 0, "paper around the logo becomes transparent"
    assert info["ring"][3] == 255 and info["ring"][1] > info["ring"][0], "the green disc is kept"
    assert info["centre"][:3] == [255, 255, 255] and info["centre"][3] == 255, "white inside the logo is kept"
    assert info["span"] > 0.9, "the page margins were trimmed away"

    room = open_page("/room/1")
    expect(room.locator(".topbar .brand img")).to_have_count(1)

    src = server.state()["config"]["logo"]
    a.click("#logoReset")
    confirm(a)
    expect(tv.locator("#logoDefault")).to_be_visible()
    expect(tv.locator("#logoImg")).to_be_hidden()
    a.wait_for_timeout(300)
    import urllib.error, urllib.request
    try:
        urllib.request.urlopen(f"{server.url}/media/{src}")
        raise AssertionError("old logo file should be deleted")
    except urllib.error.HTTPError as e:
        assert e.code == 404


def test_logo_upload_rejects_other_files(open_page, tmp_path):
    a = open_page("/admin")
    login(a)
    bad = tmp_path / "logo.gif"
    bad.write_bytes(b"GIF89a")
    a.set_input_files("#logoFile", str(bad))
    a.click("#logoUpload")
    expect(a.locator(".toast.error")).to_have_text("ใช้ได้เฉพาะไฟล์ PNG, JPG หรือ WebP")


def test_tv_theme_can_be_switched_from_admin(open_page, server):
    tv = open_page("/display", 1920, 1080)
    expect(tv.locator("#tv")).to_have_attribute("data-theme", "green")
    green_head = tv.evaluate("getComputedStyle(document.querySelector('.tv-head')).backgroundImage")
    a = open_page("/admin")
    login(a)
    expect(a.locator("input[name=theme][value=green]")).to_be_checked()
    a.check("input[name=theme][value=purple]")
    a.locator("#screen [data-save]").click()
    expect(tv.locator("#tv")).to_have_attribute("data-theme", "purple")
    purple_head = tv.evaluate("getComputedStyle(document.querySelector('.tv-head')).backgroundImage")
    assert green_head == "none" and "gradient" in purple_head
    assert server.state()["config"]["theme"] == "purple"
    a.reload()  # still logged in: the settings load straight away
    expect(a.locator("#app")).to_be_visible()
    expect(a.locator("input[name=theme][value=purple]")).to_be_checked()


def test_theme_preview_link_does_not_change_the_setting(open_page, server):
    tv = open_page("/display?theme=purple", 1920, 1080)
    expect(tv.locator("#tv")).to_have_attribute("data-theme", "purple")
    assert server.state()["config"]["theme"] == "green"


@pytest.mark.parametrize("theme", ["green", "purple", "lavender", "orchid"])
def test_every_theme_renders_readably(open_page, server, theme):
    server.api("/admin/login", {"password": "admin"})
    cfg = server.api("/admin/settings")["config"]
    cfg["theme"] = theme
    server.api("/admin/config", cfg, method="PUT")
    server.api("/rooms/1/next", {})
    tv = open_page("/display", 1920, 1080)
    expect(tv.locator("#tv")).to_have_attribute("data-theme", theme)
    # Room numbers must stand out from their card: compare the text colour with the card's main colour.
    ratio = tv.evaluate(
        """() => {
            const lum = (c) => { const [r, g, b] = c.match(/[\\d.]+/g).slice(0, 3).map(Number).map(v => { v /= 255; return v <= .03928 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4; }); return .2126 * r + .7152 * g + .0722 * b; };
            const card = document.querySelector('.card[data-id="2"]');
            const cs = getComputedStyle(card);
            const bg = cs.backgroundImage !== 'none' ? cs.backgroundImage.match(/rgba?\\([^)]*\\)/)[0] : cs.backgroundColor;
            const fg = getComputedStyle(card.querySelector('.card-name')).color;
            const [a, b] = [lum(fg), lum(bg)].sort((x, y) => y - x);
            return (a + .05) / (b + .05);
        }"""
    )
    assert ratio >= 4.5, f"{theme}: room name contrast {ratio:.1f}"
    assert not _clipped(tv, "#orgName")

"""Browser tests for the four pages. Each test gets a fresh server (see conftest.py)."""

import pathlib
import shutil
import struct
import zlib

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

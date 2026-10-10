// Admin page: settings, rooms, voice files, media playlist, status.
(function () {
  const $ = (id) => document.getElementById(id);
  let C = null;        // full config from the server
  let rooms = [];
  let phrases = [];
  Q.topbar('/admin');

  // ---------------------------------------------------------------- login
  async function start() {
    let s;
    try { s = await Q.api('/admin/session'); } catch (e) { Q.toast(e.message, true); return; }
    if (!s.logged_in) {
      $('login').hidden = false;
      $('app').hidden = true;
      $('pw').focus();
      return;
    }
    $('defaultPw').hidden = !s.default_password;
    // Show the forms only once they hold the saved values, so nothing typed early gets overwritten.
    await loadSettings();
    $('login').hidden = true;
    $('app').hidden = false;
    loadOverview();
    loadVoice();
  }

  $('loginForm').addEventListener('submit', async (e) => {
    e.preventDefault();
    const ok = await Q.act(() => Q.api('/admin/login', { password: $('pw').value }));
    $('pw').value = '';
    if (ok) start();
  });
  $('logout').addEventListener('click', async () => {
    await Q.act(() => Q.api('/admin/logout', {}));
    location.reload();
  });

  // ---------------------------------------------------------------- side nav
  const navLinks = [...document.querySelectorAll('#side a')];
  function markNav() {
    const y = window.scrollY + 120;
    let cur = navLinks[0];
    for (const a of navLinks) {
      const sec = document.querySelector(a.getAttribute('href'));
      if (sec && sec.offsetTop <= y) cur = a;
    }
    navLinks.forEach((a) => a.setAttribute('aria-current', String(a === cur)));
  }
  window.addEventListener('scroll', markNav, { passive: true });

  // ---------------------------------------------------------------- config forms
  const getPath = (o, p) => p.split('.').reduce((x, k) => (x == null ? x : x[k]), o);
  function setPath(o, p, v) {
    const ks = p.split('.');
    const last = ks.pop();
    ks.reduce((x, k) => x[k], o)[last] = v;
  }

  function fillForms() {
    document.querySelectorAll('[data-k]').forEach((el) => {
      const v = getPath(C, el.dataset.k);
      if (el.type === 'checkbox') el.checked = !!v;
      else el.value = v ?? '';
    });
    $('pharmacyOn').checked = C.pharmacy_enabled;
  }

  function readForms() {
    const c = JSON.parse(JSON.stringify(C));
    document.querySelectorAll('[data-k]').forEach((el) => {
      let v;
      if (el.type === 'checkbox') v = el.checked;
      else if (el.type === 'number' || el.dataset.type === 'number') v = Number(el.value);
      else v = el.value;
      setPath(c, el.dataset.k, v);
    });
    c.pharmacy_enabled = $('pharmacyOn').checked;
    return c;
  }

  function apply(data) {
    C = data.config;
    rooms = data.rooms;
    phrases = data.phrases;
    Q.topbar('/admin', C);
    fillForms();
    renderRooms();
    renderMedia();
    renderTestRooms();
  }

  async function loadSettings() {
    const d = await Q.act(() => Q.api('/admin/settings'));
    if (d) apply(d);
  }

  async function saveConfig(c, msg) {
    const d = await Q.act(() => Q.api('/admin/config', c, 'PUT'), msg || 'บันทึกแล้ว');
    if (d) apply(d);
    return d;
  }

  document.querySelectorAll('[data-save]').forEach((b) => b.addEventListener('click', () => {
    const c = readForms();
    if (c.number_max <= c.number_min) return Q.toast('เลขสุดท้ายต้องมากกว่าเลขแรก', true);
    saveConfig(c);
  }));

  $('pharmacyOn').addEventListener('change', (e) => {
    saveConfig({ ...C, pharmacy_enabled: e.target.checked }, e.target.checked ? 'เปิดใช้ห้องยาแล้ว' : 'ปิดห้องยาแล้ว');
  });

  // ---------------------------------------------------------------- rooms
  const phraseOptions = (sel) => phrases.map((p) => `<option value="${p.key}"${p.key === sel ? ' selected' : ''}>${Q.esc(p.text)}</option>`).join('');

  function renderRooms() {
    $('roomRows').innerHTML = rooms.map((r, i) => `<tr data-id="${r.id}">
      <td class="ops"><button class="btn small" data-room="up" ${i === 0 ? 'disabled' : ''} aria-label="เลื่อนขึ้น">↑</button>
        <button class="btn small" data-room="down" ${i === rooms.length - 1 ? 'disabled' : ''} aria-label="เลื่อนลง">↓</button></td>
      <td><input type="text" name="name" value="${Q.esc(r.name)}"><br><small class="hint">/room/${r.id}</small></td>
      <td><select name="kind"><option value="exam"${r.kind === 'exam' ? ' selected' : ''}>ห้องตรวจ</option><option value="pharmacy"${r.kind === 'pharmacy' ? ' selected' : ''}>ห้องยา</option></select></td>
      <td><label class="switch"><input type="checkbox" name="enabled"${r.enabled ? ' checked' : ''} aria-label="เปิดใช้"></label></td>
      <td><select name="phrase">${phraseOptions(r.voice_phrase)}</select></td>
      <td><input type="number" name="vnum" min="1" max="999" value="${r.voice_number ?? ''}"></td>
      <td class="ops"><button class="btn small primary" data-room="save">บันทึก</button>
        <button class="btn small danger" data-room="delete">ลบ</button></td></tr>`).join('');
  }

  $('roomRows').addEventListener('click', async (e) => {
    const b = e.target.closest('[data-room]');
    if (!b) return;
    const tr = b.closest('tr');
    const id = +tr.dataset.id;
    const r = rooms.find((x) => x.id === id);
    let d = null;
    if (b.dataset.room === 'up' || b.dataset.room === 'down') {
      d = await Q.act(() => Q.api(`/admin/rooms/${id}/move`, { up: b.dataset.room === 'up' }));
    } else if (b.dataset.room === 'save') {
      const vn = tr.querySelector('[name=vnum]').value;
      d = await Q.act(() => Q.api(`/admin/rooms/${id}`, {
        name: tr.querySelector('[name=name]').value,
        kind: tr.querySelector('[name=kind]').value,
        enabled: tr.querySelector('[name=enabled]').checked,
        voice_phrase: tr.querySelector('[name=phrase]').value,
        voice_number: vn === '' ? null : Number(vn),
      }, 'PUT'), `บันทึก ${r.name} แล้ว`);
    } else if (b.dataset.room === 'delete') {
      if (!(await Q.confirmBox(`ลบ ${r.name}?`, 'ถ้าแค่ไม่ใช้ชั่วคราว ให้ปิด “เปิดใช้” แทน', 'ลบห้อง', true))) return;
      d = await Q.act(() => Q.api(`/admin/rooms/${id}`, undefined, 'DELETE'), `ลบ ${r.name} แล้ว`);
    }
    if (d) apply(d);
  });

  $('addRoom').addEventListener('submit', async (e) => {
    e.preventDefault();
    const name = $('newName').value.trim();
    const kind = $('newKind').value;
    const m = name.match(/(\d+)\s*$/);
    const d = await Q.act(() => Q.api('/admin/rooms', {
      name, kind, enabled: true,
      voice_phrase: kind === 'pharmacy' ? 'pharmacy' : 'exam',
      voice_number: m ? Number(m[1]) : null,
    }), `เพิ่ม ${name} แล้ว`);
    if (d) { $('newName').value = ''; apply(d); }
  });

  // ---------------------------------------------------------------- test sound
  function renderTestRooms() {
    const sel = $('testRoom');
    const keep = sel.value;
    sel.innerHTML = rooms.map((r) => `<option value="${r.id}">${Q.esc(r.name)}</option>`).join('');
    if (keep) sel.value = keep;
  }
  $('testForm').addEventListener('submit', async (e) => {
    e.preventDefault();
    const d = await Q.act(() => Q.api('/admin/test-call', { room_id: Number($('testRoom').value), number: Number($('testNum').value) }), 'ส่งประกาศทดสอบแล้ว');
    if (d) {
      const c = d.call;
      $('testText').textContent = `“${c.text}” · ${c.clips.length ? 'ใช้ไฟล์เสียง ' + c.clips.length + ' ท่อน' : 'ไฟล์เสียงไม่ครบ จะใช้เสียงของเบราว์เซอร์ (ถ้าเปิดไว้)'}`;
    }
  });

  // ---------------------------------------------------------------- voice files
  async function loadVoice() {
    const d = await Q.act(() => Q.api('/admin/voice'));
    if (!d) return;
    const have = d.lines.filter((l) => l.present).length;
    $('vHave').textContent = have;
    $('vTotal').textContent = d.lines.length;
    const missing = d.lines.filter((l) => !l.present);
    $('vState').innerHTML = missing.length ? '<span class="bad">ยังไม่ครบ</span>' : '<span class="ok">ครบแล้ว</span>';
    $('vMissing').innerHTML = missing.slice(0, 40).map((l) => `<span>${l.name}</span>`).join('') + (missing.length > 40 ? `<span>และอีก ${missing.length - 40} ไฟล์</span>` : '');
    const rows = d.lines.map((l) => `<tr><td><code>${l.name}</code></td><td>${Q.esc(l.text)}</td><td>${l.present ? '<span class="ok">มีแล้ว</span>' : '<span class="bad">ไม่มี</span>'}</td>
      <td>${l.present ? `<button class="btn small" data-vdel="${l.name}">ลบ</button>` : ''}</td></tr>`);
    rows.push(...d.rooms.map((r) => `<tr><td><code>${r.name}</code></td><td>เสียงชื่อห้องทั้งท่อนของ ${Q.esc(r.room)} (ไม่บังคับ)</td><td>${r.present ? '<span class="ok">มีแล้ว</span>' : '—'}</td>
      <td>${r.present ? `<button class="btn small" data-vdel="${r.name}">ลบ</button>` : ''}</td></tr>`));
    $('voiceRows').innerHTML = rows.join('');
  }
  $('voiceRows').addEventListener('click', async (e) => {
    const b = e.target.closest('[data-vdel]');
    if (!b) return;
    if (await Q.act(() => Q.api(`/admin/voice/${b.dataset.vdel}`, undefined, 'DELETE'), 'ลบไฟล์เสียงแล้ว')) loadVoice();
  });
  $('voiceUpload').addEventListener('click', async () => {
    const files = $('voiceFiles').files;
    if (!files.length) return Q.toast('เลือกไฟล์เสียงก่อน', true);
    const fd = new FormData();
    for (const f of files) fd.append('file', f, f.name);
    const d = await Q.act(() => Q.api('/admin/voice', fd));
    if (d) {
      Q.toast(`บันทึก ${d.saved.length} ไฟล์` + (d.skipped.length ? ` · ข้าม ${d.skipped.length} ไฟล์ที่ชื่อไม่ตรงรายการ` : ''), d.skipped.length > 0 && !d.saved.length);
      $('voiceFiles').value = '';
      loadVoice();
    }
  });

  // ---------------------------------------------------------------- media playlist
  const KIND = { youtube: 'YouTube', video: 'วิดีโอ', image: 'รูปภาพ' };
  function ytThumb(url) {
    const m = url.match(/(?:v=|youtu\.be\/|shorts\/|embed\/|live\/)([\w-]{11})/);
    return m ? `https://i.ytimg.com/vi/${m[1]}/default.jpg` : null;
  }
  function renderMedia() {
    const list = C.media || [];
    $('mediaRows').innerHTML = list.map((m, i) => {
      const thumb = m.kind === 'image' ? `/media/${encodeURIComponent(m.src)}` : m.kind === 'youtube' ? ytThumb(m.src) : null;
      return `<tr class="media-row" data-i="${i}">
        <td><div class="thumb">${thumb ? `<img src="${Q.esc(thumb)}" alt="" loading="lazy">` : KIND[m.kind]}</div></td>
        <td><input type="text" data-m="title" value="${Q.esc(m.title || '')}" placeholder="${Q.esc(m.kind === 'youtube' ? m.src : m.src)}"></td>
        <td>${KIND[m.kind] || m.kind}</td>
        <td>${m.kind === 'image' ? `<input type="number" min="3" data-m="seconds" value="${m.seconds ?? ''}" placeholder="${C.image_seconds}">` : '<span class="hint">จนจบ</span>'}</td>
        <td><label class="switch"><input type="checkbox" data-m="enabled"${m.enabled ? ' checked' : ''} aria-label="เปิดใช้"></label></td>
        <td class="ops" style="white-space:nowrap"><button class="btn small" data-mm="up" ${i === 0 ? 'disabled' : ''} aria-label="เลื่อนขึ้น">↑</button>
          <button class="btn small" data-mm="down" ${i === list.length - 1 ? 'disabled' : ''} aria-label="เลื่อนลง">↓</button>
          <button class="btn small danger" data-mm="del">ลบ</button></td></tr>`;
    }).join('') || '<tr><td colspan="6" class="empty" style="padding:16px 18px">ยังไม่มีสื่อ จอจะแสดงชื่อหน่วยงานแทน</td></tr>';
  }

  function mediaFromTable() {
    const list = (C.media || []).map((m) => ({ ...m }));
    document.querySelectorAll('#mediaRows tr[data-i]').forEach((tr) => {
      const m = list[+tr.dataset.i];
      m.title = tr.querySelector('[data-m=title]').value;
      m.enabled = tr.querySelector('[data-m=enabled]').checked;
      const sec = tr.querySelector('[data-m=seconds]');
      if (sec) m.seconds = sec.value === '' ? null : Number(sec.value);
    });
    return list;
  }

  $('mediaRows').addEventListener('change', () => saveConfig({ ...C, media: mediaFromTable() }, 'บันทึกรายการสื่อแล้ว'));
  $('mediaRows').addEventListener('click', async (e) => {
    const b = e.target.closest('[data-mm]');
    if (!b) return;
    const i = +b.closest('tr').dataset.i;
    const list = mediaFromTable();
    if (b.dataset.mm === 'del') {
      const m = list[i];
      if (!(await Q.confirmBox(`ลบ “${m.title || m.src}”?`, m.kind === 'youtube' ? 'ลบลิงก์ออกจากรายการ' : 'ไฟล์จะถูกลบออกจากเครื่องด้วย', 'ลบ', true))) return;
      list.splice(i, 1);
      if (await saveConfig({ ...C, media: list }, 'ลบแล้ว') && m.kind !== 'youtube') {
        Q.api(`/admin/media/${encodeURIComponent(m.src)}`, undefined, 'DELETE').catch(() => {});
      }
      return;
    }
    const j = b.dataset.mm === 'up' ? i - 1 : i + 1;
    [list[i], list[j]] = [list[j], list[i]];
    saveConfig({ ...C, media: list }, 'จัดลำดับแล้ว');
  });

  const newId = () => Date.now().toString(36) + Math.random().toString(36).slice(2, 6);

  $('ytForm').addEventListener('submit', (e) => {
    e.preventDefault();
    const url = $('ytUrl').value.trim();
    if (!/youtu\.?be/.test(url)) return Q.toast('ลิงก์นี้ไม่ใช่ YouTube', true);
    const list = mediaFromTable();
    list.push({ id: newId(), kind: 'youtube', src: url, title: '', seconds: null, enabled: true });
    saveConfig({ ...C, media: list }, 'เพิ่ม YouTube แล้ว').then((ok) => { if (ok) $('ytUrl').value = ''; });
  });

  $('mediaUpload').addEventListener('click', () => {
    const files = $('mediaFiles').files;
    if (!files.length) return Q.toast('เลือกไฟล์ก่อน', true);
    const fd = new FormData();
    for (const f of files) fd.append('file', f, f.name);
    // XMLHttpRequest gives upload progress for large videos.
    const xhr = new XMLHttpRequest();
    xhr.open('POST', '/api/admin/media');
    xhr.upload.onprogress = (ev) => {
      if (ev.lengthComputable) $('uploadState').textContent = `กำลังอัปโหลด ${Math.round((ev.loaded / ev.total) * 100)}%`;
    };
    xhr.onload = async () => {
      let d = null;
      try { d = JSON.parse(xhr.responseText); } catch (e) {}
      if (xhr.status !== 200 || !d || !d.ok) {
        $('uploadState').textContent = '';
        return Q.toast((d && d.error) || 'อัปโหลดไม่สำเร็จ', true);
      }
      const list = mediaFromTable();
      for (const f of d.files) list.push({ id: newId(), kind: f.kind, src: f.src, title: f.title, seconds: null, enabled: true });
      $('mediaFiles').value = '';
      $('uploadState').textContent = '';
      saveConfig({ ...C, media: list }, `อัปโหลด ${d.files.length} ไฟล์แล้ว`);
    };
    xhr.onerror = () => { $('uploadState').textContent = ''; Q.toast('อัปโหลดไม่สำเร็จ', true); };
    xhr.send(fd);
  });

  // ---------------------------------------------------------------- overview
  async function loadOverview() {
    const info = await Q.act(() => Q.api('/info'));
    if (info) {
      $('version').textContent = `เวอร์ชัน ${info.version}`;
      const pages = [['จอแสดงผล', '/display'], ['จัดการคิว', '/manage'], ['ผู้ดูแลระบบ', '/admin']];
      rooms.filter((r) => r.enabled).forEach((r) => pages.splice(pages.length - 2, 0, [r.name, `/room/${r.id}`]));
      $('links').innerHTML = pages.map(([t, p]) => `<div><b>${Q.esc(t)}</b><code>${Q.esc(info.base + p)}</code>
        <button class="btn small" data-copy="${Q.esc(info.base + p)}" type="button">คัดลอก</button></div>`).join('');
    }
    loadStats();
    loadClients();
  }
  $('links').addEventListener('click', (e) => {
    const b = e.target.closest('[data-copy]');
    if (!b) return;
    navigator.clipboard.writeText(b.dataset.copy).then(() => Q.toast('คัดลอกแล้ว'), () => Q.toast('คัดลอกไม่ได้ ให้เลือกข้อความแล้วกด Ctrl+C', true));
  });

  async function loadStats() {
    const s = await Q.act(() => Q.api('/admin/stats'));
    if (!s) return;
    $('statDay').textContent = new Date(s.day + 'T00:00:00').toLocaleDateString('th-TH', { dateStyle: 'long' });
    $('statRows').innerHTML = s.rooms.map((r) => `<tr><td>${Q.esc(r.room_name)}</td><td class="num">${r.calls}</td><td class="num">${r.finished}</td><td class="num">${r.to_pharmacy}</td><td class="num">${r.skipped}</td></tr>`).join('')
      || '<tr><td colspan="5" class="empty">ยังไม่มีการเรียกคิววันนี้</td></tr>';
    const from = 6, to = 20; // clinic hours shown; others summed into the edges
    const hrs = s.hourly;
    const max = Math.max(1, ...hrs);
    $('bars').style.gridTemplateColumns = `repeat(${to - from + 1}, minmax(0, 1fr))`;
    $('hours').style.gridTemplateColumns = $('bars').style.gridTemplateColumns;
    let bars = '', labels = '';
    for (let h = from; h <= to; h++) {
      let v = hrs[h];
      if (h === from) v += hrs.slice(0, from).reduce((a, b) => a + b, 0);
      if (h === to) v += hrs.slice(to + 1).reduce((a, b) => a + b, 0);
      bars += `<div class="${v ? '' : 'zero'}" style="height:${Math.max(2, (v / max) * 100)}%">${v ? `<span>${v}</span>` : ''}</div>`;
      labels += `<span>${String(h).padStart(2, '0')}</span>`;
    }
    $('bars').innerHTML = bars;
    $('hours').innerHTML = labels;
    $('expired').textContent = s.expired ? `วันนี้มีคิวที่ไม่มาและถูกเอาออกอัตโนมัติ ${s.expired} คิว` : '';
  }

  const ROLE = { display: 'จอแสดงผล', room: 'ห้องตรวจ', manage: 'จัดการคิว', admin: 'ผู้ดูแลระบบ' };
  async function loadClients() {
    const d = await Q.act(() => Q.api('/admin/clients'));
    if (!d) return;
    $('dataDir').textContent = d.data_dir;
    $('voiceDir').textContent = `โฟลเดอร์ ${d.voice_dir}`;
    $('clientRows').innerHTML = d.clients.map((c) => `<tr><td>${ROLE[c.role] || Q.esc(c.role)}</td><td>${c.room ? Q.esc((rooms.find((r) => r.id === c.room) || {}).name || c.room) : '—'}</td>
      <td><code>${Q.esc(c.ip)}</code></td><td>${Q.hhmm(c.since)} น.</td></tr>`).join('') || '<tr><td colspan="4" class="empty">ไม่มี</td></tr>';
  }
  $('refreshClients').addEventListener('click', () => { loadClients(); loadStats(); });

  // ---------------------------------------------------------------- password
  $('pwForm').addEventListener('submit', async (e) => {
    e.preventDefault();
    const ok = await Q.act(() => Q.api('/admin/password', { current: $('pwOld').value, new: $('pwNew').value }), 'เปลี่ยนรหัสผ่านแล้ว');
    if (ok) { $('pwOld').value = ''; $('pwNew').value = ''; $('defaultPw').hidden = true; }
  });

  Q.connect({ role: 'admin' }, { status: Q.setConn });
  start();
})();

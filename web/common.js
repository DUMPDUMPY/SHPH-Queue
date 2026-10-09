// Shared helpers for every page: API calls, live connection, Thai numbers, small UI bits.
(function () {
  const D = ['ศูนย์', 'หนึ่ง', 'สอง', 'สาม', 'สี่', 'ห้า', 'หก', 'เจ็ด', 'แปด', 'เก้า'];

  function thaiNum(n) {
    if (n === 0) return D[0];
    const h = Math.floor(n / 100), t = Math.floor((n % 100) / 10), u = n % 10;
    let s = '';
    if (h) s += D[h] + 'ร้อย';
    if (t) s += t === 1 ? 'สิบ' : t === 2 ? 'ยี่สิบ' : D[t] + 'สิบ';
    if (u) s += (u === 1 && (t || h)) ? 'เอ็ด' : D[u];
    return s;
  }

  async function api(path, body, method) {
    const opts = { method: method || (body === undefined ? 'GET' : 'POST'), headers: {}, credentials: 'same-origin' };
    if (body !== undefined && !(body instanceof FormData)) {
      opts.headers['Content-Type'] = 'application/json';
      opts.body = JSON.stringify(body);
    } else if (body instanceof FormData) {
      opts.body = body;
    }
    let res;
    try {
      res = await fetch('/api' + path, opts);
    } catch (e) {
      throw new Error('ติดต่อเครื่องแม่ข่ายไม่ได้ ตรวจสอบสาย LAN หรือว่าโปรแกรมยังเปิดอยู่');
    }
    let data = null;
    try { data = await res.json(); } catch (e) { /* not JSON */ }
    if (!res.ok || (data && data.ok === false)) {
      const err = new Error((data && data.error) || `เกิดข้อผิดพลาด (${res.status})`);
      err.status = res.status;
      throw err;
    }
    return data;
  }

  // Live connection with automatic reconnect.
  function connect(params, handlers) {
    let ws = null, wait = 1000, closed = false;
    const qs = new URLSearchParams(params).toString();
    function open() {
      const proto = location.protocol === 'https:' ? 'wss' : 'ws';
      ws = new WebSocket(`${proto}://${location.host}/ws?${qs}`);
      ws.onopen = () => { wait = 1000; handlers.status && handlers.status(true); };
      ws.onmessage = (ev) => {
        let msg; try { msg = JSON.parse(ev.data); } catch (e) { return; }
        if (msg.t === 'state') {
          Q.clockOffset = msg.state.now - Date.now() / 1000;
          handlers.state && handlers.state(msg.state);
        } else if (msg.t === 'call') handlers.call && handlers.call(msg);
      };
      ws.onclose = () => {
        handlers.status && handlers.status(false);
        if (!closed) setTimeout(open, wait);
        wait = Math.min(wait * 1.6, 8000);
      };
      ws.onerror = () => { try { ws.close(); } catch (e) {} };
    }
    open();
    return { close() { closed = true; ws && ws.close(); } };
  }

  const nowSec = () => Date.now() / 1000 + (Q.clockOffset || 0);
  const hhmm = (ts) => ts ? new Date(ts * 1000).toLocaleTimeString('th-TH', { hour: '2-digit', minute: '2-digit', hour12: false }) : '';
  function ago(ts) {
    const m = Math.max(0, Math.floor((nowSec() - ts) / 60));
    if (m < 1) return 'เมื่อสักครู่';
    if (m < 60) return `${m} นาที`;
    return `${Math.floor(m / 60)} ชม. ${m % 60} นาที`;
  }
  const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));

  function toast(msg, isError) {
    let wrap = document.querySelector('.toast-wrap');
    if (!wrap) { wrap = document.createElement('div'); wrap.className = 'toast-wrap'; wrap.setAttribute('role', 'status'); document.body.appendChild(wrap); }
    const t = document.createElement('div');
    t.className = 'toast' + (isError ? ' error' : '');
    t.textContent = msg;
    wrap.appendChild(t);
    setTimeout(() => t.remove(), isError ? 5000 : 3000);
  }

  // In-page confirmation (the browser's confirm() is easy to click through by habit).
  function confirmBox(title, text, okLabel, danger) {
    return new Promise((resolve) => {
      const el = document.createElement('div');
      el.className = 'confirm';
      el.innerHTML = `<div class="box" role="dialog" aria-modal="true"><h3></h3><p></p><div class="row">
        <button class="btn" data-a="no" type="button">ยกเลิก</button>
        <button class="btn ${danger ? 'danger' : 'primary'}" data-a="yes" type="button"></button></div></div>`;
      el.querySelector('h3').textContent = title;
      el.querySelector('p').textContent = text;
      el.querySelector('[data-a=yes]').textContent = okLabel || 'ยืนยัน';
      const done = (v) => { el.remove(); document.removeEventListener('keydown', key); resolve(v); };
      const key = (e) => { if (e.key === 'Escape') done(false); };
      el.addEventListener('click', (e) => {
        if (e.target === el) done(false);
        const a = e.target.closest('[data-a]');
        if (a) done(a.dataset.a === 'yes');
      });
      document.addEventListener('keydown', key);
      document.body.appendChild(el);
      el.querySelector('[data-a=yes]').focus();
    });
  }

  // Run an action once at a time; show its error.
  async function act(fn, okMsg) {
    try {
      const r = await fn();
      if (okMsg) toast(okMsg);
      return r;
    } catch (e) {
      toast(e.message, true);
      return null;
    }
  }

  const LOGO = '<svg viewBox="0 0 72 72" aria-hidden="true"><rect x="2" y="2" width="68" height="68" rx="20" fill="#0E5B49"/><path d="M29 16h14v13h13v14H43v13H29V43H16V29h13z" fill="#fff"/></svg>';

  let topbarKey = null;
  function topbar(current, org) {
    const key = current + '|' + (org ? org.org_prefix + org.org_name : '');
    if (key === topbarKey) return;
    const wasOn = topbarKey !== null && !document.getElementById('conn')?.classList.contains('off');
    topbarKey = key;
    const links = [['/', 'หน้าแรก'], ['/display', 'จอแสดงผล'], ['/room', 'ห้องตรวจ'], ['/manage', 'จัดการคิว'], ['/admin', 'ผู้ดูแลระบบ']];
    const el = document.querySelector('.topbar');
    if (!el) return;
    el.innerHTML = `<a class="brand" href="/">${LOGO}<span><small>${esc(org?.org_prefix || 'ระบบเรียกคิว')}</small><b>${esc(org?.org_name || 'รพ.สต.')}</b></span></a>
      <nav>${links.map(([h, t]) => `<a href="${h}"${h === current ? ' aria-current="page"' : ''}>${t}</a>`).join('')}</nav>
      <span class="conn off" id="conn">กำลังเชื่อมต่อ…</span>`;
    if (wasOn || connected) setConn(true);
  }
  let connected = false;
  function setConn(on) {
    connected = on;
    const c = document.getElementById('conn');
    if (!c) return;
    c.classList.toggle('off', !on);
    c.textContent = on ? 'เชื่อมต่อแล้ว' : 'ขาดการเชื่อมต่อ กำลังลองใหม่…';
  }

  const ACTIONS = {
    call: 'เรียก', call_manual: 'เรียกเลขที่ระบุ', recall: 'เรียกกลับจากพักคิว', repeat: 'เรียกซ้ำ',
    skip: 'ไม่มา · พักคิว', done: 'เสร็จ · ไม่รับยา', to_pharmacy: 'เสร็จ · ส่งห้องยา', dispensed: 'จ่ายยาแล้ว',
    auto_done: 'เสร็จ (เรียกคิวถัดไป)', undo: 'ย้อนกลับ', expire: 'ลบจากพักคิว (หมดเวลา)', remove_held: 'ลบจากพักคิว',
    remove_waiting: 'ลบจากคิวรอรับยา', set_last: 'ตั้งเลขล่าสุด', reset: 'เริ่มคิวใหม่',
  };

  window.Q = { thaiNum, api, connect, hhmm, ago, esc, toast, confirmBox, act, topbar, setConn, nowSec, LOGO, ACTIONS, clockOffset: 0 };
})();

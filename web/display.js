// TV display: room numbers, announcements, and the media playlist.
(function () {
  const $ = (id) => document.getElementById(id);
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  let S = null;          // latest state from the server
  let cfg = null;        // S.config
  let hotRoom = null;    // room that called most recently

  // ---------------------------------------------------------------- stage fit
  const tv = $('tv');
  function fit() {
    const w = window.innerWidth, h = window.innerHeight;
    const s = Math.min(w / 1920, h / 1080);
    tv.style.transform = `translate(${(w - 1920 * s) / 2}px, ${(h - 1080 * s) / 2}px) scale(${s})`;
  }
  window.addEventListener('resize', fit);
  fit();

  // ---------------------------------------------------------------- clock
  function tick() {
    const d = new Date(Date.now() + Q.clockOffset * 1000);
    $('tvDate').textContent = d.toLocaleDateString('th-TH', { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' });
    const hm = d.toLocaleTimeString('th-TH', { hour: '2-digit', minute: '2-digit', hour12: false });
    $('tvClock').innerHTML = `${hm}<span>:${String(d.getSeconds()).padStart(2, '0')}</span>`;
  }
  tick();
  setInterval(tick, 1000);

  // ---------------------------------------------------------------- rooms
  let cardsKey = '';
  function activeRooms() {
    return S.rooms.filter((r) => r.active);
  }
  function buildCards() {
    const rooms = activeRooms();
    const key = JSON.stringify(rooms.map((r) => [r.id, r.name, r.kind]));
    if (key === cardsKey) return;
    cardsKey = key;
    const el = $('tvRight');
    el.innerHTML = '';
    const groups = [
      ['ห้องตรวจ', rooms.filter((r) => r.kind === 'exam')],
      ['รับยา', rooms.filter((r) => r.kind === 'pharmacy')],
    ].filter(([, list]) => list.length);
    // Size cards so every room fits the column.
    const total = groups.reduce((n, [, l]) => n + l.length, 0);
    const twoCols = total > 5;
    const rowsOf = (n) => (twoCols ? Math.ceil(n / 2) : n);
    const rows = groups.reduce((n, [, l]) => n + rowsOf(l.length), 0);
    const avail = 1080 - 124 - 68 - 48 - groups.length * 48 - Math.max(0, rows - groups.length) * 14;
    const h = Math.max(80, avail / Math.max(rows, 1));
    const numSize = Math.min(twoCols ? 110 : 190, h * 0.76);
    const nameSize = Math.min(twoCols ? 34 : 52, h * 0.24);
    const subSize = Math.min(24, h * 0.15);
    for (const [title, list] of groups) {
      const g = document.createElement('div');
      g.className = 'grp';
      g.textContent = title;
      el.appendChild(g);
      const box = document.createElement('div');
      box.className = 'cards';
      box.style.gridTemplateColumns = twoCols && list.length > 1 ? '1fr 1fr' : '1fr';
      box.style.flex = String(rowsOf(list.length));
      box.style.gridAutoRows = '1fr';
      for (const r of list) {
        const c = document.createElement('div');
        c.className = 'card' + (r.kind === 'pharmacy' ? ' pharm' : '');
        c.dataset.id = r.id;
        c.style.setProperty('--num-size', numSize + 'px');
        c.style.setProperty('--name-size', nameSize + 'px');
        c.style.setProperty('--sub-size', subSize + 'px');
        c.innerHTML = '<div><div class="card-name"></div><div class="card-sub"></div></div><div class="card-num"></div>';
        c.querySelector('.card-name').textContent = r.name;
        box.appendChild(c);
      }
      el.appendChild(box);
    }
  }

  function chips(el, nums, firstCls) {
    el.innerHTML = '';
    if (!nums.length) {
      el.innerHTML = '<span class="none">ไม่มี</span>';
      return;
    }
    nums.forEach((n, i) => {
      const c = document.createElement('span');
      c.className = 'chip' + (i === 0 && firstCls ? ' ' + firstCls : '');
      c.textContent = n;
      el.appendChild(c);
    });
  }

  const PER_PAGE = 5;
  let heldPage = 0;
  function renderHeld() {
    const nums = S.held.map((h) => h.number);
    const pages = Math.max(1, Math.ceil(nums.length / PER_PAGE));
    if (heldPage >= pages) heldPage = 0;
    chips($('heldTv'), nums.slice(heldPage * PER_PAGE, (heldPage + 1) * PER_PAGE));
    $('heldMeta').textContent = nums.length > PER_PAGE ? `${nums.length} คิว · หน้า ${heldPage + 1}/${pages}` : nums.length ? `${nums.length} คิว` : '';
  }
  setInterval(() => {
    if (!S) return;
    const pages = Math.ceil(S.held.length / PER_PAGE);
    if (pages < 2) return;
    heldPage = (heldPage + 1) % pages;
    renderHeld();
    const el = $('heldTv');
    el.classList.remove('flip');
    void el.offsetWidth;
    el.classList.add('flip');
  }, 5000);

  function render() {
    buildCards();
    document.querySelectorAll('.card').forEach((c) => {
      const r = S.rooms.find((x) => x.id === +c.dataset.id);
      if (!r) return;
      const idle = r.current == null;
      c.classList.toggle('idle', idle);
      c.classList.toggle('hot', r.id === hotRoom && !idle);
      c.querySelector('.card-num').textContent = idle ? '—' : r.current;
      c.querySelector('.card-sub').textContent =
        r.kind === 'pharmacy'
          ? S.pharmacy.length ? `รอรับยา ${S.pharmacy.slice(0, 6).map((w) => w.number).join(' · ')}` : 'ไม่มีคิวรอรับยา'
          : idle ? 'ว่าง' : 'กำลังเรียก';
    });
    $('upStrip').hidden = !S.upcoming.length;
    $('strips').classList.toggle('solo', !S.upcoming.length);
    chips($('upNext'), S.upcoming, 'first');
    renderHeld();
  }

  let tickerText = null;
  function applyConfig() {
    $('orgPrefix').textContent = cfg.org_prefix;
    $('orgName').textContent = cfg.org_name;
    $('phKicker').textContent = `ยินดีต้อนรับสู่${cfg.org_prefix}`;
    $('phName').textContent = cfg.org_name;
    document.title = `จอแสดงผลคิว · ${cfg.org_name}`;
    if (cfg.ticker !== tickerText) {
      tickerText = cfg.ticker;
      const t = (cfg.ticker || '').trim();
      $('tick').hidden = !t;
      tv.classList.toggle('no-tick', !t);
      $('tickText').textContent = t;
      $('tickText').style.setProperty('--run-time', Math.max(20, t.length * 0.32) + 's');
    }
    media.setItems(cfg.media || []);
  }

  // ---------------------------------------------------------------- media playlist
  const media = (() => {
    const box = $('media');
    let items = [], key = '', idx = 0, cur = null, timer = null, ytReady = null;
    const BASE_VOL = 100, DUCK_VOL = 12;
    let ducked = false;

    function loadYT() {
      if (ytReady) return ytReady;
      ytReady = new Promise((resolve, reject) => {
        window.onYouTubeIframeAPIReady = () => resolve(window.YT);
        const s = document.createElement('script');
        s.src = 'https://www.youtube.com/iframe_api';
        s.onerror = () => { ytReady = null; reject(new Error('โหลด YouTube ไม่ได้')); };
        document.head.appendChild(s);
      });
      return ytReady;
    }

    function parseYT(url) {
      try {
        const u = new URL(url);
        let id = null;
        if (u.hostname.includes('youtu.be')) id = u.pathname.slice(1);
        else if (/^\/(shorts|embed|live)\//.test(u.pathname)) id = u.pathname.split('/')[2];
        else id = u.searchParams.get('v');
        return { id, list: u.searchParams.get('list') };
      } catch (e) {
        return { id: /^[\w-]{11}$/.test(url) ? url : null, list: null };
      }
    }

    function clear() {
      clearTimeout(timer);
      timer = null;
    }

    function swapIn(layer) {
      box.insertBefore(layer, $('unlock'));
      const old = cur;
      requestAnimationFrame(() => requestAnimationFrame(() => layer.classList.add('on')));
      if (old) setTimeout(() => {
        try { old.yt && old.yt.destroy(); } catch (e) {}
        old.el.remove();
      }, 900);
    }

    function applyMute(item) {
      cur.item = item;
      try { if (cur.yt) { if (item.muted) cur.yt.mute(); else cur.yt.unMute(); } } catch (e) {}
      if (cur.video) cur.video.muted = !!item.muted;
    }

    function next(delay) {
      clear();
      timer = setTimeout(() => {
        if (!items.length) return;
        idx = (idx + 1) % items.length;
        play();
      }, delay || 0);
    }

    function play() {
      clear();
      $('placeholder').hidden = items.length > 0;
      if (!items.length) {
        if (cur) { cur.el.remove(); cur = null; }
        return;
      }
      const item = items[idx];
      const layer = document.createElement('div');
      layer.className = 'layer';
      const entry = { el: layer, item, yt: null, video: null };
      const single = items.length === 1;

      if (item.kind === 'image') {
        const img = document.createElement('img');
        img.src = '/media/' + encodeURIComponent(item.src);
        img.alt = item.title || '';
        img.onerror = () => next(2000);
        layer.appendChild(img);
        if (!single) timer = setTimeout(() => next(), (item.seconds || cfg.image_seconds || 10) * 1000);
      } else if (item.kind === 'video') {
        const v = document.createElement('video');
        v.src = '/media/' + encodeURIComponent(item.src);
        v.autoplay = true;
        v.playsInline = true;
        v.loop = single;
        v.volume = (ducked ? DUCK_VOL : BASE_VOL) / 100;
        v.muted = !!item.muted;
        v.onended = () => next();
        v.onerror = () => next(2000);
        layer.appendChild(v);
        entry.video = v;
        v.play().catch(() => {
          if (item.muted) return;
          v.muted = true;
          v.play().catch(() => {});
          audio.needUnlock();
        });
      } else if (item.kind === 'youtube') {
        const { id, list } = parseYT(item.src);
        const holder = document.createElement('div');
        layer.appendChild(holder);
        loadYT().then((YT) => {
          const vars = { autoplay: 1, controls: 0, rel: 0, modestbranding: 1, playsinline: 1, iv_load_policy: 3, disablekb: 1, mute: item.muted ? 1 : 0 };
          if (list) { vars.listType = 'playlist'; vars.list = list; if (single) vars.loop = 1; }
          entry.yt = new YT.Player(holder, {
            videoId: id || undefined,
            playerVars: vars,
            events: {
              onReady: (e) => {
                e.target.setVolume(ducked ? DUCK_VOL : BASE_VOL);
                if (item.muted) e.target.mute();
                e.target.playVideo();
                if (item.muted) return;
                setTimeout(() => {
                  try {
                    if (e.target.getPlayerState() !== YT.PlayerState.PLAYING) { e.target.mute(); e.target.playVideo(); audio.needUnlock(); }
                  } catch (err) {}
                }, 2500);
              },
              onStateChange: (e) => {
                if (e.data !== YT.PlayerState.ENDED) return;
                if (list) {
                  const pl = e.target.getPlaylist() || [];
                  if (e.target.getPlaylistIndex() < pl.length - 1) return;
                }
                if (single) { e.target.seekTo(0); e.target.playVideo(); } else next();
              },
              onError: () => next(3000),
            },
          });
        }).catch(() => next(5000));
      } else {
        next(0);
        return;
      }
      swapIn(layer);
      cur = entry;
    }

    return {
      setItems(list) {
        // A sound on/off change alone should not restart the playlist.
        const k = JSON.stringify(list.map(({ muted, ...rest }) => rest));
        if (k === key) {
          items = list.slice();
          if (cur) applyMute(items.find((m) => m.id === cur.item.id) || cur.item);
          return;
        }
        key = k;
        items = list.slice();
        idx = 0;
        play();
      },
      duck(on) {
        ducked = on;
        if (!cur) return;
        const vol = on ? DUCK_VOL : BASE_VOL;
        try { cur.yt && cur.yt.setVolume(vol); } catch (e) {}
        if (cur.video) cur.video.volume = vol / 100;
      },
      unmute() {
        if (!cur || cur.item.muted) return;
        try { if (cur.yt) { cur.yt.unMute(); cur.yt.playVideo(); } } catch (e) {}
        if (cur.video) { cur.video.muted = false; cur.video.play().catch(() => {}); }
      },
    };
  })();

  // ---------------------------------------------------------------- announcement audio
  const audio = (() => {
    let ac = null;
    const cache = new Map();
    let thVoice = null;

    function ctx() {
      if (!ac) {
        try { ac = new (window.AudioContext || window.webkitAudioContext)(); } catch (e) { ac = null; }
      }
      return ac;
    }
    function pickVoice() {
      try {
        const th = speechSynthesis.getVoices().filter((v) => /^th/i.test(v.lang));
        thVoice = th.find((v) => /premwadee|achara|kanya|narisa|female|หญิง/i.test(v.name)) || th[0] || null;
      } catch (e) { thVoice = null; }
    }
    if (window.speechSynthesis) {
      pickVoice();
      try { speechSynthesis.addEventListener('voiceschanged', pickVoice); } catch (e) {}
    }

    // Cut the silence most recordings start and end with, so joined clips sound like one sentence.
    function trim(buf) {
      const a = ctx(), sr = buf.sampleRate, n = buf.length;
      let start = n, end = 0;
      for (let c = 0; c < buf.numberOfChannels; c++) {
        const d = buf.getChannelData(c);
        let i = 0;
        while (i < n && Math.abs(d[i]) < 0.02) i++;
        let j = n - 1;
        while (j > i && Math.abs(d[j]) < 0.02) j--;
        start = Math.min(start, i);
        end = Math.max(end, j);
      }
      if (end <= start) return buf;
      start = Math.max(0, start - Math.floor(sr * 0.02));
      end = Math.min(n, end + Math.floor(sr * 0.06));
      const out = a.createBuffer(buf.numberOfChannels, end - start, sr);
      for (let c = 0; c < buf.numberOfChannels; c++) out.copyToChannel(buf.getChannelData(c).subarray(start, end), c);
      return out;
    }

    function load(url) {
      if (cache.has(url)) return cache.get(url);
      const p = fetch(url)
        .then((r) => { if (!r.ok) throw new Error('missing'); return r.arrayBuffer(); })
        .then((b) => new Promise((res, rej) => ctx().decodeAudioData(b, res, rej)))
        .then(trim);
      cache.set(url, p);
      p.catch(() => cache.delete(url));
      return p;
    }

    async function clips(urls, vol) {
      const a = ctx();
      if (!a) throw new Error('no audio');
      const bufs = await Promise.all(urls.map(load));
      const g = a.createGain();
      g.gain.value = vol;
      g.connect(a.destination);
      let t = a.currentTime + 0.05;
      for (const b of bufs) {
        const s = a.createBufferSource();
        s.buffer = b;
        s.connect(g);
        s.start(t);
        t += b.duration;
      }
      await wait((t - a.currentTime) * 1000 + 80);
    }

    function chime(vol) {
      const a = ctx();
      if (!a) return Promise.resolve();
      const t = a.currentTime + 0.02;
      [[659.25, 0], [523.25, 0.45]].forEach(([f, d]) => {
        const o = a.createOscillator(), g = a.createGain();
        o.type = 'sine';
        o.frequency.value = f;
        g.gain.setValueAtTime(0.0001, t + d);
        g.gain.exponentialRampToValueAtTime(0.35 * vol + 0.0001, t + d + 0.02);
        g.gain.exponentialRampToValueAtTime(0.0001, t + d + 1.1);
        o.connect(g).connect(a.destination);
        o.start(t + d);
        o.stop(t + d + 1.2);
      });
      return wait(1300);
    }

    function tts(text, vol) {
      return new Promise((res) => {
        if (!window.speechSynthesis || !thVoice) return res();
        let fin = false;
        const end = () => { if (!fin) { fin = true; res(); } };
        try {
          const u = new SpeechSynthesisUtterance(text);
          u.lang = 'th-TH';
          u.voice = thVoice;
          u.rate = 0.9;
          u.volume = vol;
          u.onend = end;
          u.onerror = end;
          speechSynthesis.speak(u);
        } catch (e) { end(); }
        setTimeout(end, 10000);
      });
    }

    async function speak(call, sound) {
      const vol = (sound.volume ?? 100) / 100;
      if (call.clips && call.clips.length) {
        try { await clips(call.clips, vol); return; } catch (e) { /* fall through */ }
      }
      if (sound.browser_tts_fallback) await tts(call.text, vol);
    }

    let unlockShown = false;
    function needUnlock() {
      if (unlockShown) return;
      unlockShown = true;
      $('unlock').hidden = false;
    }
    function unlock() {
      const a = ctx();
      if (a && a.state !== 'running') a.resume().catch(() => {});
      try { if (window.speechSynthesis) { const u = new SpeechSynthesisUtterance(' '); u.volume = 0; speechSynthesis.speak(u); } } catch (e) {}
      media.unmute();
      $('unlock').hidden = true;
      unlockShown = false;
    }
    ['pointerdown', 'keydown', 'touchstart'].forEach((ev) => window.addEventListener(ev, unlock, { passive: true }));
    // Browsers without autoplay permission start the audio context suspended.
    setTimeout(() => {
      const a = ctx();
      if (a && a.state !== 'running') a.resume().then(() => { if (a.state !== 'running') needUnlock(); }).catch(needUnlock);
      setTimeout(() => { if (a && a.state !== 'running') needUnlock(); }, 800);
    }, 1200);

    return { speak, chime, needUnlock, ready: () => !ac || ac.state === 'running' || !!thVoice };
  })();

  // ---------------------------------------------------------------- announcement queue
  const calls = [];
  let busy = false;
  function enqueue(c) {
    if (calls.some((x) => x.room_id === c.room_id && x.number === c.number)) return;
    if (calls.length >= 8) calls.shift();
    calls.push(c);
    if (!busy) runCalls();
  }
  function flash(id) {
    const c = document.querySelector(`.card[data-id="${id}"]`);
    if (!c) return;
    c.classList.remove('flash');
    void c.offsetWidth;
    c.classList.add('flash');
  }
  async function runCalls() {
    busy = true;
    while (calls.length) {
      const c = calls.shift();
      // A call that waited behind others may be stale (the room already skipped or finished it).
      if (!c.test && S) {
        const r = S.rooms.find((x) => x.id === c.room_id);
        if (!r || r.current !== c.number) continue;
      }
      $('cNum').textContent = c.number;
      $('cRoom').textContent = c.kind === 'pharmacy' ? `รับยาที่ ${c.room_name}` : `ที่ ${c.room_name}`;
      $('cTest').hidden = !c.test;
      $('callout').classList.add('on');
      if (!c.test) {
        hotRoom = c.room_id;
        if (S) render();
        flash(c.room_id);
      }
      media.duck(true);
      const t0 = Date.now();
      const sound = cfg.sound;
      if (sound.enabled) {
        for (let i = 0; i < sound.repeat; i++) {
          if (sound.chime) await audio.chime((sound.volume ?? 100) / 100);
          await audio.speak(c, sound);
          if (i < sound.repeat - 1) await wait(sound.gap_seconds * 1000);
        }
      }
      const min = (cfg.callout_seconds || 5) * 1000;
      const spent = Date.now() - t0;
      if (spent < min) await wait(min - spent);
      media.duck(false);
      $('callout').classList.remove('on');
      await wait(450);
    }
    busy = false;
  }

  // ---------------------------------------------------------------- live data
  Q.connect({ role: 'display' }, {
    state(st) {
      S = st;
      cfg = st.config;
      applyConfig();
      render();
    },
    call(c) {
      if (cfg) enqueue(c);
    },
    status(on) {
      $('offline').hidden = on;
    },
  });

  // Keep the screen awake where the browser allows it.
  async function wake() {
    try { if (navigator.wakeLock) await navigator.wakeLock.request('screen'); } catch (e) {}
  }
  document.addEventListener('visibilitychange', () => { if (document.visibilityState === 'visible') wake(); });
  wake();
})();

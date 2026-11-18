const IMG = { home:'img/home.png', library:'img/library.png', machines:'img/machines.png', history:'img/history.png', settings:'img/settings.png' };

  // clickable app: all page images first, then click areas over the sidebar rows
  const PAGES = [['home','Home',87],['library','Library',123],['machines','Machines',196],['history','History',232],['settings','Settings',268]];
  const app = document.getElementById('app'), imgs = {}, hots = {};
  const show = function (id) { Object.keys(imgs).forEach(function (k) { imgs[k].classList.toggle('on', k === id); hots[k].setAttribute('aria-pressed', String(k === id)); }); };
  PAGES.forEach(function (p) { const im = document.createElement('img'); im.src = IMG[p[0]]; im.alt = p[1] + ' page of the Brisk app'; im.draggable = false; imgs[p[0]] = im; app.appendChild(im); });
  PAGES.forEach(function (p) { const h = document.createElement('button'); h.className = 'hot'; h.type = 'button'; h.title = p[1]; h.setAttribute('aria-label', p[1]); h.style.top = ((p[2] - 18) / 900 * 100) + '%'; h.onclick = function () { show(p[0]); }; hots[p[0]] = h; app.appendChild(h); });
  show('home');

  // crops of the real screenshots (source is 1440 x 900)
  document.querySelectorAll('[data-crop]').forEach(function (c) {
    const x = +c.dataset.x, y = +c.dataset.y, w = +c.dataset.w, h = +c.dataset.h;
    c.style.aspectRatio = w + '/' + h;
    const im = document.createElement('img'); im.src = IMG[c.dataset.crop]; im.alt = ''; im.setAttribute('aria-hidden', 'true');
    im.style.width = (1440 / w * 100) + '%'; im.style.left = (-x / w * 100) + '%'; im.style.top = (-y / h * 100) + '%'; im.style.height = (900 / h * 100) + '%';
    c.appendChild(im);
  });

  // tagline: words brighten one at a time as they cross the middle of the screen
  (function () {
    const el = document.getElementById('tagline'), words = el.textContent.trim().split(/\s+/);
    el.setAttribute('aria-label', words.join(' ')); el.textContent = '';
    const spans = words.map(function (w, i) { const s = document.createElement('span'); s.className = 'w'; s.setAttribute('aria-hidden', 'true'); s.textContent = w; el.appendChild(s); if (i < words.length - 1) el.appendChild(document.createTextNode(' ')); return s; });
    if (matchMedia('(prefers-reduced-motion: reduce)').matches || !('IntersectionObserver' in window)) { spans.forEach(function (s) { s.classList.add('on'); }); return; }
    const io = new IntersectionObserver(function (es) { es.forEach(function (e) { if (e.isIntersecting || e.boundingClientRect.top < 0) { e.target.classList.add('on'); io.unobserve(e.target); } }); }, { rootMargin: '0px 0px -45% 0px', threshold: 0 });
    spans.forEach(function (s) { io.observe(s); });
  })();

  // mobile menu
  (function () {
    const b = document.getElementById('menubtn'), s = document.getElementById('sheet');
    const set = function (open) { s.classList.toggle('open', open); s.setAttribute('aria-hidden', String(!open)); b.setAttribute('aria-expanded', String(open)); b.setAttribute('aria-label', open ? 'Close menu' : 'Open menu'); b.querySelector('use').setAttribute('href', open ? '#i-x' : '#i-list'); document.body.style.overflow = open ? 'hidden' : ''; };
    b.addEventListener('click', function () { set(!s.classList.contains('open')); });
    s.querySelectorAll('a').forEach(function (a) { a.addEventListener('click', function () { set(false); }); });
    document.addEventListener('keydown', function (e) { if (e.key === 'Escape') set(false); });
  })();

  document.getElementById('finalcta').addEventListener('click', function () { document.querySelector('.cmd button').click(); });

  // copy button: idle, done, error
  document.querySelectorAll('.cmd button').forEach(function (b) {
    const label = b.querySelector('span'), icon = b.querySelector('use');
    const flash = function (state, text, ic) { b.dataset.state = state; label.textContent = text; icon.setAttribute('href', ic); setTimeout(function () { b.dataset.state = 'idle'; label.textContent = 'Copy'; icon.setAttribute('href', '#i-copy'); }, 1600); };
    b.addEventListener('click', function () {
      const t = b.parentElement.querySelector('.t').textContent.replace(/^\$\s*/, '');
      try { navigator.clipboard.writeText(t).then(function () { flash('done', 'Copied', '#i-check'); }, function () { flash('error', 'Select it', '#i-copy'); }); } catch (e) { flash('error', 'Select it', '#i-copy'); }
    });
  });

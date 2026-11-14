(function () {
  var root = document.documentElement, btn = document.getElementById('theme');
  var modes = ['auto', 'light', 'dark'];
  function apply(m) {
    if (m === 'auto') root.removeAttribute('data-theme'); else root.setAttribute('data-theme', m);
    btn.textContent = 'Theme: ' + m;
  }
  var saved = 'auto';
  try { saved = localStorage.getItem('smelt-arch-theme') || 'auto'; } catch (e) {}
  apply(saved);
  btn.addEventListener('click', function () {
    var next = modes[(modes.indexOf(btn.textContent.replace('Theme: ', '')) + 1) % modes.length];
    apply(next);
    try { localStorage.setItem('smelt-arch-theme', next); } catch (e) {}
  });
  // crate graph: highlight the edges of the hovered or focused crate
  document.querySelectorAll('.hoverable svg').forEach(function (svg) {
    svg.querySelectorAll('.node.crate').forEach(function (n) {
      var id = n.getAttribute('data-id');
      function on() {
        svg.classList.add('dim');
        svg.querySelectorAll('.edge.dep').forEach(function (e) {
          if (e.getAttribute('data-from') === id || e.getAttribute('data-to') === id) e.classList.add('hl');
        });
      }
      function off() {
        svg.classList.remove('dim');
        svg.querySelectorAll('.edge.hl').forEach(function (e) { e.classList.remove('hl'); });
      }
      var t = n.closest('a') || n;
      t.addEventListener('mouseenter', on); t.addEventListener('mouseleave', off);
      t.addEventListener('focus', on); t.addEventListener('blur', off);
    });
  });
  // table of contents: mark the section in view
  var links = {};
  document.querySelectorAll('nav.toc a').forEach(function (a) { links[a.getAttribute('href').slice(1)] = a; });
  if ('IntersectionObserver' in window) {
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (en) {
        if (en.isIntersecting) {
          Object.keys(links).forEach(function (k) { links[k].classList.remove('on'); });
          if (links[en.target.id]) links[en.target.id].classList.add('on');
        }
      });
    }, { rootMargin: '-20% 0px -70% 0px' });
    document.querySelectorAll('main section').forEach(function (s) { io.observe(s); });
  }
})();

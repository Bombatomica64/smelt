// Theme toggle: honors saved preference, falls back to the OS scheme.
(function () {
  var root = document.documentElement;
  var saved = null;
  try { saved = localStorage.getItem("smelt-theme"); } catch (e) { /* private mode */ }
  var preferred = saved ||
    (window.matchMedia && window.matchMedia("(prefers-color-scheme: light)").matches
      ? "light"
      : "dark");
  root.setAttribute("data-theme", preferred);

  window.addEventListener("DOMContentLoaded", function () {
    var button = document.getElementById("theme-toggle");
    if (!button) { return; }
    button.addEventListener("click", function () {
      var next = root.getAttribute("data-theme") === "dark" ? "light" : "dark";
      root.setAttribute("data-theme", next);
      try { localStorage.setItem("smelt-theme", next); } catch (e) { /* private mode */ }
    });
  });
})();

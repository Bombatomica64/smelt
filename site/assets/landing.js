// Landing-page behavior: ember field, scroll reveals, demo tabs, and the
// fake `smelt build` run animation.
(function () {

  // --- ember particle field --------------------------------------
  var field = document.querySelector(".embers");
  if (field) {
    for (var i = 0; i < 26; i++) {
      var ember = document.createElement("span");
      ember.className = "ember";
      ember.style.setProperty("--x", (Math.random() * 100).toFixed(2) + "%");
      ember.style.setProperty("--dur", (7 + Math.random() * 9).toFixed(2) + "s");
      ember.style.setProperty("--delay", (-Math.random() * 12).toFixed(2) + "s");
      ember.style.setProperty("--drift", ((Math.random() - 0.5) * 8).toFixed(2) + "rem");
      ember.style.setProperty("--peak", (0.35 + Math.random() * 0.55).toFixed(2));
      var size = (2 + Math.random() * 3.5).toFixed(1);
      ember.style.width = size + "px";
      ember.style.height = size + "px";
      field.appendChild(ember);
    }
  }

  // --- scroll reveal -------------------------------------------------------
  if ("IntersectionObserver" in window) {
    var observer = new IntersectionObserver(function (entries) {
      entries.forEach(function (entry) {
        if (entry.isIntersecting) {
          entry.target.classList.add("is-visible");
          observer.unobserve(entry.target);
        }
      });
    }, { threshold: 0.15 });
    document.querySelectorAll(".reveal").forEach(function (el) { observer.observe(el); });
  } else {
    document.querySelectorAll(".reveal").forEach(function (el) { el.classList.add("is-visible"); });
  }

  // --- demo tabs + run animation ------------------------------------------
  var tabs = document.querySelectorAll(".demo-tabs .tab");
  var panes = document.querySelectorAll(".demo-pane");
  function showPane(name) {
    tabs.forEach(function (tab) { tab.setAttribute("aria-selected", String(tab.dataset.pane === name)); });
    panes.forEach(function (pane) { pane.classList.toggle("is-active", pane.dataset.pane === name); });
  }
  tabs.forEach(function (tab) {
    tab.addEventListener("click", function () { showPane(tab.dataset.pane); });
  });

  var runButton = document.querySelector(".demo-run");
  var terminal = document.querySelector(".demo-terminal .line");
  var running = false;
  var script = [
    ["$ smelt --manifest-path Smelt.toml build", 260],
    ["\nsmelt: 2 sources -> HIR -> MIR", 420],
    ["\nsmelt: emitted crate at ./dist-smelt", 380],
    ["\n$ cargo test --manifest-path dist-smelt/Cargo.toml", 300],
    ["\ntest result: ok. 2 passed; 0 failed", 500],
  ];
  if (runButton && terminal) {
    runButton.addEventListener("click", function () {
      if (running) { return; }
      running = true;
      terminal.textContent = "";
      showPane("rust");
      var index = 0;
      function step() {
        if (index >= script.length) { running = false; return; }
        var chunk = script[index][0];
        var pause = script[index][1];
        var chars = 0;
        var typer = setInterval(function () {
          chars += 2;
          terminal.textContent = terminal.textContent.slice(0, terminal.textContent.length - 0);
          terminal.textContent += chunk.slice(chars - 2, chars);
          if (chars >= chunk.length) {
            clearInterval(typer);
            index += 1;
            setTimeout(step, pause);
          }
        }, 12);
      }
      step();
    });
  }
})();

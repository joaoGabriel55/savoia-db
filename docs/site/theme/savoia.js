// Savoia Studio docs: a home link and a Ko-fi note in the sidebar, a Ko-fi
// button in the menu band, and "Dark" instead of mdBook's "Coal".
(function () {
  "use strict";
  var KOFI = "https://ko-fi.com/O5I528IC3A";
  var root = typeof path_to_root === "string" ? path_to_root : "";
  var cup =
    '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' +
    '<path d="M10 2v2"/><path d="M14 2v2"/><path d="M16 8a1 1 0 0 1 1 1v8a4 4 0 0 1-4 4H7a4 4 0 0 1-4-4V9a1 1 0 0 1 1-1h14a4 4 0 1 1 0 8h-1"/><path d="M6 2v2"/></svg>';

  function sidebar() {
    var box = document.querySelector(".sidebar .sidebar-scrollbox");
    if (!box || box.querySelector(".savoia-home")) return;

    var home = document.createElement("a");
    home.className = "savoia-home";
    home.href = root + "../";
    home.innerHTML = '<img src="' + root + '../app-icon.svg" alt="">Savoia Studio<small>Docs</small>';
    box.insertBefore(home, box.firstChild);

    var note = document.createElement("div");
    note.className = "savoia-support";
    note.innerHTML =
      "<b>Free, no license key.</b>" +
      "<p>No trial, no paid tier, no account. If Savoia saves you time, you can buy the maker a coffee.</p>" +
      '<a class="savoia-kofi" href="' + KOFI + '">' + cup + "Support on Ko-fi</a>";
    box.appendChild(note);
  }

  function band() {
    var right = document.querySelector(".menu-bar .right-buttons");
    if (right && !right.querySelector(".savoia-kofi-band")) {
      var a = document.createElement("a");
      a.className = "savoia-kofi-band";
      a.href = KOFI;
      a.title = "Support Savoia Studio on Ko-fi";
      a.innerHTML = cup + "<span>Ko-fi</span>";
      right.insertBefore(a, right.firstChild);
    }
    var title = document.querySelector(".menu-bar .menu-title");
    if (title && !title.querySelector("a")) {
      title.innerHTML = '<a href="' + root + 'index.html">' + title.textContent + "</a>";
    }
    var coal = document.getElementById("coal") || document.getElementById("mdbook-theme-coal");
    if (coal) coal.textContent = "Dark";
  }

  // Recordings autoplay like GIFs; with reduced motion they wait for a click.
  function motion() {
    if (!matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    document.querySelectorAll(".content video[autoplay]").forEach(function (v) {
      v.removeAttribute("autoplay");
      v.pause();
      v.controls = true;
    });
  }

  function run() {
    sidebar();
    band();
    motion();
  }
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", run);
  } else {
    run();
  }
})();

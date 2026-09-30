// ts-rs.bext.dev: small progressive enhancements. No dependencies. Every
// feature degrades to plain server-rendered HTML when this file does not run.
(function () {
  "use strict";

  // Sticky nav border once the page is scrolled.
  var nav = document.querySelector(".t-nav");
  function onScroll() {
    if (nav) nav.setAttribute("data-scrolled", window.scrollY > 6 ? "1" : "0");
  }
  onScroll();
  window.addEventListener("scroll", onScroll, { passive: true });

  // Mobile menu.
  var toggle = document.getElementById("t-nav-toggle");
  var mobile = document.getElementById("t-nav-mobile");
  if (toggle && mobile) {
    toggle.addEventListener("click", function () {
      var open = mobile.classList.toggle("open");
      toggle.setAttribute("aria-expanded", open ? "true" : "false");
    });
  }

  // Scroll reveal. The head script only hides .rv elements when JS is on.
  var reveals = document.querySelectorAll(".rv");
  if ("IntersectionObserver" in window) {
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (e) {
        if (e.isIntersecting) {
          e.target.classList.add("in");
          io.unobserve(e.target);
        }
      });
    }, { rootMargin: "0px 0px -6% 0px", threshold: 0.06 });
    reveals.forEach(function (el) { io.observe(el); });
  } else {
    reveals.forEach(function (el) { el.classList.add("in"); });
  }

  // Copy buttons on code blocks.
  document.querySelectorAll("[data-copy]").forEach(function (btn) {
    btn.addEventListener("click", function () {
      var wrap = btn.closest("[data-code]");
      var code = wrap && wrap.querySelector("code");
      if (!code || !navigator.clipboard) return;
      navigator.clipboard.writeText(code.innerText).then(function () {
        var label = btn.textContent;
        btn.textContent = "Copied";
        setTimeout(function () { btn.textContent = label; }, 1400);
      });
    });
  });

  // Docs sidebar: rendered open (works without JS), collapsed on small screens.
  var disc = document.querySelector(".d-disc");
  if (disc && window.matchMedia) {
    var mq = window.matchMedia("(max-width: 900px)");
    var sync = function (m) {
      if (m.matches) disc.removeAttribute("open");
      else disc.setAttribute("open", "");
    };
    sync(mq);
    if (mq.addEventListener) mq.addEventListener("change", sync);
  }

  // Docs "On this page" rail, built from the rendered h2 and h3.
  var rail = document.querySelector(".d-toc");
  if (rail) {
    var shell = document.querySelector(".d-shell");
    var doc = document.querySelector(".doc");
    var heads = doc ? doc.querySelectorAll("h2[id], h3[id]") : [];
    if (heads.length < 2) {
      if (shell) shell.classList.add("no-toc");
      rail.remove();
    } else {
      var title = document.createElement("div");
      title.className = "d-toc-title";
      title.textContent = "On this page";
      var ul = document.createElement("ul");
      var byId = {};
      heads.forEach(function (h) {
        var li = document.createElement("li");
        li.className = "lvl-" + h.tagName.toLowerCase();
        var a = document.createElement("a");
        a.href = "#" + h.id;
        a.textContent = (h.getAttribute("data-title") || h.textContent || "").trim();
        li.appendChild(a);
        ul.appendChild(li);
        byId[h.id] = a;
      });
      rail.appendChild(title);
      rail.appendChild(ul);
      if ("IntersectionObserver" in window) {
        var current = null;
        var spy = new IntersectionObserver(function (entries) {
          entries.forEach(function (e) {
            if (!e.isIntersecting) return;
            var a = byId[e.target.id];
            if (current === a) return;
            if (current) current.classList.remove("active");
            current = a;
            if (a) a.classList.add("active");
          });
        }, { rootMargin: "-84px 0px -70% 0px", threshold: 0 });
        heads.forEach(function (h) { spy.observe(h); });
      }
    }
  }

  // Hero demo tabs (WAI-ARIA tabs: click, arrow keys, Home and End).
  document.querySelectorAll("[data-tabs]").forEach(function (root) {
    var tabs = Array.prototype.slice.call(root.querySelectorAll('[role="tab"]'));
    var select = function (tab, focus) {
      tabs.forEach(function (t) {
        var on = t === tab;
        t.setAttribute("aria-selected", on ? "true" : "false");
        if (on) t.removeAttribute("tabindex"); else t.setAttribute("tabindex", "-1");
        var panel = document.getElementById(t.getAttribute("aria-controls"));
        if (panel) { if (on) panel.removeAttribute("hidden"); else panel.setAttribute("hidden", ""); }
      });
      if (focus) tab.focus();
    };
    tabs.forEach(function (tab, i) {
      tab.addEventListener("click", function () { select(tab, false); });
      tab.addEventListener("keydown", function (ev) {
        var k = ev.key;
        var next = k === "ArrowRight" ? i + 1 : k === "ArrowLeft" ? i - 1 : k === "Home" ? 0 : k === "End" ? tabs.length - 1 : -2;
        if (next === -2) return;
        ev.preventDefault();
        select(tabs[(next + tabs.length) % tabs.length], true);
      });
    });
  });

  // Theme toggle. The choice is stored; without one the OS preference applies.
  function currentTheme() {
    var t = document.documentElement.getAttribute("data-theme");
    if (t === "dark" || t === "light") return t;
    return window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }
  document.querySelectorAll("[data-theme-toggle]").forEach(function (btn) {
    btn.addEventListener("click", function () {
      var next = currentTheme() === "dark" ? "light" : "dark";
      document.documentElement.setAttribute("data-theme", next);
      try { localStorage.setItem("theme", next); } catch (e) { /* private mode */ }
    });
  });

  // Docs search: a dialog over a static index, fetched on first use.
  var searchDialog = null;
  var searchIndex = null;
  var searchSel = 0;

  function escapeHtml(s) {
    return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  }
  function markTerms(text, terms) {
    var out = escapeHtml(text);
    terms.forEach(function (t) {
      var re = new RegExp("(" + t.replace(/[.*+?^${}()|[\]\\]/g, "\\$&") + ")", "ig");
      out = out.replace(re, "<mark>$1</mark>");
    });
    return out;
  }
  function snippet(text, terms) {
    var low = text.toLowerCase();
    var at = -1;
    terms.forEach(function (t) { var k = low.indexOf(t); if (k !== -1 && (at === -1 || k < at)) at = k; });
    var start = Math.max(0, at - 60);
    var cut = text.slice(start, start + 180);
    return (start > 0 ? "..." : "") + cut + (start + 180 < text.length ? "..." : "");
  }
  function runSearch(q) {
    var terms = q.toLowerCase().split(/\s+/).filter(function (t) { return t.length > 1; });
    if (!terms.length || !searchIndex) return [];
    var hits = [];
    searchIndex.forEach(function (e) {
      var page = e.t.toLowerCase();
      var head = e.h.toLowerCase();
      var text = e.x.toLowerCase();
      var score = 0;
      for (var i = 0; i < terms.length; i++) {
        var t = terms[i];
        var s = 0;
        if (head.indexOf(t) !== -1) s += 6;
        if (page.indexOf(t) !== -1) s += e.h ? 2 : 5;
        var hitsInText = text.split(t).length - 1;
        if (hitsInText) s += Math.min(hitsInText, 4);
        if (!s) return;
        score += s;
      }
      hits.push({ e: e, score: score });
    });
    hits.sort(function (a, b) { return b.score - a.score; });
    return hits.slice(0, 8).map(function (h) { return { e: h.e, terms: terms }; });
  }
  function renderResults(list, q, results) {
    searchSel = 0;
    if (!q.trim()) {
      list.innerHTML = '<li class="sr-empty">Type to search the documentation: a flag, an option, an error code.</li>';
      return;
    }
    if (!results.length) {
      list.innerHTML = '<li class="sr-empty">No results for this search.</li>';
      return;
    }
    list.innerHTML = results.map(function (r, i) {
      return '<li class="sr-item" role="option" aria-selected="' + (i === 0 ? "true" : "false") + '"><a href="' + r.e.u + '">' +
        '<div class="sr-page">' + escapeHtml(r.e.t) + "</div>" +
        '<div class="sr-title">' + markTerms(r.e.h || r.e.t, r.terms) + "</div>" +
        '<div class="sr-snip">' + markTerms(snippet(r.e.x, r.terms), r.terms) + "</div></a></li>";
    }).join("");
  }
  function openSearch() {
    if (!searchDialog) {
      if (typeof HTMLDialogElement === "undefined") { window.location.href = "/docs"; return; }
      searchDialog = document.createElement("dialog");
      searchDialog.className = "sr";
      searchDialog.setAttribute("aria-label", "Search the documentation");
      searchDialog.innerHTML =
        '<div class="sr-bar"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16z M21 21l-4.35-4.35"/></svg>' +
        '<input class="sr-input" type="search" placeholder="Search the docs" autocomplete="off" spellcheck="false" aria-label="Search">' +
        '<button class="sr-esc" type="button">esc</button></div>' +
        '<ul class="sr-list" role="listbox"></ul>' +
        '<div class="sr-foot"><span>up / down to move</span><span>enter to open</span></div>';
      document.body.appendChild(searchDialog);
      var input = searchDialog.querySelector(".sr-input");
      var list = searchDialog.querySelector(".sr-list");
      var update = function () { renderResults(list, input.value, runSearch(input.value)); };
      input.addEventListener("input", update);
      searchDialog.querySelector(".sr-esc").addEventListener("click", function () { searchDialog.close(); });
      searchDialog.addEventListener("click", function (ev) { if (ev.target === searchDialog) searchDialog.close(); });
      input.addEventListener("keydown", function (ev) {
        var items = list.querySelectorAll(".sr-item");
        if (ev.key === "ArrowDown" || ev.key === "ArrowUp") {
          ev.preventDefault();
          if (!items.length) return;
          items[searchSel].setAttribute("aria-selected", "false");
          searchSel = (searchSel + (ev.key === "ArrowDown" ? 1 : items.length - 1)) % items.length;
          items[searchSel].setAttribute("aria-selected", "true");
          items[searchSel].scrollIntoView({ block: "nearest" });
        } else if (ev.key === "Enter" && items.length) {
          ev.preventDefault();
          var href = items[searchSel].querySelector("a").getAttribute("href");
          searchDialog.close();
          window.location.href = href;
        }
      });
      update();
      fetch("/search-index.json?v=" + (document.documentElement.getAttribute("data-build") || "1"))
        .then(function (r) { return r.json(); })
        .then(function (data) { searchIndex = data; update(); })
        .catch(function () { list.innerHTML = '<li class="sr-empty">The search index could not be loaded.</li>'; });
    }
    if (!searchDialog.open) searchDialog.showModal();
    var field = searchDialog.querySelector(".sr-input");
    field.focus();
    field.select();
  }
  document.querySelectorAll("[data-search]").forEach(function (btn) { btn.addEventListener("click", openSearch); });
  document.addEventListener("keydown", function (ev) {
    var tag = (ev.target && ev.target.tagName) || "";
    var typing = tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || (ev.target && ev.target.isContentEditable);
    if ((ev.key === "k" && (ev.metaKey || ev.ctrlKey)) || (ev.key === "/" && !typing && !ev.metaKey && !ev.ctrlKey && !ev.altKey)) {
      ev.preventDefault();
      openSearch();
    }
  });

  // Progress chart: crosshair and tooltip. Every value it shows is also in the
  // wave list under the chart.
  var plot = document.getElementById("wave-chart");
  var dataEl = document.getElementById("wave-data");
  if (plot && dataEl) {
    var cd = JSON.parse(dataEl.textContent);
    var svg = plot.querySelector("svg");
    var hover = svg.querySelector(".cv-hover");
    var cross = hover.querySelector(".cv-cross");
    var dots = hover.querySelectorAll(".cv-dot");
    var tip = plot.querySelector(".cv-tip");
    var g = { l: +svg.dataset.l, r: +svg.dataset.r, t: +svg.dataset.t, b: +svg.dataset.b, w: +svg.dataset.w, h: +svg.dataset.h, ymax: +svg.dataset.ymax };
    var n = cd.points.length;
    var xAt = function (i) { return g.l + (n === 1 ? 0 : i * (g.w - g.l - g.r) / (n - 1)); };
    var yAt = function (v) { return g.t + (g.h - g.t - g.b) * (1 - v / g.ymax); };
    var fmt = function (v) { return String(v).replace(/\B(?=(\d{3})+(?!\d))/g, ","); };
    var row = function (cls, label, value, base) {
      var el = document.createElement("div");
      el.className = "cv-tip-row";
      var key = document.createElement("i");
      key.className = "cv-key " + cls;
      var b = document.createElement("b");
      b.textContent = value === null ? "n/a" : "+" + (value - base);
      var s = document.createElement("span");
      s.textContent = label + (value === null ? "" : " (" + fmt(value) + " passing)");
      el.append(key, b, s);
      return el;
    };
    var show = function (clientX) {
      var box = svg.getBoundingClientRect();
      var x = (clientX - box.left) / box.width * g.w;
      var i = Math.max(0, Math.min(n - 1, Math.round((x - g.l) / ((g.w - g.l - g.r) / Math.max(1, n - 1)))));
      var p = cd.points[i];
      var px = xAt(i);
      hover.removeAttribute("hidden");
      cross.setAttribute("x1", px); cross.setAttribute("x2", px);
      [[dots[0], p.f, cd.baseF], [dots[1], p.c, cd.baseC]].forEach(function (d) {
        if (d[1] === null) { d[0].setAttribute("visibility", "hidden"); return; }
        d[0].setAttribute("visibility", "visible");
        d[0].setAttribute("cx", px); d[0].setAttribute("cy", yAt(d[1] - d[2]));
      });
      tip.textContent = "";
      var date = document.createElement("div"); date.className = "cv-tip-date"; date.textContent = p.date;
      var title = document.createElement("div"); title.className = "cv-tip-title"; title.textContent = p.title;
      tip.append(date, title, row("cv-s1", "Compiler", p.c, cd.baseC), row("cv-s2", "Conformance", p.f, cd.baseF));
      tip.removeAttribute("hidden");
      var left = px / g.w * box.width;
      var tw = tip.offsetWidth;
      var pos = left + 14 + tw > box.width ? left - 14 - tw : left + 14;
      tip.style.transform = "translate(" + Math.max(0, pos) + "px, 6px)";
    };
    var hide = function () { hover.setAttribute("hidden", ""); tip.setAttribute("hidden", ""); };
    svg.addEventListener("pointermove", function (ev) { show(ev.clientX); });
    svg.addEventListener("pointerdown", function (ev) { show(ev.clientX); });
    svg.addEventListener("pointerleave", hide);
  }
})();

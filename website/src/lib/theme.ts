// The whole stylesheet for ts-rs.bext.dev, inlined by the root layout.
//
// Hand-written and self-contained: every class is prefixed (t- for the site,
// d- for docs, pg- for the playground, hl- for syntax tokens) so nothing
// depends on the route_css utility generator.
//
// Authoring rules for this file: the CSS lives in one template literal, so it
// must contain no backtick, no dollar-brace and no backslash.

export const THEME_CSS = `
:root {
  --bg: #faf9f7; --surface: #ffffff; --tint: #f2f0eb; --tint-2: #ebe8e1;
  --ink: #0c0e13; --ink-2: #2a2f3a; --muted: #565d6a; --faint: #666d7a;
  --line: #e7e4de; --line-2: #d8d4cb;
  --accent: #2563c9; --accent-ink: #1c4fa6; --accent-tint: #e9f0fc;
  --pass: #157043; --pass-tint: #e6f4ec; --warn: #b45309; --warn-tint: #fdf1e2; --rust: #b7410e;
  --code-bg: #0c0f15; --code-bar: #12161e; --code-line: #1e232d; --code-ink: #dde2ea; --code-dim: #868e9d;
  --mono: 'JetBrains Mono', ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  --sans: 'Inter', system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif;
  --wrap: 1160px; --nav-h: 64px; --radius: 14px;
  --ink-hover: #232733; --on-ink: #ffffff; --code-edge: #1a1f29; --shadow: rgba(12,14,19,.4);
  --series-1: #2a78d6; --series-2: #eb6834;
  color-scheme: light;
}
*, *::before, *::after { box-sizing: border-box; }
html { -webkit-text-size-adjust: 100%; scroll-padding-top: 84px; }
body { margin: 0; background: var(--bg); color: var(--ink); font-family: var(--sans); font-size: 16px; line-height: 1.6; letter-spacing: -.011em; -webkit-font-smoothing: antialiased; -moz-osx-font-smoothing: grayscale; text-rendering: optimizeLegibility; }
::selection { background: rgba(37,99,201,.18); }
a { color: inherit; text-decoration: none; }
h1, h2, h3, h4, p { margin: 0; }
code, pre, kbd { font-family: var(--mono); }
img, svg { display: block; }
button { font-family: inherit; }
.t-wrap { max-width: var(--wrap); margin: 0 auto; padding: 0 1.5rem; }
.t-wrap-sm { max-width: 52rem; margin: 0 auto; padding: 0 1.5rem; }
.t-vh { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
.t-skip { position: absolute; left: -999px; top: 0; background: var(--ink); color: var(--on-ink); padding: .6rem 1rem; border-radius: 0 0 8px 0; z-index: 100; }
.t-skip:focus { left: 0; }
:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; border-radius: 4px; }

/* ---- Navigation ---- */
.t-nav { position: sticky; top: 0; z-index: 60; background: color-mix(in srgb, var(--bg) 82%, transparent); backdrop-filter: saturate(1.6) blur(14px); -webkit-backdrop-filter: saturate(1.6) blur(14px); border-bottom: 1px solid transparent; transition: border-color .2s, background .2s; }
.t-nav[data-scrolled="1"] { border-bottom-color: var(--line); background: color-mix(in srgb, var(--bg) 94%, transparent); }
.t-nav-in { max-width: 1240px; margin: 0 auto; padding: 0 1.5rem; height: var(--nav-h); display: flex; align-items: center; gap: 1rem; }
.t-logo { display: inline-flex; align-items: center; gap: .6rem; flex: 0 0 auto; }
.t-logo-mk { position: relative; width: 30px; height: 30px; border-radius: 8px; background: var(--ink); color: var(--on-ink); font: 700 .78rem/1 var(--mono); letter-spacing: -.04em; display: inline-flex; align-items: center; justify-content: center; }
.t-logo-mk::after { content: ""; position: absolute; top: 4px; right: 4px; width: 6px; height: 6px; border-radius: 2px; background: #3b82f6; }
.t-logo-wm { font: 700 1.06rem/1 var(--mono); letter-spacing: -.045em; color: var(--ink); }
.t-logo-v { font: 500 .7rem/1 var(--mono); color: var(--faint); padding: .28rem .42rem; border-radius: 6px; background: var(--tint); }
.t-nav-links { display: flex; align-items: center; gap: .15rem; margin-left: 1rem; }
.t-nav-links a { padding: .45rem .75rem; border-radius: 8px; font-size: .89rem; font-weight: 500; color: var(--muted); transition: color .15s, background .15s; }
.t-nav-links a:hover { color: var(--ink); background: var(--tint); }
.t-nav-links a[aria-current="page"] { color: var(--ink); background: var(--tint); font-weight: 600; }
.t-nav-right { margin-left: auto; display: flex; align-items: center; gap: .5rem; }
.t-nav-gh { display: inline-flex; align-items: center; gap: .45rem; padding: .45rem .7rem; border-radius: 8px; font-size: .89rem; font-weight: 500; color: var(--muted); transition: color .15s, background .15s; }
.t-nav-gh:hover { color: var(--ink); background: var(--tint); }
.t-nav-toggle { display: none; margin-left: auto; width: 40px; height: 40px; align-items: center; justify-content: center; border: 1px solid var(--line-2); border-radius: 10px; background: var(--surface); color: var(--ink); cursor: pointer; }
.t-nav-mobile { display: none; }

/* ---- Buttons, pills, headings ---- */
.t-btn { display: inline-flex; align-items: center; justify-content: center; gap: .5rem; height: 2.85rem; padding: 0 1.3rem; border-radius: 10px; font-size: .93rem; font-weight: 600; letter-spacing: -.01em; border: 1px solid transparent; cursor: pointer; white-space: nowrap; transition: transform .15s, background .15s, color .15s, border-color .15s, box-shadow .15s; }
.t-btn .ic { transition: transform .2s cubic-bezier(.2,.7,.2,1); }
.t-btn:hover .ic { transform: translateX(3px); }
.t-btn-primary { background: var(--ink); color: var(--on-ink); box-shadow: 0 10px 24px -14px rgba(12,14,19,.7); }
.t-btn-primary:hover { background: var(--ink-hover); transform: translateY(-1px); }
.t-btn-ghost { background: var(--surface); color: var(--ink); border-color: var(--line-2); }
.t-btn-ghost:hover { background: var(--tint); }
.t-btn-light { background: #fff; color: #0c0e13; }
.t-btn-light:hover { transform: translateY(-1px); }
.t-btn-ondark { background: rgba(255,255,255,.08); color: #fff; border-color: rgba(255,255,255,.18); }
.t-btn-ondark:hover { background: rgba(255,255,255,.15); }
.t-btn-sm { height: 2.35rem; padding: 0 1rem; font-size: .86rem; border-radius: 9px; }
.t-pill { display: inline-flex; align-items: center; gap: .55rem; padding: .38rem .85rem .38rem .7rem; border-radius: 999px; background: var(--surface); border: 1px solid var(--line); font-size: .8rem; font-weight: 550; color: var(--muted); }
.t-pill-dot { width: 7px; height: 7px; border-radius: 50%; background: var(--pass); box-shadow: 0 0 0 3px var(--pass-tint); }
.t-pill b { color: var(--ink); font-weight: 650; }
.t-eyebrow { display: inline-block; font: 600 .72rem/1 var(--mono); letter-spacing: .08em; text-transform: uppercase; color: var(--accent); }
.t-sec { padding: 5.5rem 0; }
.t-sec-tint { background: var(--tint); border-top: 1px solid var(--line); border-bottom: 1px solid var(--line); }
.t-head { max-width: 44rem; margin: 0 0 3rem; }
.t-head-center { margin-left: auto; margin-right: auto; text-align: center; }
.t-head h2 { margin-top: .9rem; font-size: clamp(1.75rem, 3.3vw, 2.35rem); font-weight: 750; letter-spacing: -.035em; line-height: 1.1; }
.t-head p { margin-top: .95rem; font-size: 1.07rem; line-height: 1.62; color: var(--muted); }
.t-note { margin-top: 1.4rem; font-size: .84rem; line-height: 1.6; color: var(--faint); }
.t-note a, .t-link { color: var(--accent-ink); font-weight: 550; border-bottom: 1px solid rgba(37,99,201,.3); transition: border-color .15s; }
.t-note a:hover, .t-link:hover { border-bottom-color: var(--accent); }

/* ---- Hero ---- */
.t-hero { position: relative; padding: 5rem 0 3.5rem; overflow: hidden; }
.t-hero-grid { position: absolute; inset: 0; z-index: -1; pointer-events: none; background-image: linear-gradient(var(--line) 1px, transparent 1px), linear-gradient(90deg, var(--line) 1px, transparent 1px); background-size: 44px 44px; -webkit-mask-image: radial-gradient(70% 58% at 50% 0%, rgba(0,0,0,.55), transparent 75%); mask-image: radial-gradient(70% 58% at 50% 0%, rgba(0,0,0,.55), transparent 75%); }
.t-hero-copy { max-width: 50rem; margin: 0 auto; text-align: center; }
.t-hero h1 { margin-top: 1.5rem; font-size: clamp(2.3rem, 5.6vw, 3.75rem); font-weight: 800; letter-spacing: -.05em; line-height: 1.02; }
.t-hero h1 em { font-style: normal; color: var(--accent); }
.t-hero-lead { max-width: 41rem; margin: 1.5rem auto 0; font-size: clamp(1.03rem, 1.5vw, 1.18rem); line-height: 1.62; color: var(--muted); }
.t-hero-actions { margin-top: 2.1rem; display: flex; gap: .7rem; justify-content: center; flex-wrap: wrap; }
.t-hero-meta { margin-top: 1.4rem; display: flex; gap: 1.4rem; justify-content: center; flex-wrap: wrap; font-size: .84rem; color: var(--faint); }
.t-hero-meta span { display: inline-flex; align-items: center; gap: .4rem; }
.t-hero-meta .ic { color: var(--pass); }

/* ---- Compile window ---- */
.t-win { margin: 3.4rem auto 0; max-width: 1080px; border-radius: 16px; overflow: hidden; background: var(--code-bg); border: 1px solid var(--code-edge); box-shadow: 0 40px 90px -50px rgba(12,14,19,.75), 0 2px 0 rgba(255,255,255,.04) inset; text-align: left; }
.t-win-bar { display: flex; align-items: center; gap: .5rem; padding: .7rem 1rem; background: var(--code-bar); border-bottom: 1px solid var(--code-line); }
.t-win-bar i { width: 10px; height: 10px; border-radius: 50%; background: #2b313d; flex: 0 0 auto; }
.t-win-cmd { margin-left: .6rem; font: 500 .78rem/1.3 var(--mono); color: var(--code-dim); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; }
.t-win-cmd b { color: #cdd3dd; font-weight: 500; }
.t-win-ok { margin-left: auto; flex: 0 0 auto; display: inline-flex; align-items: center; gap: .4rem; font: 500 .72rem/1 var(--mono); color: #6fd39a; background: rgba(111,211,154,.1); padding: .34rem .55rem; border-radius: 6px; }
.t-win-panes { display: grid; grid-template-columns: 1fr 1fr; }
.t-pane { min-width: 0; }
.t-pane + .t-pane { border-left: 1px solid var(--code-line); }
.t-pane-tab { padding: .55rem 1.15rem 0; }
.t-pane-tab span { display: inline-block; font: 500 .72rem/1 var(--mono); color: var(--code-dim); padding: .4rem 0 .55rem; border-bottom: 2px solid #3b82f6; }
.t-pane pre, .t-code pre { margin: 0; padding: .9rem 1.15rem 1.2rem; overflow-x: auto; }
.t-pane code, .t-code code { font-size: .8rem; line-height: 1.75; color: var(--code-ink); white-space: pre; }

/* ---- Scoreboard ---- */
.t-score { display: grid; grid-template-columns: repeat(4, 1fr); gap: 1px; background: var(--line); border: 1px solid var(--line); border-radius: 16px; overflow: hidden; }
.t-score-cell { background: var(--surface); padding: 1.7rem 1.5rem 1.6rem; display: flex; flex-direction: column; }
.t-score-n { font: 700 clamp(2rem, 3.6vw, 2.7rem)/1 var(--mono); letter-spacing: -.06em; color: var(--ink); font-variant-numeric: tabular-nums; }
.t-score-n span { font-size: .5em; font-weight: 600; margin-left: .08em; color: var(--faint); letter-spacing: -.02em; }
.t-score-l { margin-top: .95rem; font-size: .95rem; font-weight: 650; }
.t-score-s { margin: .25rem 0 1.15rem; font-size: .83rem; line-height: 1.5; color: var(--muted); flex: 1 1 auto; }
.t-meter { height: 6px; border-radius: 999px; background: var(--tint-2); overflow: hidden; }
.t-meter-fill { height: 100%; width: var(--v); border-radius: 999px; background: var(--ink); }
.t-meter-pass .t-meter-fill { background: var(--pass); }
.t-meter-accent .t-meter-fill { background: var(--accent); }
.t-meter-warn .t-meter-fill { background: #d08a2e; }
.t-meter-muted .t-meter-fill { background: var(--faint); }

/* ---- Feature cards ---- */
.t-cards { display: grid; grid-template-columns: repeat(3, 1fr); gap: 1rem; }
.t-card { background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius); padding: 1.55rem 1.5rem 1.5rem; display: flex; flex-direction: column; transition: transform .2s cubic-bezier(.2,.7,.2,1), box-shadow .2s; }
.t-card:hover { transform: translateY(-3px); box-shadow: 0 22px 44px -32px rgba(12,14,19,.4); }
.t-card-ic { width: 38px; height: 38px; border-radius: 10px; background: var(--tint); color: var(--ink); display: inline-flex; align-items: center; justify-content: center; margin-bottom: 1.05rem; }
.t-card h3, .t-card h2 { font-size: 1.04rem; font-weight: 680; letter-spacing: -.018em; }
.t-card p { margin-top: .5rem; font-size: .9rem; line-height: 1.6; color: var(--muted); flex: 1 1 auto; }
.t-card-cmd { margin-top: 1.1rem; font: 500 .76rem/1.4 var(--mono); color: var(--ink-2); background: var(--tint); border-radius: 8px; padding: .5rem .7rem; overflow-x: auto; white-space: nowrap; }
.t-card-cmd span { color: var(--faint); }

/* ---- Pipeline ---- */
.t-pipe { display: grid; grid-template-columns: repeat(5, 1fr); gap: .6rem; counter-reset: stage; }
.t-stage { position: relative; background: var(--surface); border: 1px solid var(--line); border-radius: 12px; padding: 1.1rem 1.05rem 1.15rem; }
.t-stage-n { font: 600 .7rem/1 var(--mono); color: var(--accent); letter-spacing: .04em; }
.t-stage code { display: block; margin-top: .7rem; font-size: .82rem; font-weight: 600; color: var(--ink); letter-spacing: -.02em; }
.t-stage p { margin-top: .4rem; font-size: .82rem; line-height: 1.5; color: var(--muted); }
.t-stage + .t-stage::before { content: ""; position: absolute; left: -.6rem; top: 50%; width: .6rem; height: 1px; background: var(--line-2); }
.t-pipe-io { display: flex; justify-content: space-between; margin-top: .9rem; font: 500 .76rem/1 var(--mono); color: var(--faint); }
.t-crates { list-style: none; margin: 2.2rem 0 0; padding: 0; display: grid; grid-template-columns: repeat(3, 1fr); gap: 0 2rem; }
.t-crates li { display: flex; gap: .9rem; align-items: baseline; padding: .7rem 0; border-top: 1px solid var(--line-2); font-size: .86rem; color: var(--muted); }
.t-crates code { flex: 0 0 auto; font-size: .78rem; font-weight: 600; color: var(--ink); letter-spacing: -.02em; }

/* ---- Benchmarks ---- */
.t-bench { display: grid; grid-template-columns: 1fr 1fr; gap: 1rem; }
.t-bench-box { background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius); padding: 1.5rem; }
.t-bench-box h3 { font-size: .98rem; font-weight: 660; letter-spacing: -.015em; }
.t-bench-box > p { margin-top: .3rem; font-size: .84rem; color: var(--muted); }
.t-bar { display: grid; grid-template-columns: 4.2rem 1fr 4.6rem; align-items: center; gap: .85rem; margin-top: 1.05rem; }
.t-bar-nm { font: 600 .8rem/1 var(--mono); color: var(--muted); }
.t-bar-track { height: 12px; border-radius: 4px; background: var(--tint); overflow: hidden; }
.t-bar-fill { height: 100%; width: var(--v); border-radius: 4px; background: var(--line-2); }
.t-bar-val { font: 600 .8rem/1 var(--mono); color: var(--muted); text-align: right; font-variant-numeric: tabular-nums; }
.t-bar-us .t-bar-nm, .t-bar-us .t-bar-val { color: var(--ink); }
.t-bar-us .t-bar-fill { background: var(--ink); }
.t-facts { margin-top: 1rem; display: grid; grid-template-columns: repeat(3, 1fr); gap: 1rem; }
.t-fact { padding: 1.2rem 0 0; border-top: 1px solid var(--line-2); }
.t-fact b { display: block; font: 700 1.35rem/1 var(--mono); letter-spacing: -.04em; }
.t-fact span { display: block; margin-top: .55rem; font-size: .87rem; line-height: 1.55; color: var(--muted); }

/* ---- Split section and code blocks ---- */
.t-split { display: grid; grid-template-columns: minmax(0, .9fr) minmax(0, 1.1fr); gap: 3.5rem; align-items: center; }
.t-split .t-head { margin-bottom: 0; }
.t-split > *, .t-block-head > *, .t-bench > *, .t-cards > * { min-width: 0; }
.t-list { list-style: none; margin: 1.6rem 0 0; padding: 0; display: grid; gap: .85rem; }
.t-list li { display: grid; grid-template-columns: auto 1fr; gap: .8rem; font-size: .94rem; line-height: 1.55; color: var(--muted); }
.t-list li .ic { margin-top: .22rem; color: var(--pass); }
.t-list b { color: var(--ink); font-weight: 620; }
.t-list code, .t-inline { font-size: .84em; background: var(--tint); border-radius: 5px; padding: .08rem .36rem; color: var(--ink-2); }
.t-code { border-radius: 14px; overflow: hidden; background: var(--code-bg); border: 1px solid var(--code-edge); box-shadow: 0 30px 60px -44px rgba(12,14,19,.7); }
.t-code + .t-code { margin-top: .8rem; }
.t-code-bar { display: flex; align-items: center; padding: .55rem .7rem .55rem 1.15rem; background: var(--code-bar); border-bottom: 1px solid var(--code-line); }
.t-code-label { font: 500 .74rem/1 var(--mono); color: var(--code-dim); }
.t-copy { margin-left: auto; font: 600 .7rem/1 var(--sans); color: #a5adba; background: rgba(255,255,255,.05); border: 1px solid rgba(255,255,255,.1); border-radius: 7px; padding: .34rem .6rem; cursor: pointer; transition: background .15s, color .15s; }
.t-copy:hover { background: rgba(255,255,255,.12); color: #fff; }

/* ---- Production panel and limits ---- */
.t-prod { display: grid; grid-template-columns: repeat(3, 1fr); gap: 1px; background: var(--line); border: 1px solid var(--line); border-radius: 16px; overflow: hidden; }
.t-prod-cell { background: var(--surface); padding: 1.6rem 1.5rem; }
.t-prod-cell h3 { font-size: 1rem; font-weight: 670; letter-spacing: -.015em; }
.t-prod-cell p { margin-top: .5rem; font-size: .9rem; line-height: 1.6; color: var(--muted); }
.t-limits { display: grid; grid-template-columns: repeat(3, 1fr); gap: 2rem; }
.t-limit { padding-top: 1.2rem; border-top: 2px solid var(--ink); }
.t-limit h3 { font-size: 1rem; font-weight: 670; letter-spacing: -.015em; }
.t-limit p { margin-top: .5rem; font-size: .9rem; line-height: 1.6; color: var(--muted); }

/* ---- CTA band and footer ---- */
.t-band { position: relative; overflow: hidden; border-radius: 20px; border: 1px solid var(--code-edge); background: var(--code-bg); color: #fff; padding: 3.6rem 2rem; text-align: center; }
.t-band::before { content: ""; position: absolute; inset: 0; pointer-events: none; background: radial-gradient(560px 300px at 85% -10%, rgba(59,130,246,.22), transparent 62%), radial-gradient(520px 300px at 10% 115%, rgba(255,255,255,.07), transparent 60%); }
.t-band > * { position: relative; }
.t-band h2 { font-size: clamp(1.65rem, 3.2vw, 2.2rem); font-weight: 750; letter-spacing: -.035em; line-height: 1.12; }
.t-band p { max-width: 35rem; margin: 1rem auto 0; color: rgba(255,255,255,.64); font-size: 1.02rem; }
.t-band-actions { margin-top: 2rem; display: flex; gap: .7rem; justify-content: center; flex-wrap: wrap; }
.t-foot { margin-top: 5rem; border-top: 1px solid var(--line); background: var(--tint); }
.t-foot-top { max-width: var(--wrap); margin: 0 auto; padding: 3.4rem 1.5rem 2.6rem; display: grid; grid-template-columns: 1.6fr 1fr 1fr 1fr; gap: 2rem; }
.t-foot-brand p { margin-top: 1rem; max-width: 21rem; font-size: .87rem; line-height: 1.62; color: var(--muted); }
.t-foot-h { font: 600 .7rem/1 var(--mono); letter-spacing: .08em; text-transform: uppercase; color: var(--faint); margin-bottom: .95rem; }
.t-foot-col a { display: block; padding: .27rem 0; font-size: .88rem; color: var(--muted); transition: color .15s; }
.t-foot-col a:hover { color: var(--ink); }
.t-foot-bottom { border-top: 1px solid var(--line-2); }
.t-foot-bottom a { color: var(--muted); border-bottom: 1px solid var(--line-2); }
.t-foot-bottom code { font-size: .92em; }
.t-foot-bottom div + div { padding-top: 0; }
.t-foot-bottom div { max-width: var(--wrap); margin: 0 auto; padding: 1.35rem 1.5rem; display: flex; flex-wrap: wrap; gap: .6rem 2rem; justify-content: space-between; font-size: .8rem; color: var(--faint); }

/* ---- Page header (conformance, playground) ---- */
.t-page-head { padding: 4.2rem 0 2.6rem; }
.t-page-head h1 { margin-top: 1rem; font-size: clamp(2rem, 4.2vw, 2.9rem); font-weight: 780; letter-spacing: -.045em; line-height: 1.06; }
.t-page-head p { max-width: 44rem; margin-top: 1.1rem; font-size: 1.08rem; line-height: 1.62; color: var(--muted); }
.t-stamp { margin-top: 1.5rem; display: inline-flex; flex-wrap: wrap; gap: .4rem 1.1rem; padding: .6rem .9rem; border-radius: 10px; background: var(--surface); border: 1px solid var(--line); font: 500 .76rem/1.5 var(--mono); color: var(--muted); }
.t-stamp b { color: var(--ink); font-weight: 600; }
.t-block { padding: 2.6rem 0; border-top: 1px solid var(--line); }
.t-block-head { display: grid; grid-template-columns: minmax(0, 20rem) minmax(0, 1fr); gap: 3rem; align-items: start; }
.t-block-head h2 { font-size: 1.35rem; font-weight: 720; letter-spacing: -.028em; line-height: 1.2; }
.t-block-head > div > p { margin-top: .7rem; font-size: .93rem; line-height: 1.62; color: var(--muted); }
.t-tag { display: inline-block; margin-top: .9rem; font: 600 .68rem/1 var(--mono); letter-spacing: .05em; text-transform: uppercase; padding: .36rem .55rem; border-radius: 6px; }
.t-tag-now { color: var(--pass); background: var(--pass-tint); }
.t-tag-old { color: var(--warn); background: var(--warn-tint); }
.t-lane { background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius); padding: 1.35rem 1.45rem 1.45rem; }
.t-lane + .t-lane { margin-top: .8rem; }
.t-lane h3 { font-size: 1rem; font-weight: 670; letter-spacing: -.015em; }
.t-lane > p { margin-top: .3rem; font-size: .86rem; line-height: 1.55; color: var(--muted); }
.t-lane-row { display: grid; grid-template-columns: 8.8rem minmax(0, 1fr) 10.5rem 3.9rem; align-items: center; gap: 1rem; margin-top: 1.05rem; }
.t-lane-suite { font-size: .85rem; font-weight: 560; color: var(--ink-2); }
.t-lane-count { font: 500 .78rem/1 var(--mono); color: var(--muted); text-align: right; font-variant-numeric: tabular-nums; white-space: nowrap; }
.t-lane-pct { font: 700 .88rem/1 var(--mono); text-align: right; font-variant-numeric: tabular-nums; letter-spacing: -.03em; }
.t-lane .t-meter { height: 8px; }
.t-table-wrap { background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius); overflow-x: auto; }
.t-table { width: 100%; border-collapse: collapse; font-size: .9rem; }
.t-table th, .t-table td { padding: .85rem 1.1rem; text-align: left; border-bottom: 1px solid var(--line); white-space: nowrap; }
.t-table tbody tr:last-child td { border-bottom: 0; }
.t-table thead th { font: 600 .68rem/1 var(--mono); letter-spacing: .06em; text-transform: uppercase; color: var(--faint); background: var(--bg); }
.t-table td.n, .t-table th.n { text-align: right; font-family: var(--mono); font-size: .82rem; font-variant-numeric: tabular-nums; }
.t-table td:first-child { font-weight: 570; }
.t-table tr.total td { font-weight: 700; background: var(--bg); }
.t-table .t-meter { width: 7rem; }
.t-prose { font-size: .95rem; line-height: 1.7; color: var(--muted); }
.t-prose p + p { margin-top: .9rem; }
.t-prose b { color: var(--ink); font-weight: 620; }
.t-prose ul { margin: 0; padding: 0; list-style: none; display: grid; gap: .8rem; }
.t-prose li { padding-left: 1.1rem; position: relative; }
.t-prose li::before { content: ""; position: absolute; left: 0; top: .68em; width: 5px; height: 5px; border-radius: 1px; background: var(--ink); }
.t-prose code { font-size: .84em; background: var(--tint); border-radius: 5px; padding: .08rem .36rem; color: var(--ink-2); }

/* ---- Docs shell ---- */
.d-shell { max-width: 1300px; margin: 0 auto; padding: 0 1.5rem; display: grid; grid-template-columns: 15.5rem minmax(0, 1fr) 13rem; gap: 2.75rem; align-items: start; }
.d-shell.no-toc { grid-template-columns: 15.5rem minmax(0, 1fr); max-width: 1200px; }
.d-side { position: sticky; top: var(--nav-h); align-self: start; max-height: calc(100vh - var(--nav-h)); overflow-y: auto; padding: 2.2rem 0 3rem; }
.d-disc { border: 0; margin: 0; padding: 0; }
.d-disc > summary { display: none; align-items: center; list-style: none; cursor: pointer; padding: .75rem 1rem; margin-bottom: .9rem; border: 1px solid var(--line-2); border-radius: 10px; background: var(--surface); font-weight: 620; font-size: .9rem; }
.d-disc > summary::-webkit-details-marker { display: none; }
.d-disc > summary::after { content: ""; margin-left: auto; width: 8px; height: 8px; border-right: 2px solid var(--faint); border-bottom: 2px solid var(--faint); transform: rotate(45deg); transition: transform .2s; }
.d-disc[open] > summary::after { transform: rotate(-135deg); }
.d-nav-sec { margin-bottom: 1.6rem; }
.d-nav-h { font: 600 .68rem/1 var(--mono); letter-spacing: .08em; text-transform: uppercase; color: var(--faint); margin: 0 0 .55rem; padding: 0 .85rem; }
.d-nav-sec ul { list-style: none; margin: 0; padding: 0; display: grid; gap: .06rem; }
.d-nav-sec a { display: block; padding: .42rem .85rem; border-radius: 8px; font-size: .88rem; color: var(--muted); transition: color .15s, background .15s; }
.d-nav-sec a:hover { color: var(--ink); background: var(--tint); }
.d-nav-sec a[aria-current="page"] { color: var(--ink); background: var(--tint-2); font-weight: 620; }
.d-main { min-width: 0; padding: 2.5rem 0 4.5rem; }
.d-hero { max-width: 44rem; margin: 0 0 2.6rem; }
.d-hero h1 { margin-top: .95rem; font-size: clamp(1.9rem, 3.4vw, 2.5rem); font-weight: 780; letter-spacing: -.04em; line-height: 1.08; }
.d-hero p { margin-top: .95rem; font-size: 1.06rem; line-height: 1.62; color: var(--muted); }
.d-hero-actions { margin-top: 1.6rem; display: flex; gap: .7rem; flex-wrap: wrap; }
.d-grid { display: grid; grid-template-columns: repeat(2, 1fr); gap: 1rem; }
.d-grid .t-card p { flex: 0 0 auto; }
.d-cardlinks { list-style: none; margin: 1.1rem 0 0; padding: .95rem 0 0; border-top: 1px solid var(--line); display: grid; gap: .1rem; }
.d-cardlinks a { display: inline-flex; align-items: center; gap: .5rem; padding: .22rem 0; font-size: .87rem; color: var(--muted); transition: color .15s; }
.d-cardlinks a::before { content: ""; width: 4px; height: 4px; border-radius: 1px; background: var(--faint); flex: 0 0 auto; }
.d-cardlinks a:hover { color: var(--ink); }

.doc { max-width: 46rem; color: var(--ink-2); font-size: 1rem; line-height: 1.72; }
.doc > *:first-child { margin-top: 0; }
.doc h1 { font-size: clamp(1.9rem, 3.2vw, 2.4rem); font-weight: 780; letter-spacing: -.04em; line-height: 1.1; color: var(--ink); margin: 0 0 1.1rem; }
.doc h2 { position: relative; font-size: 1.42rem; font-weight: 720; letter-spacing: -.028em; color: var(--ink); margin: 2.7rem 0 1rem; padding-top: 1.8rem; border-top: 1px solid var(--line); }
.doc h3 { position: relative; font-size: 1.13rem; font-weight: 680; letter-spacing: -.018em; color: var(--ink); margin: 2rem 0 .7rem; }
.doc h4 { font-size: 1rem; font-weight: 680; color: var(--ink); margin: 1.6rem 0 .5rem; }
.doc p { margin: 0 0 1.15rem; }
.doc h1 + p { font-size: 1.1rem; line-height: 1.62; color: var(--muted); }
.doc a { color: var(--accent-ink); border-bottom: 1px solid rgba(37,99,201,.3); transition: border-color .15s; }
.doc a:hover { border-bottom-color: var(--accent); }
.doc strong { color: var(--ink); font-weight: 640; }
.doc ul, .doc ol { margin: 0 0 1.15rem; padding-left: 1.35rem; }
.doc li { margin: .35rem 0; padding-left: .2rem; }
.doc li::marker { color: var(--faint); }
.doc li > ul, .doc li > ol { margin: .35rem 0; }
.doc :not(pre) > code { font-size: .85em; background: var(--tint); border-radius: 5px; padding: .1rem .38rem; color: var(--ink-2); }
.doc pre { margin: 0; padding: 1.05rem 1.2rem; background: var(--code-bg); border: 1px solid var(--code-edge); border-radius: 12px; overflow-x: auto; }
.doc pre code { font-size: .82rem; line-height: 1.78; color: var(--code-ink); white-space: pre; }
.doc blockquote { margin: 0 0 1.4rem; padding: .2rem 0 .2rem 1.1rem; border-left: 3px solid var(--line-2); color: var(--muted); }
.doc blockquote p:last-child { margin-bottom: 0; }
.doc hr { border: 0; border-top: 1px solid var(--line); margin: 2.6rem 0; }
.doc table { width: 100%; border-collapse: collapse; font-size: .89rem; margin: 0 0 1.4rem; display: block; overflow-x: auto; }
.doc th, .doc td { padding: .68rem .9rem; text-align: left; border-bottom: 1px solid var(--line); vertical-align: top; }
.doc thead th { font: 600 .68rem/1.3 var(--mono); letter-spacing: .05em; text-transform: uppercase; color: var(--faint); white-space: nowrap; }
.doc tbody td:first-child { font-weight: 570; color: var(--ink); white-space: nowrap; }
.doc .callout { margin: 0 0 1.4rem; padding: 1rem 1.2rem; border-radius: 12px; background: var(--tint); }
.doc .callout-label { font: 600 .68rem/1 var(--mono); letter-spacing: .07em; text-transform: uppercase; margin-bottom: .5rem; color: var(--muted); }
.doc .callout-body > *:last-child { margin-bottom: 0; }
.doc .callout-body p { margin: 0 0 .6rem; font-size: .94rem; }
.doc .callout-note, .doc .callout-info { background: var(--accent-tint); }
.doc .callout-note .callout-label, .doc .callout-info .callout-label { color: var(--accent-ink); }
.doc .callout-tip { background: var(--pass-tint); }
.doc .callout-tip .callout-label { color: var(--pass); }
.doc .callout-warning, .doc .callout-caution { background: var(--warn-tint); }
.doc .callout-warning .callout-label, .doc .callout-caution .callout-label { color: var(--warn); }
.doc-code { position: relative; margin: 0 0 1.4rem; }
.doc-code .t-copy { position: absolute; top: .55rem; right: .55rem; z-index: 2; opacity: 0; transition: opacity .15s, background .15s, color .15s; }
.doc-code:hover .t-copy, .doc-code .t-copy:focus-visible { opacity: 1; }
.doc-code .doc-run { right: 4.1rem; border-bottom: 1px solid rgba(255,255,255,.1); }
.doc .doc-code a.doc-run { color: #a5adba; }
.doc .doc-code a.doc-run:hover { color: #fff; border-color: rgba(255,255,255,.1); }
.doc h2, .doc h3 { scroll-margin-top: 84px; }
.doc .hanchor { margin-left: .45rem; color: var(--faint); border: 0; font-weight: 500; opacity: 0; transition: opacity .15s, color .15s; }
.doc h2:hover .hanchor, .doc h3:hover .hanchor, .doc .hanchor:focus-visible { opacity: 1; }
.doc .hanchor:hover { color: var(--accent); }
.doc-src { margin-top: 3rem; font-size: .84rem; color: var(--faint); }
.doc-src a { color: var(--muted); border-bottom: 1px solid var(--line-2); }
.d-nf { max-width: 34rem; padding: 2.4rem 2rem; border-radius: 16px; background: var(--surface); border: 1px solid var(--line); }
.d-nf h1 { font-size: 1.45rem; font-weight: 720; letter-spacing: -.025em; }
.d-nf p { margin: .7rem 0 1.4rem; font-size: .95rem; line-height: 1.6; color: var(--muted); }
.d-nf code { font-size: .85em; background: var(--tint); border-radius: 5px; padding: .1rem .38rem; }
.d-toc { position: sticky; top: var(--nav-h); align-self: start; max-height: calc(100vh - var(--nav-h)); overflow-y: auto; padding: 2.7rem 0 3rem; }
.d-toc-title { font: 600 .68rem/1 var(--mono); letter-spacing: .08em; text-transform: uppercase; color: var(--faint); margin: 0 0 .75rem; }
.d-toc ul { list-style: none; margin: 0; padding: 0; display: grid; gap: .05rem; border-left: 1px solid var(--line); }
.d-toc a { display: block; margin-left: -1px; padding: .3rem 0 .3rem .9rem; border-left: 2px solid transparent; font-size: .82rem; line-height: 1.4; color: var(--muted); transition: color .15s, border-color .15s; }
.d-toc li.lvl-h3 a { padding-left: 1.7rem; font-size: .8rem; }
.d-toc a:hover { color: var(--ink); }
.d-toc a.active { color: var(--ink); border-left-color: var(--ink); font-weight: 600; }
.d-pager { display: grid; grid-template-columns: 1fr 1fr; gap: 1rem; margin-top: 3rem; padding-top: 2rem; border-top: 1px solid var(--line); max-width: 46rem; }
.d-pager a { display: flex; flex-direction: column; gap: .3rem; padding: 1rem 1.15rem; border: 1px solid var(--line); border-radius: 12px; background: var(--surface); transition: box-shadow .18s, transform .18s; }
.d-pager a:hover { transform: translateY(-2px); box-shadow: 0 18px 40px -30px rgba(12,14,19,.4); }
.d-pager .dir { font: 600 .68rem/1 var(--mono); letter-spacing: .06em; text-transform: uppercase; color: var(--faint); }
.d-pager .ttl { font-size: .97rem; font-weight: 660; letter-spacing: -.012em; }
.d-pager .next { text-align: right; grid-column: 2; }
.d-pager .prev { grid-column: 1; }

/* ---- Playground ---- */
.pg { max-width: 1400px; margin: 0 auto; padding: 0 1.5rem 3rem; }
.pg-head { display: flex; flex-wrap: wrap; align-items: end; gap: 1rem 2rem; padding: 2.4rem 0 1.4rem; }
.pg-head h1 { font-size: clamp(1.6rem, 3vw, 2.1rem); font-weight: 780; letter-spacing: -.04em; line-height: 1.1; }
.pg-head p { margin-top: .5rem; max-width: 40rem; font-size: .97rem; line-height: 1.55; color: var(--muted); }
.pg-app { border-radius: 16px; overflow: hidden; background: var(--code-bg); border: 1px solid var(--code-edge); box-shadow: 0 40px 90px -56px rgba(12,14,19,.8); }
.pg-bar { display: flex; flex-wrap: wrap; align-items: center; gap: .55rem .8rem; padding: .7rem 1rem; background: var(--code-bar); border-bottom: 1px solid var(--code-line); }
.pg-field { display: inline-flex; align-items: center; gap: .45rem; font: 500 .72rem/1 var(--mono); color: var(--code-dim); }
.pg-field select { font: 500 .78rem/1 var(--mono); color: #dde2ea; background: #1a1f29; border: 1px solid #2a303c; border-radius: 7px; padding: .4rem .55rem; cursor: pointer; }
.pg-check { display: inline-flex; align-items: center; gap: .4rem; font: 500 .74rem/1 var(--mono); color: #b7bfcc; cursor: pointer; }
.pg-check input { accent-color: #3b82f6; }
.pg-status { margin-left: auto; display: inline-flex; align-items: center; gap: .45rem; font: 500 .72rem/1 var(--mono); color: var(--code-dim); }
.pg-status::before { content: ""; width: 7px; height: 7px; border-radius: 50%; background: #d08a2e; }
.pg-status[data-state="ready"]::before { background: #6fd39a; }
.pg-status[data-state="error"]::before { background: #ef6b6b; }
.pg-panes { display: grid; grid-template-columns: 1fr 1fr; min-height: 30rem; height: calc(100vh - 21rem); }
.pg-pane { display: flex; flex-direction: column; min-width: 0; min-height: 0; }
.pg-pane + .pg-pane { border-left: 1px solid var(--code-line); }
.pg-tabs { display: flex; align-items: center; gap: .2rem; padding: .45rem .7rem 0; border-bottom: 1px solid var(--code-line); }
.pg-tab { font: 500 .74rem/1 var(--mono); color: var(--code-dim); background: none; border: 0; border-bottom: 2px solid transparent; padding: .5rem .55rem .6rem; cursor: pointer; }
.pg-tab.on { color: #e6eaf0; border-bottom-color: #3b82f6; }
.pg-tab-info { margin-left: auto; font: 500 .7rem/1 var(--mono); color: var(--code-dim); padding-bottom: .15rem; }
.pg-input { flex: 1 1 auto; min-height: 0; width: 100%; resize: none; border: 0; outline: 0; background: transparent; color: var(--code-ink); font: 400 .82rem/1.75 var(--mono); padding: 1rem 1.15rem; tab-size: 2; white-space: pre; overflow: auto; }
.pg-output { flex: 1 1 auto; min-height: 0; margin: 0; padding: 1rem 1.15rem; overflow: auto; }
.pg-output code { font-size: .82rem; line-height: 1.75; color: var(--code-ink); white-space: pre; }
.pg-diags { flex: 0 0 auto; max-height: 11rem; overflow-y: auto; border-top: 1px solid var(--code-line); background: var(--code-bar); padding: .55rem 0; font: 400 .76rem/1.6 var(--mono); }
.pg-diag { display: grid; grid-template-columns: auto auto 1fr; gap: .8rem; padding: .22rem 1.15rem; color: #c9d0da; }
.pg-diag-pos { color: var(--code-dim); }
.pg-diag-code { color: #ef8f8f; }
.pg-diag-none { padding: .22rem 1.15rem; color: #6fd39a; }
.pg-foot { margin-top: 1.1rem; font-size: .85rem; line-height: 1.6; color: var(--faint); max-width: 56rem; }
.pg-foot code { font-size: .86em; background: var(--tint); border-radius: 5px; padding: .08rem .36rem; color: var(--ink-2); }

/* ---- Syntax theme (dark code surfaces) ---- */
.hl-com { color: #8590a0; font-style: italic; }
.hl-str { color: #8fdcaa; }
.hl-num { color: #f0b978; }
.hl-kw { color: #8fb6ff; }
.hl-fn { color: #d9b8ff; }
.hl-type { color: #74d4c6; }
.hl-attr { color: #86cfff; }
.hl-punct { color: #9aa2af; }

/* ---- Scroll reveal (only when the inline head script marked html.js) ---- */
html.js .rv { opacity: 0; transform: translateY(14px); }
html.js .rv.in { opacity: 1; transform: none; transition: opacity .6s cubic-bezier(.2,.7,.2,1), transform .6s cubic-bezier(.2,.7,.2,1); }
html.js .rv .t-meter-fill, html.js .rv .t-bar-fill { width: 0; }
html.js .rv.in .t-meter-fill, html.js .rv.in .t-bar-fill { width: var(--v); transition: width 1s cubic-bezier(.2,.7,.2,1) .15s; }
@media (prefers-reduced-motion: reduce) {
  html.js .rv, html.js .rv.in { opacity: 1; transform: none; transition: none; }
  html.js .rv .t-meter-fill, html.js .rv .t-bar-fill, html.js .rv.in .t-meter-fill, html.js .rv.in .t-bar-fill { width: var(--v); transition: none; }
  .t-btn, .t-btn .ic, .t-card, .d-pager a { transition: none; }
}

/* ---- Responsive ---- */
@media (max-width: 1099px) {
  .d-shell { grid-template-columns: 15.5rem minmax(0, 1fr); }
  .d-toc { display: none; }
}
@media (max-width: 980px) {
  .t-cards { grid-template-columns: repeat(2, 1fr); }
  .t-pipe { grid-template-columns: repeat(2, 1fr); }
  .t-stage + .t-stage::before { display: none; }
  .t-crates { grid-template-columns: repeat(2, 1fr); }
  .t-split { grid-template-columns: minmax(0, 1fr); gap: 2.2rem; }
  .t-score { grid-template-columns: repeat(2, 1fr); }
  .t-block-head { grid-template-columns: minmax(0, 1fr); gap: 1.5rem; }
  .t-foot-top { grid-template-columns: 1fr 1fr; }
  .t-foot-brand { grid-column: 1 / -1; }
}
@media (max-width: 900px) {
  .t-nav-links, .t-nav-right { display: none; }
  .t-nav-toggle { display: inline-flex; }
  .t-nav-mobile { display: block; overflow: hidden; max-height: 0; transition: max-height .28s ease; background: color-mix(in srgb, var(--bg) 98%, transparent); border-bottom: 1px solid var(--line); }
  .t-nav-mobile.open { max-height: 26rem; }
  .t-nav-mobile div { display: flex; flex-direction: column; padding: .5rem 1.5rem 1.3rem; }
  .t-nav-mobile a:not(.t-btn) { padding: .75rem .2rem; font-size: 1rem; font-weight: 550; color: var(--ink-2); border-bottom: 1px solid var(--line); }
  .t-nav-mobile .t-btn { margin-top: 1rem; }
  .d-shell, .d-shell.no-toc { grid-template-columns: 1fr; gap: 0; }
  .d-side { position: static; max-height: none; overflow: visible; padding: 1.4rem 0 0; }
  .d-disc > summary { display: flex; }
  .d-main { padding-top: 1.6rem; }
  .pg-panes { grid-template-columns: 1fr; height: auto; }
  .pg-pane { height: 24rem; }
  .pg-pane + .pg-pane { border-left: 0; border-top: 1px solid var(--code-line); }
}
@media (max-width: 760px) {
  .t-sec { padding: 3.75rem 0; }
  .t-hero { padding: 3.2rem 0 2.4rem; }
  .t-win-panes { grid-template-columns: 1fr; }
  .t-pane + .t-pane { border-left: 0; border-top: 1px solid var(--code-line); }
  .t-win-ok { display: none; }
  .t-cards, .t-bench, .t-prod, .t-limits, .t-facts, .d-grid { grid-template-columns: 1fr; }
  .t-limits { gap: 1.6rem; }
  .t-crates { grid-template-columns: 1fr; }
  .t-lane-row { grid-template-columns: 1fr auto; gap: .5rem 1rem; }
  .t-lane-row .t-meter { grid-column: 1 / -1; order: 3; }
  .t-lane-count { display: none; }
  .d-pager { grid-template-columns: 1fr; }
  .d-pager .next, .d-pager .prev { grid-column: 1; text-align: left; }
  .doc .hanchor { opacity: 1; }
  .doc-code .t-copy { opacity: 1; }
}
@media (max-width: 520px) {
  .t-score, .t-pipe { grid-template-columns: 1fr; }
  .t-foot-top { grid-template-columns: 1fr; }
  .t-hero-meta { gap: .5rem 1.1rem; }
}
`;

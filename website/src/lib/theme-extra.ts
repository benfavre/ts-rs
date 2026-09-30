// Second half of the stylesheet: dark theme tokens, nav tools, search, the
// progress chart and wave log, and the playground editor.
//
// Same authoring rules as theme.ts: no backtick, no dollar-brace and no
// backslash inside the template literals.

// Dark values for every token defined in theme.ts. Applied twice by themeCss():
// under the OS preference (unless the visitor chose light) and under the
// explicit toggle.
const DARK_TOKENS = `
  --bg: #0e1014; --surface: #15181e; --tint: #1a1e26; --tint-2: #242a35;
  --ink: #eef0f4; --ink-2: #d0d5dd; --muted: #a3abb8; --faint: #8b94a2;
  --line: #222730; --line-2: #303642;
  --accent: #6ea3f5; --accent-ink: #8fb8fa; --accent-tint: #16233a;
  --pass: #4cc38a; --pass-tint: #12281e; --warn: #e0a458; --warn-tint: #2c2114; --rust: #e0855a;
  --code-bg: #0a0c10; --code-bar: #10131a; --code-line: #1d222c; --code-edge: #262c38;
  --ink-hover: #ffffff; --on-ink: #0e1014; --shadow: rgba(0,0,0,.6);
  --series-1: #3987e5; --series-2: #d95926;
  color-scheme: dark;
`;

const EXTRA_CSS = `
/* ---- Nav tools: search and theme toggle ---- */
.t-tool { display: inline-flex; align-items: center; justify-content: center; gap: .5rem; height: 2.35rem; min-width: 2.35rem; padding: 0 .6rem; border-radius: 9px; border: 1px solid transparent; background: none; color: var(--muted); cursor: pointer; font: 500 .86rem/1 var(--sans); transition: color .15s, background .15s; }
.t-tool:hover { color: var(--ink); background: var(--tint); }
.t-search-btn { border-color: var(--line-2); background: var(--surface); padding: 0 .55rem 0 .7rem; min-width: 9.5rem; justify-content: flex-start; }
.t-search-btn kbd { margin-left: auto; font: 500 .68rem/1 var(--mono); color: var(--faint); border: 1px solid var(--line-2); border-radius: 5px; padding: .22rem .34rem; }
.t-theme .ic-moon { display: none; }
:root[data-theme="dark"] .t-theme .ic-sun { display: none; }
:root[data-theme="dark"] .t-theme .ic-moon { display: block; }
@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) .t-theme .ic-sun { display: none; }
  :root:not([data-theme="light"]) .t-theme .ic-moon { display: block; }
}
.t-nav-tools-m { display: none; margin-left: auto; align-items: center; gap: .3rem; }

/* ---- Hero demo tabs ---- */
.t-win-tabs { display: flex; gap: .15rem; margin-left: .7rem; overflow-x: auto; }
.t-win-tab { font: 500 .76rem/1 var(--mono); color: var(--code-dim); background: none; border: 0; border-radius: 7px; padding: .45rem .7rem; cursor: pointer; white-space: nowrap; transition: color .15s, background .15s; }
.t-win-tab:hover { color: #dde2ea; }
.t-win-tab[aria-selected="true"] { color: #f1f4f8; background: #222834; }
.t-win-cmdline { display: flex; align-items: center; gap: .8rem; padding: .6rem 1.15rem; border-bottom: 1px solid var(--code-line); }
.t-win-cmdline .t-win-cmd { margin-left: 0; }
.t-win-bad { color: #f0a0a0; background: rgba(239,107,107,.12); }
.t-win-panel .t-win-panes { min-height: 25.5rem; }
.t-pane-wrap code { white-space: pre-wrap; overflow-wrap: anywhere; }

/* ---- Search dialog ---- */
.sr { width: min(40rem, calc(100vw - 2rem)); max-height: min(34rem, calc(100vh - 6rem)); margin: 5rem auto auto; padding: 0; border: 1px solid var(--line-2); border-radius: 16px; background: var(--surface); color: var(--ink); box-shadow: 0 40px 100px -30px var(--shadow); overflow: hidden; }
.sr::backdrop { background: rgba(8,10,14,.5); backdrop-filter: blur(3px); -webkit-backdrop-filter: blur(3px); }
.sr[open] { display: flex; flex-direction: column; }
.sr-bar { display: flex; align-items: center; gap: .7rem; padding: .9rem 1.1rem; border-bottom: 1px solid var(--line); color: var(--faint); }
.sr-input { flex: 1 1 auto; min-width: 0; border: 0; outline: 0; background: none; color: var(--ink); font: 400 1.02rem/1.4 var(--sans); }
.sr-input::placeholder { color: var(--faint); }
.sr-esc { font: 500 .68rem/1 var(--mono); color: var(--faint); border: 1px solid var(--line-2); border-radius: 5px; padding: .25rem .4rem; background: none; cursor: pointer; }
.sr-list { list-style: none; margin: 0; padding: .45rem; overflow-y: auto; }
.sr-item a { display: block; padding: .62rem .75rem; border-radius: 10px; }
.sr-item[aria-selected="true"] a { background: var(--tint); }
.sr-page { font: 600 .66rem/1 var(--mono); letter-spacing: .06em; text-transform: uppercase; color: var(--faint); }
.sr-title { margin-top: .3rem; font-size: .95rem; font-weight: 620; letter-spacing: -.012em; color: var(--ink); }
.sr-snip { margin-top: .2rem; font-size: .84rem; line-height: 1.5; color: var(--muted); display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
.sr-snip mark, .sr-title mark { background: var(--accent-tint); color: inherit; border-radius: 3px; padding: 0 .1em; }
.sr-empty { padding: 1.6rem 1.2rem; font-size: .92rem; color: var(--muted); }
.sr-foot { display: flex; gap: 1.1rem; padding: .6rem 1.1rem; border-top: 1px solid var(--line); font: 500 .7rem/1 var(--mono); color: var(--faint); }

/* ---- Progress chart ---- */
.cv { background: var(--surface); border: 1px solid var(--line); border-radius: 16px; padding: 1.6rem 1.6rem 1.3rem; }
.cv-head { display: flex; flex-wrap: wrap; gap: 1.2rem 3rem; align-items: flex-end; justify-content: space-between; }
.cv-head h2 { font-size: 1.25rem; font-weight: 720; letter-spacing: -.026em; }
.cv-head p { margin-top: .35rem; font-size: .9rem; color: var(--muted); }
.cv-legend { list-style: none; margin: 0; padding: 0; display: flex; flex-wrap: wrap; gap: .8rem 2.2rem; }
.cv-legend li { display: grid; grid-template-columns: auto auto auto; column-gap: .55rem; align-items: center; font-size: .88rem; color: var(--ink-2); }
.cv-legend b { font: 700 1rem/1 var(--mono); letter-spacing: -.03em; color: var(--ink); }
.cv-legend em { grid-column: 2 / -1; margin-top: .3rem; font-style: normal; font-size: .78rem; color: var(--muted); }
.cv-key { display: inline-block; width: 16px; height: 0; border-top: 2px solid var(--series-1); border-radius: 2px; }
.cv-key.cv-s2 { border-top-color: var(--series-2); }
.cv-plot { position: relative; margin-top: 1.5rem; }
.cv-svg { width: 100%; height: auto; overflow: visible; touch-action: pan-y; }
.cv-grid { stroke: var(--line); stroke-width: 1; }
.cv-tick { fill: var(--muted); font: 500 11.5px var(--mono); font-variant-numeric: tabular-nums; }
.cv-line { fill: none; stroke-width: 2; stroke-linejoin: round; stroke-linecap: round; }
.cv-line.cv-s1 { stroke: var(--series-1); }
.cv-line.cv-s2 { stroke: var(--series-2); }
.cv-dot { stroke: var(--surface); stroke-width: 2; }
.cv-dot.cv-s1 { fill: var(--series-1); }
.cv-dot.cv-s2 { fill: var(--series-2); }
.cv-cross { stroke: var(--line-2); stroke-width: 1; }
.cv-tip { position: absolute; top: 0; left: 0; z-index: 3; min-width: 13rem; max-width: 19rem; pointer-events: none; background: var(--surface); border: 1px solid var(--line-2); border-radius: 10px; padding: .7rem .8rem; box-shadow: 0 18px 40px -20px var(--shadow); font-size: .8rem; }
.cv-tip-date { font: 600 .66rem/1 var(--mono); letter-spacing: .06em; text-transform: uppercase; color: var(--faint); }
.cv-tip-title { margin: .35rem 0 .55rem; font-weight: 600; line-height: 1.35; color: var(--ink); }
.cv-tip-row { display: grid; grid-template-columns: auto auto 1fr; gap: .5rem; align-items: center; margin-top: .25rem; color: var(--muted); }
.cv-tip-row b { font: 700 .82rem/1 var(--mono); color: var(--ink); font-variant-numeric: tabular-nums; }
.cv-note { margin-top: .9rem; font-size: .82rem; color: var(--faint); }

/* ---- Wave log ---- */
.wv-side { position: sticky; top: calc(var(--nav-h) + 1.5rem); }
.wv-day { margin: 2.2rem 0 .9rem; font: 600 .72rem/1 var(--mono); letter-spacing: .08em; text-transform: uppercase; color: var(--faint); }
.wv-day:first-child { margin-top: 0; }
.wv-list { list-style: none; margin: 0; padding: 0; display: grid; gap: .7rem; }
.wv { background: var(--surface); border: 1px solid var(--line); border-radius: 12px; padding: 1.1rem 1.25rem 1rem; scroll-margin-top: 84px; }
.wv:target { background: var(--tint); }
.wv-head { display: flex; flex-wrap: wrap; gap: .5rem 1rem; align-items: baseline; justify-content: space-between; }
.wv h4 { font-size: .98rem; font-weight: 660; letter-spacing: -.014em; }
.wv-chips { display: flex; flex-wrap: wrap; gap: .35rem; }
.wv-chip, .wv-code { display: inline-flex; align-items: center; gap: .4rem; font: 500 .7rem/1 var(--mono); padding: .3rem .45rem; border-radius: 6px; background: var(--tint); color: var(--ink-2); white-space: nowrap; }
.wv-code { color: var(--muted); }
.wv p { margin-top: .55rem; font-size: .88rem; line-height: 1.6; color: var(--muted); }
.wv p code { font-size: .86em; background: var(--tint); border-radius: 4px; padding: .05rem .3rem; color: var(--ink-2); }
.wv-foot { margin-top: .75rem; display: flex; flex-wrap: wrap; gap: .4rem 1.2rem; justify-content: space-between; font-size: .78rem; color: var(--faint); }
.wv-foot .t-link { font-size: .78rem; }

/* ---- Latest waves strip (home) ---- */
.nw-wrap { margin-top: 1.4rem; display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)) auto; gap: 1px; background: var(--line); border: 1px solid var(--line); border-radius: 12px; overflow: hidden; }
.nw { display: block; background: var(--surface); padding: .85rem 1.05rem; min-width: 0; transition: background .15s; }
.nw:hover { background: var(--tint); }
.nw-date { display: block; font: 600 .66rem/1 var(--mono); letter-spacing: .06em; text-transform: uppercase; color: var(--faint); }
.nw-title { display: block; margin-top: .4rem; font-size: .86rem; font-weight: 560; line-height: 1.35; color: var(--ink-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.nw-all { display: flex; align-items: center; gap: .4rem; background: var(--surface); padding: 0 1.1rem; font-size: .86rem; font-weight: 600; color: var(--ink); transition: background .15s; }
.nw-all:hover { background: var(--tint); }

/* ---- 404 ---- */
.nf { padding: 6rem 0 5rem; text-align: center; }
.nf-code { font: 700 clamp(4rem, 12vw, 7rem)/1 var(--mono); letter-spacing: -.08em; color: var(--tint-2); }
.nf h1 { margin-top: 1rem; font-size: clamp(1.6rem, 3vw, 2.1rem); font-weight: 760; letter-spacing: -.035em; }
.nf p { max-width: 30rem; margin: .9rem auto 0; color: var(--muted); }
.nf-actions { margin-top: 2rem; display: flex; gap: .7rem; justify-content: center; flex-wrap: wrap; }

/* ---- Playground editor ---- */
.pg-editor { flex: 1 1 auto; min-height: 0; display: grid; grid-template-columns: auto minmax(0, 1fr); overflow: hidden; }
.pg-gutter { overflow: hidden; padding: 1rem 0; text-align: right; color: #7d8594; font: 400 .82rem/1.75 var(--mono); letter-spacing: 0; user-select: none; -webkit-user-select: none; min-width: 2.9rem; }
.pg-gutter span { display: block; padding: 0 .65rem 0 .9rem; }
.pg-gutter span.e { color: #ef8f8f; background: rgba(239,107,107,.13); }
.pg-code { position: relative; min-width: 0; min-height: 0; }
.pg-hl, .pg-editor .pg-input { position: absolute; inset: 0; margin: 0; padding: 1rem 1.15rem 1rem .5rem; border: 0; font: 400 .82rem/1.75 var(--mono); letter-spacing: 0; font-variant-ligatures: none; tab-size: 2; white-space: pre; word-wrap: normal; }
.pg-hl { overflow: hidden; pointer-events: none; color: var(--code-ink); }
.pg-hl .l { display: block; min-height: 1.75em; }
.pg-hl .l.e { background: rgba(239,107,107,.09); box-shadow: 0 0 0 100vmax rgba(239,107,107,.09); clip-path: inset(0 -100vmax); }
.pg-editor .pg-input { overflow: auto; }
.pg-editor.hl-on .pg-input { color: transparent; caret-color: #e6eaf0; }
.pg-input::selection { background: rgba(59,130,246,.38); }
.pg-output { font-size: .82rem; line-height: 1.75; }
.pg-output code { font-size: inherit; line-height: inherit; }
.pg-diag { background: none; border: 0; width: 100%; text-align: left; font: inherit; cursor: pointer; }
.pg-diag:hover { background: rgba(255,255,255,.04); }
.pg-btn { font: 500 .74rem/1 var(--mono); color: #b7bfcc; background: #1a1f29; border: 1px solid #2a303c; border-radius: 7px; padding: .42rem .6rem; cursor: pointer; }
.pg-btn:hover { color: #fff; border-color: #3a4150; }

@media (max-width: 1160px) {
  .t-search-btn { min-width: 2.35rem; padding: 0 .6rem; border-color: transparent; background: none; }
  .t-search-btn span, .t-search-btn kbd { display: none; }
}
@media (max-width: 980px) {
  .wv-side { position: static; }
  .nw-wrap { grid-template-columns: 1fr; }
  .nw-all { padding: .85rem 1.05rem; }
}
@media (max-width: 760px) {
  .cv { padding: 1.2rem 1rem 1rem; }
  .cv-plot { overflow-x: auto; }
  .cv-svg { min-width: 640px; }
}
@media (max-width: 900px) {
  .t-nav-tools-m { display: flex; }
  .t-nav-toggle { margin-left: .2rem; }
}
@media (prefers-reduced-motion: reduce) {
  .sr::backdrop { backdrop-filter: none; -webkit-backdrop-filter: none; }
}
`;

export function extraCss(): string {
  return (
    EXTRA_CSS +
    "@media (prefers-color-scheme: dark){:root:not([data-theme='light']){" + DARK_TOKENS + "}}" +
    ":root[data-theme='dark']{" + DARK_TOKENS + "}"
  );
}

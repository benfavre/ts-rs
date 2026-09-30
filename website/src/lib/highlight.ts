// Server-side syntax highlighting: a small hand-rolled per-language scanner
// that turns source text into <span class="hl-*"> tokens. Highlighting lives in
// the server HTML (no flash, works with JS off). Tuned for the clean samples on
// this site, not adversarial input. Adapted from sites/lite.

export function hlEsc(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

export function hlUnesc(s: string): string {
  return s
    .replace(/&lt;/g, "<").replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"').replace(/&#39;/g, "'")
    .replace(/&amp;/g, "&");
}

function tok(cls: string, text: string): string {
  return '<span class="hl-' + cls + '">' + hlEsc(text) + "</span>";
}

export interface LangCfg {
  line: string[];
  block: boolean;
  quotes: string[];
  literalSingle: boolean;
  numbers: boolean;
  kw: Set<string>;
  types: boolean;
  funcs: boolean;
  jsonKeys: boolean;
  tomlKeys: boolean;
  shell: boolean;
}

function kws(s: string): Set<string> {
  return new Set(s.split(/\s+/));
}

function tsCfg(): LangCfg {
  return {
    line: ["//"], block: true, quotes: ['"', "'", "`"], literalSingle: false, numbers: true,
    kw: kws(
      "const let var function return if else for while do switch case break continue new class " +
      "extends implements interface type enum import export from as default async await yield try catch " +
      "finally throw typeof instanceof in of void delete this super static public private protected " +
      "readonly get set namespace declare abstract null undefined true false keyof infer satisfies is " +
      "module require",
    ),
    types: true, funcs: true, jsonKeys: false, tomlKeys: false, shell: false,
  };
}

function rustCfg(): LangCfg {
  return {
    line: ["//"], block: true, quotes: ['"'], literalSingle: false, numbers: true,
    kw: kws(
      "fn let mut const static struct enum trait impl for while loop if else match return break " +
      "continue use mod pub crate self super as where type dyn move ref async await unsafe extern in box " +
      "true false Some None Ok Err",
    ),
    types: true, funcs: true, jsonKeys: false, tomlKeys: false, shell: false,
  };
}

function bashCfg(): LangCfg {
  return {
    line: ["#"], block: false, quotes: ['"', "'"], literalSingle: true, numbers: false,
    kw: kws(
      "if then else elif fi for while until do done case esac in function select time return " +
      "export local readonly declare unset source exit set shift trap eval exec",
    ),
    types: false, funcs: false, jsonKeys: false, tomlKeys: false, shell: true,
  };
}

function jsonCfg(): LangCfg {
  return {
    line: ["//"], block: false, quotes: ['"'], literalSingle: false, numbers: true,
    kw: kws("true false null"), types: false, funcs: false, jsonKeys: true, tomlKeys: false, shell: false,
  };
}

function tomlCfg(): LangCfg {
  return {
    line: ["#"], block: false, quotes: ['"', "'"], literalSingle: true, numbers: true,
    kw: kws("true false"), types: false, funcs: false, jsonKeys: false, tomlKeys: true, shell: false,
  };
}

export function langCfg(lang: string | undefined): LangCfg | null {
  switch ((lang || "").toLowerCase()) {
    case "ts": case "tsx": case "js": case "jsx": case "javascript": case "typescript":
      return tsCfg();
    case "bash": case "sh": case "shell": case "zsh": case "console":
      return bashCfg();
    case "json": case "jsonc": return jsonCfg();
    case "toml": return tomlCfg();
    case "rust": case "rs": return rustCfg();
    default: return null;
  }
}

function identStart(c: string, cfg: LangCfg): boolean {
  return cfg.shell ? /[A-Za-z_./]/.test(c) : /[A-Za-z_$]/.test(c);
}

function identCont(c: string, cfg: LangCfg): boolean {
  return cfg.shell ? /[A-Za-z0-9_./+-]/.test(c) : /[A-Za-z0-9_$]/.test(c);
}

function nextNonSpace(s: string, from: number): string {
  let k = from;
  while (k < s.length && /\s/.test(s[k])) k++;
  return s[k] || "";
}

function startsLineComment(src: string, i: number, cfg: LangCfg): string | null {
  const prev = i > 0 ? src[i - 1] : "\n";
  for (const p of cfg.line) {
    if (!src.startsWith(p, i)) continue;
    // A hash only opens a comment at the start of a token, so URLs survive.
    if (p === "#" && !(/\s/.test(prev) || prev === "\n")) continue;
    // Two slashes right after a colon are a URL scheme, not a comment.
    if (p === "//" && prev === ":") continue;
    return p;
  }
  return null;
}

export function highlightCode(src: string, cfg: LangCfg): string {
  let out = "";
  let i = 0;
  const n = src.length;
  let atCmd = cfg.shell;
  while (i < n) {
    const c = src[i];
    const prev = i > 0 ? src[i - 1] : "\n";

    if (c === "\n") { out += "\n"; i++; if (cfg.shell) atCmd = true; continue; }
    if (c === " " || c === "\t" || c === "\r") { out += c; i++; continue; }

    if (cfg.block && c === "/" && src[i + 1] === "*") {
      let j = src.indexOf("*/", i + 2);
      j = j === -1 ? n : j + 2;
      out += tok("com", src.slice(i, j)); i = j; continue;
    }

    const lc = startsLineComment(src, i, cfg);
    if (lc !== null) {
      let j = src.indexOf("\n", i);
      j = j === -1 ? n : j;
      out += tok("com", src.slice(i, j)); i = j; continue;
    }

    if (cfg.quotes.indexOf(c) !== -1) {
      const q = c;
      const noEsc = q === "'" && cfg.literalSingle;
      let j = i + 1;
      while (j < n) {
        if (!noEsc && src[j] === "\\") { j += 2; continue; }
        if (src[j] === q) { j++; break; }
        j++;
      }
      if (cfg.jsonKeys && q === '"' && nextNonSpace(src, j) === ":") {
        out += tok("attr", src.slice(i, j));
      } else {
        out += tok("str", src.slice(i, j));
      }
      i = j; if (cfg.shell) atCmd = false; continue;
    }

    if (cfg.shell) {
      if (c === "$") {
        if (src[i + 1] === "{") {
          let j = src.indexOf("}", i + 2);
          j = j === -1 ? n : j + 1;
          out += tok("attr", src.slice(i, j)); i = j; atCmd = false; continue;
        }
        const m = /^\$[A-Za-z0-9_@#?!*-]*/.exec(src.slice(i)) as RegExpExecArray;
        out += tok("attr", m[0]); i += m[0].length; atCmd = false; continue;
      }
      if (c === "-" && (/\s/.test(prev) || prev === "\n")) {
        const m = /^--?[A-Za-z0-9][A-Za-z0-9-]*/.exec(src.slice(i));
        if (m) { out += tok("attr", m[0]); i += m[0].length; atCmd = false; continue; }
      }
    }

    if (cfg.numbers && (/[0-9]/.test(c) || (c === "." && /[0-9]/.test(src[i + 1] || "")))) {
      const m = /^(0[xXbBoO][0-9a-fA-F_]+|\d[\d_]*(\.\d[\d_]*)?([eE][+-]?\d+)?)/.exec(src.slice(i));
      if (m) { out += tok("num", m[0]); i += m[0].length; if (cfg.shell) atCmd = false; continue; }
    }
    if (!cfg.numbers && /[0-9]/.test(c)) { out += hlEsc(c); i++; continue; }

    if (identStart(c, cfg)) {
      let j = i + 1;
      while (j < n && identCont(src[j], cfg)) j++;
      const word = src.slice(i, j);
      const nxt = nextNonSpace(src, j);
      let cls = "";
      if (cfg.kw.has(word)) {
        cls = "kw";
      } else if (cfg.shell && atCmd) {
        cls = "fn";
      } else if (cfg.tomlKeys && nxt === "=") {
        cls = "attr";
      } else if (cfg.funcs && nxt === "(") {
        cls = "fn";
      } else if (cfg.types && /^[A-Z]/.test(word)) {
        cls = "type";
      }
      out += cls ? tok(cls, word) : hlEsc(word);
      i = j;
      if (cfg.shell) {
        atCmd = cfg.kw.has(word) && /^(then|do|else|elif|in|time)$/.test(word);
      }
      continue;
    }

    out += tok("punct", c);
    i++;
    if (cfg.shell) {
      if (c === "|" || c === "&" || c === ";" || c === "(") atCmd = true;
      else atCmd = false;
    }
  }
  return out;
}

// Highlight a snippet by language name; unknown languages come back escaped.
export function highlight(src: string, lang: string): string {
  const cfg = langCfg(lang);
  return cfg ? highlightCode(src, cfg) : hlEsc(src);
}

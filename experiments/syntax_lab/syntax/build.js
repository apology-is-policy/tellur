// Builds villanelle-before-after.html from the sources in v1/ and v2/.
// Usage: node build.js   (run from any directory)
const fs = require("fs");
const path = require("path");
const here = __dirname;
const read = (p) => fs.readFileSync(path.join(here, p), "utf8").replace(/\s+$/, "") + "\n";

const KW = new Set(("control exits delta exit forward capture loop again leave if else for in emit spawn into await " +
  "cancel run async pub import as outcome variant struct trait implement function extern resource view faults fault " +
  "supervise supervised boundary contingency terminate retains drop clone yield defer type on").split(" "));
const PROOF = new Set("where when impossible assume uses protocol".split(" "));
const MOD = new Set("ref mutable auto borrow deref move from".split(" "));
const TYPE = new Set("u8 u16 u32 u64 i32 i64 bool string bytes unit vector map option tasks slice array box".split(" "));

const esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

function payloadAhead(s) {
  if (/^\s*\}/.test(s)) return true;
  return /^\s*(?:ref\s+(?:mutable\s+)?)?[A-Za-z_][\w.]*(?:<[^{};]*>)?(?:\.[A-Za-z_]\w*)*\s+[a-z_]\w*\s*(?:,|\}|from\b)/.test(s);
}

function tokenizeVn(src) {
  const out = [];
  const push = (c, t) => out.push({ c, t });
  // Names bound by `capture` hold a completed outcome; `exit name;` forwards a value, not a label.
  const captured = new Set([...src.matchAll(/\b([a-z_]\w*)\s*=\s*capture\b/g)].map((m) => m[1]));
  let i = 0, brace = 0, paren = 0, prev = "";
  let pending = null;          // { paren } after `delta`, `outcome`, `variant`
  let contract = null;         // { brace, paren, start, afterLabel }
  const ctx = [];              // { arms, depth, paren, start }
  const top = () => ctx[ctx.length - 1];
  const armHead = () => { const t = top(); return t && t.arms && brace === t.depth && paren === t.paren && t.start; };
  const contractHead = () => contract && contract.start && brace === contract.brace && paren === contract.paren;
  const inArms = () => { const t = top(); return t && t.arms && brace === t.depth && paren === t.paren; };
  while (i < src.length) {
    const ch = src[i], rest = src.slice(i);
    let m;
    if (ch === "\n") { push(null, "\n"); i++; continue; }
    if ((m = /^[ \t]+/.exec(rest))) { push(null, m[0]); i += m[0].length; continue; }
    if (rest.startsWith("//")) { m = /^[^\n]*/.exec(rest); push("com", m[0]); i += m[0].length; continue; }
    if (ch === '"') {
      m = /^"(?:[^"\\\n]|\\.)*"/.exec(rest);
      const s = m ? m[0] : ch;
      let last = 0; const re = /\{[a-z_][\w.]*\}/g; let k;
      while ((k = re.exec(s))) { if (k.index > last) push("str", s.slice(last, k.index)); push("interp", k[0]); last = k.index + k[0].length; }
      if (last < s.length) push("str", s.slice(last));
      i += s.length; prev = "str"; continue;
    }
    if ((m = /^\d+(?:u32)?/.exec(rest))) { push("num", m[0]); i += m[0].length; prev = "num"; continue; }
    if (ch === "[" && (m = /^\[\s*[a-z_][\w.]*(?:\s*[&|]\s*[a-z_][\w.]*)*\s*\]\s*:/.exec(rest))) {
      for (const part of m[0].split(/([&|])/)) push(part === "&" || part === "|" ? "labelop" : "label", part);
      i += m[0].length; prev = "label"; continue;
    }
    if (ch === "@" && (m = /^@[a-z_]+/.exec(rest))) { push("attr", m[0]); i += m[0].length; prev = "attr"; continue; }
    if (rest.startsWith("..") && (m = /^\.\.([a-z_]\w*)/.exec(rest))) {
      push("spread", ".."); push("group", m[1]); i += m[0].length; prev = "group";
      if (armHead()) top().start = false;
      if (contractHead()) { contract.start = false; contract.afterLabel = false; }
      continue;
    }
    if (rest.startsWith("=>") || rest.startsWith("->")) { push("arrow", rest.slice(0, 2)); i += 2; prev = "=>"; continue; }
    if ((m = /^[A-Za-z_]\w*/.exec(rest))) {
      const w = m[0]; i += w.length;
      const call = /^\s*\(/.test(src.slice(i));
      let c = null, labelled = false;
      if (prev === ".") c = call ? "call" : null;
      else if (w === "next") c = prev === "await" ? "kw" : (armHead() ? "out" : null);
      else if (w === "true" || w === "false") { c = "bool"; if (armHead()) top().start = false; }
      else if (contractHead() && w === "auto") { c = "mod"; contract = null; }
      else if (KW.has(w) && !armHead() && !contractHead()) {
        c = "kw";
        if (w === "delta" || w === "outcome" || w === "variant") pending = { paren };
        if (w === "exits") contract = { brace, paren, start: true, afterLabel: false };
      }
      else if (PROOF.has(w) && !armHead()) { c = "proof"; if (w === "uses" || w === "protocol") contract = null; }
      else if (MOD.has(w) && !armHead()) c = "mod";
      else if (TYPE.has(w) && !armHead() && !contractHead()) c = "type";
      else if (armHead()) { c = "out"; top().start = false; }
      else if (contractHead()) { c = "out"; contract.start = false; contract.afterLabel = true; labelled = true; }
      else if ((prev === "exit" || prev === "leave") && !captured.has(w)) c = "out";
      else if (prev === "=>" && inArms()) c = /^\s*\{/.test(src.slice(i)) ? "out" : "path";
      else if (call) c = "call";
      if (contract && !labelled) contract.afterLabel = false;
      push(c, w); prev = w; continue;
    }
    // punctuation
    if (ch === "{") {
      let opened = false;
      if (contract && brace === contract.brace && paren === contract.paren) {
        if (contract.afterLabel && payloadAhead(src.slice(i + 1))) { ctx.push({ arms: false }); contract.afterLabel = false; opened = true; }
        else contract = null;
      }
      if (!opened) {
        if (pending && paren === pending.paren) { ctx.push({ arms: true, depth: brace + 1, paren, start: true }); pending = null; }
        else ctx.push({ arms: false });
      }
      brace++;
    } else if (ch === "}") { brace--; ctx.pop(); }
    else if (ch === "(") paren++;
    else if (ch === ")") { paren--; if (contract && paren < contract.paren) contract = null; }
    else if (ch === ",") {
      const t = top();
      if (t && t.arms && brace === t.depth && paren === t.paren) t.start = true;
      if (contract && brace === contract.brace && paren === contract.paren) { contract.start = true; contract.afterLabel = false; }
    } else if (ch === ";") { pending = null; if (contract && brace === contract.brace) contract = null; }
    push(null, ch); prev = ch; i++;
  }
  return out;
}

function tokenizeJson(src) {
  const out = []; let i = 0, m;
  while (i < src.length) {
    const rest = src.slice(i);
    if ((m = /^"(?:[^"\\]|\\.)*"/.exec(rest))) {
      const key = /^\s*:/.test(src.slice(i + m[0].length));
      out.push({ c: key ? "key" : "str", t: m[0] }); i += m[0].length; continue;
    }
    if ((m = /^(?:true|false|null)\b/.exec(rest))) { out.push({ c: "bool", t: m[0] }); i += m[0].length; continue; }
    if ((m = /^-?\d+(?:\.\d+)?/.exec(rest))) { out.push({ c: "num", t: m[0] }); i += m[0].length; continue; }
    out.push({ c: null, t: src[i] }); i++;
  }
  return out;
}

function render(src, lang) {
  const toks = lang === "json" ? tokenizeJson(src) : tokenizeVn(src);
  const lines = [[]];
  for (const { c, t } of toks) {
    const parts = t.split("\n");
    parts.forEach((p, k) => {
      if (k > 0) lines.push([]);
      if (p) lines[lines.length - 1].push(c ? `<span class="t-${c}">${esc(p)}</span>` : esc(p));
    });
  }
  if (lines.length && lines[lines.length - 1].length === 0) lines.pop();
  // No newline text between the block-level line spans: inside <pre> it would render as a blank line.
  return lines.map((l) => `<span class="l">${l.join("")}</span>`).join("");
}

const countLines = (files) => files.reduce((n, f) => n + f.code.split("\n").filter((l) => l.trim()).length, 0);
const F = (file, p, lang) => ({ file, code: read(p), lang: lang || "vn" });
const code = (s) => s.replace(/`([^`]+)`/g, (_, x) => `<code>${esc(x)}</code>`);

const samples = [
  {
    id: "file", title: "Read a file, always close it",
    lead: "The language's signature case: the close must run whatever the read did, and both results stay distinguishable. Little changes here, and that is the point.",
    before: [F("playground/read_file.vn", "v1/read_file.vn")],
    after: [F("playground/read_file.vn", "v2/read_file.vn")],
    notes: [
      "The `[entry]:` and `[open]:` labels are gone. After `open`, two alternatives exit and the rest of the body runs on the one that is left.",
      "Paths take their default names from the operation: `read.failed`, `close.failed`. The left side needed invented names (`read_bad`, `close_bad`) to tell them apart.",
      "`close(move file)` marks the call that ends the resource. Reading it with `read(file)` needs no mark.",
      "Payloads nobody uses are not bound.",
    ],
  },
  {
    id: "settings", title: "Settings: load, set, get",
    lead: "Policy code that mostly maps one set of outcomes onto another. This is where today's ceremony concentrates.",
    before: [F("applications/settings/settings.vn, lines 1 to 86", "v1/settings.vn")],
    after: [F("applications/settings/settings.vn", "v2/settings.vn")],
    notes: [
      "`storage_error` is a named, closed group. Three contracts include it with `..storage_error`, and callers forward it as a unit. Adding a member is one edit that every including contract shows.",
      "In an adapter, a bare name forwards that outcome unchanged. Only the alternatives that change are spelled out.",
      "`validate_key` was a control with two exits wrapped around a boolean. It becomes a function, and its callers test it with `if`.",
      "`loaded { mutable values }` binds the payload as mutable, so the copy into a second variable disappears.",
      "Every alternative is still named where it is decided. If `json_file.replace` gains an outcome, both sides stop compiling at the adapter.",
    ],
  },
  {
    id: "search", title: "Binary search",
    lead: "Six of the arms on the left cannot occur. Each was given an exit so that the control would compile.",
    before: [F("library/ordered_search.vn, without first_equal", "v1/ordered_search.vn")],
    after: [F("library/ordered_search.vn", "v2/ordered_search.vn")],
    notesTitle: "What the compiler proves on the right",
    notes: [
      "`count / 2`: the divisor is a literal.",
      "`start + half` and `source[middle]`: the invariant and the `count == 0` test give `middle < len(source)`.",
      "`middle + 1` and `count - half - 1`: `half < count`, so neither can wrap.",
      "Each `again` re-establishes `start + count <= len(source)`.",
      "`when index <= len(source)` is a promise to callers, proved at the one `exit`.",
      "Delete the `where` line and the control stops compiling, with a counterexample.",
    ],
  },
  {
    id: "first", title: "First match, and its caller",
    lead: "A library control that takes a test, and a program that uses it twice. Today each test needs a trait, a struct and an implementation.",
    before: [F("library/search.vn", "v1/search.vn"), F("examples/first_match/main.vn", "v1/first_match.vn")],
    after: [F("library/search.vn", "v2/search.vn"), F("examples/first_match/main.vn", "v2/first_match.vn")],
    notes: [
      "`for value in source` walks by shared borrow. The loan it yields can be returned, as `from source` declares.",
      "A test that cannot fail is a function value: `(value) => value >= limit`. A test with its own outcomes is a control passed by name.",
      "`skipped,` names the alternative and continues with the next element. `failed => exit` leaves with the same outcome and payload.",
      "Type arguments are inferred at the call.",
      "Text interpolation replaces `u32_text` and `concat`.",
    ],
  },
  {
    id: "server", title: "Server accept loop",
    lead: "The largest control in the application. Today a body may hold one tail loop, so admission and draining share it and the reason for stopping travels in a state variable.",
    before: [F("applications/settings/server.vn, lines 1 to 125", "v1/server.vn")],
    after: [F("applications/settings/server.vn", "v2/server.vn")],
    notes: [
      "Two loops in order: admit, then drain. `leave quiet` ends the first loop with an outcome, and that outcome is a path fact for everything after it.",
      "The `admission` variant and the `state` parameter are gone. The fact `[quiet]` or `[broken]` already records why admission ended, across the drain loop.",
      "`empty => impossible` is proved from `active.count() >= 64`. `remaining - 1` is proved from the test above it.",
      "`[stopped]: cancel active;` runs on one history. The drain that follows runs on all four.",
      "The right side is intended to behave exactly as the left does.",
    ],
  },
  {
    id: "contracts", title: "Contracts for host operations",
    lead: "Primitive contracts move from the JSON manifest into source, where they can say more.",
    before: [F("applications/settings/manifest.json, three entries", "v1/manifest.json", "json")],
    after: [F("the same three declarations", "v2/contracts.vn")],
    notes: [
      "`when` states the condition under which each outcome occurs. This is what lets a caller prove an arm impossible.",
      "`uses` names the effect. Today the checker ignores it; here it is checked and can appear in policies.",
      "`protocol` gives the permitted order of operations on a connection. `linear: true` is the special case that only demands an ending.",
      "Rust binding details such as `execution: poll` would stay in a small adapter section, not shown.",
    ],
  },
];

const pane = (side, files, meta) => `
      <figure class="pane ${side}">
        <figcaption><span class="tag">${side === "before" ? "Before" : "After"}</span><span class="meta">${meta}</span></figcaption>
        ${files.map((f) => `<div class="file">${esc(f.file)}</div>
        <pre class="code" tabindex="0"><code>${render(f.code, f.lang)}</code></pre>`).join("\n        ")}
      </figure>`;

const sections = samples.map((s) => {
  const a = countLines(s.before), b = countLines(s.after);
  return `
  <section id="${s.id}">
    <header class="shead">
      <h2>${esc(s.title)}</h2>
      <p class="count"><span>${a}</span> lines <span class="to">to</span> <span>${b}</span></p>
    </header>
    <p class="lead">${esc(s.lead)}</p>
    <div class="pair">${pane("before", s.before, `0.306 · compiles today · ${a} lines`)}${pane("after", s.after, `proposed · ${b} lines`)}
    </div>
    <div class="notes">
      <h3>${esc(s.notesTitle || "What changed")}</h3>
      <ul>${s.notes.map((n) => `<li>${code(n)}</li>`).join("")}</ul>
    </div>
  </section>`;
}).join("\n");

const nav = samples.map((s) => `<a href="#${s.id}">${esc(s.title)}</a>`).join("");
const html = fs.readFileSync(path.join(here, "template.html"), "utf8")
  .replace("<!--NAV-->", nav).replace("<!--SECTIONS-->", sections);
fs.writeFileSync(path.join(here, "villanelle-before-after.html"), html);
console.log(samples.map((s) => `${s.id}: ${countLines(s.before)} -> ${countLines(s.after)}`).join("\n"));

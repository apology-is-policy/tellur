// Villanelle syntax lab: parse the proposed word syntax and re-render it in several glyph styles.
// Exports { render(source, styleId) -> lines of [class, text] segments, STYLES }.

// ---------------------------------------------------------------- tokenizer
function lex(src) {
  const toks = [];
  let i = 0, line = 1, lineStart = 0, nl = 0, sp = false;
  const push = (t, v, n) => { toks.push({ t, v, line, col: i - lineStart, nl, sp }); nl = 0; sp = false; i += n; };
  while (i < src.length) {
    const c = src[i];
    if (c === "\n") { nl++; line++; i++; lineStart = i; sp = true; continue; }
    if (c === " " || c === "\t") { sp = true; i++; continue; }
    const rest = src.slice(i);
    let m;
    if (rest.startsWith("//")) { m = /^[^\n]*/.exec(rest); push("com", m[0].slice(2).trim(), m[0].length); continue; }
    if (c === '"') { m = /^"(?:[^"\\\n]|\\.)*"/.exec(rest); push("str", m[0], m[0].length); continue; }
    if ((m = /^\d+/.exec(rest))) { push("num", m[0], m[0].length); continue; }
    if ((m = /^@?[A-Za-z_]\w*/.exec(rest))) { push("id", m[0], m[0].length); continue; }
    if ((m = /^(=>|->|\.\.|==|!=|<=|>=|&&|\|\|)/.exec(rest))) { push("p", m[0], m[0].length); continue; }
    push("p", c, 1);
  }
  toks.push({ t: "eof", v: "", line: line + 1, col: 0, nl: 1, sp: false });
  return toks;
}

// ------------------------------------------------------------------- parser
class Parser {
  constructor(toks) { this.k = toks; this.i = 0; }
  get tok() { return this.k[this.i]; }
  peek(n = 1) { return this.k[Math.min(this.i + n, this.k.length - 1)]; }
  is(v) { const t = this.tok; return t.v === v && t.t !== "str" && t.t !== "com"; }
  isId(v) { return this.tok.t === "id" && (v === undefined || this.tok.v === v); }
  next() { return this.k[this.i++]; }
  fail(msg) { throw new Error(`line ${this.tok.line}: ${msg}, got '${this.tok.v}'`); }
  expect(v) { if (!this.is(v)) this.fail(`expected '${v}'`); return this.next(); }
  accept(v) { if (this.is(v)) { this.next(); return true; } return false; }
  name() { if (this.tok.t !== "id") this.fail("expected a name"); return this.next().v; }
  qname() { let n = this.name(); while (this.is(".")) { this.next(); n += "." + this.name(); } return n; }

  file() {
    const ds = [];
    while (this.tok.t !== "eof") {
      if (this.tok.nl >= 2 && ds.length) ds.push({ k: "blank" });
      ds.push(this.decl());
    }
    return ds;
  }
  decl() {
    if (this.tok.t === "com") return { k: "comment", text: this.next().v };
    let pub = false;
    if (this.isId("pub")) { pub = true; this.next(); }
    if (this.isId("import")) { this.next(); const p = this.next().v; this.expect("as"); const name = this.name(); this.expect(";"); return { k: "import", path: p, name }; }
    if (this.isId("outcome")) return this.outcomeDecl(pub);
    if (this.isId("function")) return this.functionDecl(pub);
    if (this.isId("extern")) return this.externDecl();
    let isAsync = false;
    if (this.isId("async")) { isAsync = true; this.next(); }
    if (this.isId("control")) {
      this.next();
      const name = this.name(), generics = this.generics(), params = this.params();
      const exits = this.isId("exits") ? this.exits() : null;
      return { k: "control", pub, async: isAsync, name, generics, params, exits, body: this.block() };
    }
    this.fail("expected a declaration");
  }
  outcomeDecl(pub) {
    const start = this.expect("outcome"); const name = this.name(); this.expect("{");
    const alts = [];
    while (!this.is("}")) {
      const a = { status: null };
      if (this.isId("@exit")) { this.next(); a.status = this.next().v; }
      a.name = this.name(); a.payload = this.is("{") ? this.payloadDecl() : null;
      alts.push(a); this.accept(",");
    }
    const close = this.expect("}");
    return { k: "outcome", pub, name, alts, inline: close.line === start.line };
  }
  functionDecl(pub) {
    this.expect("function"); const name = this.name(); const params = this.params(); this.expect("->");
    const ret = this.type(); this.expect("{"); const expr = this.exprUntil((t) => t.v === "}"); this.expect("}");
    return { k: "function", pub, name, params, ret, expr };
  }
  externDecl() {
    this.expect("extern");
    if (this.isId("resource")) {
      this.next(); const name = this.name(); this.expect("protocol");
      const protocol = this.exprUntil((t) => (t.t === "id" && t.v === "on"), true);
      let onFault = false;
      if (this.isId("on")) { this.next(); this.expect("fault"); this.expect("release"); onFault = true; }
      this.expect(";");
      return { k: "resource", name, protocol, onFault };
    }
    let isAsync = false;
    if (this.isId("async")) { isAsync = true; this.next(); }
    this.expect("control");
    const name = this.name(), params = this.params(), exits = this.exits(), uses = [];
    if (this.isId("uses")) { this.next(); do { uses.push(this.qname()); } while (this.accept(",")); }
    this.expect(";");
    return { k: "extern", async: isAsync, name, params, exits, uses };
  }
  generics() {
    const g = [];
    if (this.is("<")) {
      this.next();
      while (!this.is(">")) { const name = this.name(); const bound = this.accept(":") ? this.qname() : null; g.push({ name, bound }); this.accept(","); }
      this.expect(">");
    }
    return g;
  }
  params() {
    this.expect("("); const ps = [];
    while (!this.is(")")) { const type = this.type(); ps.push({ type, name: this.name() }); this.accept(","); }
    this.expect(")"); return ps;
  }
  typeList() { this.expect("("); const ts = []; while (!this.is(")")) { ts.push(this.type()); this.accept(","); } this.expect(")"); return ts; }
  type() {
    if (this.isId("ref")) { this.next(); let mut = false; if (this.isId("mutable")) { mut = true; this.next(); } return { k: "ref", mut, inner: this.type() }; }
    if (this.isId("function") && this.peek().v === "(") { this.next(); const params = this.typeList(); this.expect("->"); return { k: "fn", params, ret: this.type() }; }
    if (this.isId("control") && this.peek().v === "(") { this.next(); const params = this.typeList(); return { k: "ctl", params, exits: this.exits() }; }
    const name = this.qname(); const args = [];
    if (this.is("<")) { this.next(); while (!this.is(">")) { args.push(this.type()); this.accept(","); } this.expect(">"); }
    return { k: "name", name, args };
  }
  payloadDecl() {
    this.expect("{"); const fs = [];
    while (!this.is("}")) { const type = this.type(); const name = this.name(); let from = null; if (this.isId("from")) { this.next(); from = this.name(); } fs.push({ type, name, from }); this.accept(","); }
    this.expect("}"); return fs;
  }
  payloadAhead() {
    const save = this.i;
    try {
      this.expect("{"); if (this.is("}")) return true;
      this.type(); if (this.tok.t !== "id") return false; this.next();
      return this.is(",") || this.is("}") || this.isId("from");
    } catch (e) { return false; } finally { this.i = save; }
  }
  exits() {
    this.expect("exits");
    if (this.isId("auto")) { this.next(); return { auto: true, alts: [] }; }
    const alts = [];
    for (;;) {
      if (this.is("..")) { this.next(); alts.push({ group: this.name() }); }
      else {
        const a = { name: this.name(), payload: null, when: null };
        if (this.is("{") && this.payloadAhead()) a.payload = this.payloadDecl();
        if (this.isId("when")) { this.next(); a.when = this.exprUntil((t) => t.v === "," || t.v === "{" || t.v === ";" || (t.t === "id" && t.v === "uses")); }
        alts.push(a);
      }
      if (!this.accept(",")) break;
    }
    return { auto: false, alts };
  }
  block() { this.expect("{"); const s = this.stmts(null); this.expect("}"); return s; }
  stmts(minCol) {
    const out = [];
    while (!this.is("}") && this.tok.t !== "eof" && (minCol === null || this.tok.col > minCol)) {
      if (this.tok.nl >= 2 && out.length) out.push({ k: "blank" });
      out.push(this.stmt());
    }
    return out;
  }
  // Collect tokens up to a stop token at nesting depth zero. `raw` keeps `;` inside (protocols).
  exprUntil(stop, raw) {
    const out = []; let depth = 0;
    while (this.tok.t !== "eof") {
      const t = this.tok;
      if (depth === 0 && t.t !== "str" && t.t !== "com" && stop(t)) break;
      if (t.t === "p") {
        if ("([{".includes(t.v)) depth++;
        else if (")]}".includes(t.v)) { if (depth === 0) break; depth--; }
        else if (t.v === ";" && depth === 0 && !raw) break;
      }
      out.push(this.next());
    }
    return out;
  }
  exprList(open, close) {
    this.expect(open); const items = [];
    while (!this.is(close)) { items.push(this.exprUntil((t) => t.v === ",")); this.accept(","); }
    this.expect(close); return items;
  }
  outcomeRef() {
    if (this.tok.t !== "id") return { name: null, payload: null };
    const name = this.name();
    return { name, payload: this.is("{") ? this.exprList("{", "}") : null };
  }
  label() {
    const start = this.expect("["); const mask = [];
    while (!this.is("]")) { if (this.is("&") || this.is("|")) mask.push({ op: this.next().v }); else mask.push({ name: this.qname() }); }
    this.expect("]"); this.expect(":");
    if (this.tok.line === start.line) {
      const body = [];
      while (this.tok.line === start.line && !this.is("}") && this.tok.t !== "eof") body.push(this.stmt());
      return { k: "label", mask, body, inline: true };
    }
    return { k: "label", mask, body: this.stmts(start.col), inline: false };
  }
  delta(ctx) {
    const start = this.expect("delta"); const operand = { await: false, next: false, run: false };
    if (this.isId("await")) { operand.await = true; this.next(); }
    if (this.isId("run")) { operand.run = true; this.next(); }
    if (operand.await && this.isId("next")) { operand.next = true; this.next(); }
    operand.expr = this.exprUntil((t) => t.v === "{"); this.expect("{");
    const arms = [];
    while (!this.is("}")) { arms.push(this.arm(ctx)); this.accept(","); }
    const close = this.expect("}");
    return { k: "delta", operand, arms, inline: close.line === start.line };
  }
  arm(ctx) {
    const a = { target: null };
    if (this.is("..")) { this.next(); a.group = this.name(); }
    else {
      a.name = this.name();
      if (this.is("{")) {
        this.next(); a.pattern = [];
        while (!this.is("}")) { let mut = false; if (this.isId("mutable")) { mut = true; this.next(); } a.pattern.push({ mut, name: this.name() }); this.accept(","); }
        this.expect("}");
      }
    }
    if (!this.accept("=>")) return a;
    if (this.isId("exit")) { this.next(); a.target = { k: "exit", ...this.outcomeRef() }; }
    else if (this.isId("leave")) { this.next(); a.target = { k: "leave", ...this.outcomeRef() }; }
    else if (this.isId("again")) { this.next(); const name = this.name(); a.target = { k: "again", name, args: this.is("(") ? this.exprList("(", ")") : null }; }
    else if (this.isId("impossible")) { this.next(); a.target = { k: "impossible" }; }
    else if (this.isId("forward")) { this.next(); a.target = { k: "forward" }; }
    else if (this.is("{")) a.target = { k: "block", body: this.block() };
    else {
      const name = this.name();
      if (this.is("{")) a.target = { k: "make", name, payload: this.exprList("{", "}") };
      else a.target = { k: ctx === "adapter" ? "make" : "path", name, payload: null };
    }
    return a;
  }
  tryBinding() {
    const save = this.i;
    try {
      let mut = false, type = null;
      if (this.isId("mutable")) { mut = true; this.next(); }
      if (this.isId("auto")) this.next();
      else if (!(this.tok.t === "id" && this.peek().v === "=")) type = this.type();
      if (this.tok.t !== "id") { this.i = save; return null; }
      const name = this.next().v;
      if (!this.is("=")) { this.i = save; return null; }
      this.next();
      let init;
      if (this.isId("capture")) { this.next(); let run = false; if (this.isId("run")) { run = true; this.next(); } init = { capture: true, run, call: this.exprUntil(() => false) }; }
      else init = { expr: this.exprUntil(() => false) };
      this.expect(";");
      return { k: "bind", mut, type, name, init };
    } catch (e) { this.i = save; return null; }
  }
  stmt() {
    const t = this.tok;
    if (t.t === "com") return { k: "comment", text: this.next().v };
    if (this.is("[")) return this.label();
    if (this.isId("delta")) return this.delta("flat");
    if (this.isId("exit")) {
      this.next();
      if (this.isId("delta")) { const adapter = this.delta("adapter"); this.accept(";"); return { k: "exit", adapter }; }
      const r = this.outcomeRef(); this.accept(";"); return { k: "exit", ...r };
    }
    if (this.isId("leave")) { this.next(); const r = this.outcomeRef(); this.accept(";"); return { k: "leave", ...r }; }
    if (this.isId("again")) { this.next(); const name = this.name(); const args = this.is("(") ? this.exprList("(", ")") : null; this.accept(";"); return { k: "again", name, args }; }
    if (this.isId("forward")) { this.next(); const call = this.exprUntil(() => false); this.expect(";"); return { k: "forward", call }; }
    if (this.isId("if")) {
      this.next(); const cond = this.exprUntil((x) => x.v === "{"); const then = this.block();
      const oneLine = this.k[this.i - 1].line === t.line;
      return { k: "if", cond, then, oneLine };
    }
    if (this.isId("loop")) {
      this.next(); const name = this.name(); const params = [];
      if (this.is("(")) {
        this.next();
        while (!this.is(")")) {
          let mut = false; if (this.isId("mutable")) { mut = true; this.next(); }
          const type = this.type(); const pname = this.name(); this.expect("=");
          params.push({ mut, type, name: pname, init: this.exprUntil((x) => x.v === ",") }); this.accept(",");
        }
        this.expect(")");
      }
      let where = null;
      if (this.isId("where")) { this.next(); where = this.exprUntil((x) => x.v === "{"); }
      return { k: "loop", name, params, where, body: this.block() };
    }
    if (this.isId("for")) { this.next(); const name = this.name(); this.expect("in"); const iter = this.exprUntil((x) => x.v === "{"); return { k: "for", name, iter, body: this.block() }; }
    if (this.isId("emit") && this.peek().v === "(") { this.next(); this.expect("("); const arg = this.exprUntil(() => false); this.expect(")"); this.accept(";"); return { k: "emit", arg }; }
    if (this.isId("spawn")) { this.next(); const call = this.exprUntil((x) => x.t === "id" && x.v === "into"); this.expect("into"); const into = this.name(); this.accept(";"); return { k: "spawn", call, into }; }
    if (this.isId("cancel")) { this.next(); const name = this.name(); this.accept(";"); return { k: "cancel", name }; }
    const b = this.tryBinding();
    if (b) return b;
    const expr = this.exprUntil(() => false); this.expect(";");
    return { k: "call", expr };
  }
}

// ------------------------------------------------------------------- styles
const BASE = {
  layout: "braces",          // braces | indent | rails
  delta: "Δ", arrow: "→", exit: "■", leave: "□", again: "↑", loop: "↻", forall: "∀", isin: "∈",
  where: "⊢", when: "⇐", impossible: "⊥", guard: "?", ref: "&", mut: "~", move: "▾", bind: ":=",
  spread: "…", lambda: "↦", status: "↦", mapArrow: "⇀", altSep: ", ", comment: "//", label: "bracket",
  control: ["kw", "control"], func: ["kw", "function"], outcome: ["kw", "outcome"], pub: ["kw", "pub"],
  async: ["kw", "async"], extern: ["kw", "extern"], resource: ["kw", "resource"], uses: ["kw", "uses"],
  auto: ["kw", "auto"], importForm: "words",
  emit: null, spawn: null, await: ["kw", "await"], next: ["kw", "next"], cancel: ["kw", "cancel"],
  capture: ["kw", "capture"], run: ["kw", "run"], forward: ["kw", "forward"], clone: null, armBullet: null,
};
const STYLES = {
  notation: { ...BASE, id: "notation", name: "Notation" },
  flow: {
    ...BASE, id: "flow", name: "Flow", layout: "indent", delta: "◇", exit: "●", leave: "○", altSep: " | ", label: "pointer",
    emit: "»", spawn: "⇉", await: ["g", "⋯"], cancel: ["g", "⊘"], capture: ["g", "◈"], forward: ["g", "↠"],
  },
  rails: { ...BASE, id: "rails", name: "Rails", layout: "rails", altSep: " │ ", label: "junction", comment: "┄" },
  glyph: {
    ...BASE, id: "glyph", name: "Glyph", layout: "indent", altSep: " | ", label: "white", comment: "⍝", armBullet: "▹",
    control: ["g", "⌥"], func: ["g", "λ"], outcome: ["g", "⊎"], pub: ["g", "+"], async: ["g", "⋯"], extern: ["g", "⌂"],
    resource: ["g", "◍"], uses: ["g", "↯"], auto: ["kw", "_"], importForm: "arrow",
    emit: "»", spawn: "⇉", await: ["g", "⋯"], next: null, cancel: ["g", "⊘"], capture: ["g", "◈"], run: ["g", "▷"],
    forward: ["g", "↠"], clone: "≡",
  },
};

// ------------------------------------------------------------------ printer
const OPS = { "==": "=", "!=": "≠", "<=": "≤", ">=": "≥", "&&": "∧", "||": "∨", "/": "÷" };
const len = (segs) => segs.reduce((n, s) => n + [...s[1]].length, 0);
const sep = (lists, s) => { const out = []; lists.forEach((l, i) => { if (i) out.push([null, s]); out.push(...l); }); return out; };
const WIDTH = 96;

class Printer {
  constructor(st) { this.st = st; this.out = []; this.pre = []; this.captured = new Set(); }
  L(segs, pre) { this.out.push([...(pre || this.pre), ...segs]); }
  blockSeg(kind) {
    if (this.st.layout !== "rails") return [null, "    "];
    return ["rail", { body: "│ ", loop: "║ ", cond: "┆ ", armMid: "│ ", armLast: "  " }[kind]];
  }
  nest(kind, fn) { this.pre.push(this.blockSeg(kind)); fn(); this.pre.pop(); }
  braces() { return this.st.layout === "braces"; }

  // ---- expressions
  str(v) {
    const out = []; let last = 0; const re = /\{[a-z_][\w.]*\}/g; let k;
    while ((k = re.exec(v))) { if (k.index > last) out.push(["str", v.slice(last, k.index)]); out.push(["interp", k[0]]); last = k.index + k[0].length; }
    if (last < v.length) out.push(["str", v.slice(last)]);
    return out;
  }
  expr(toks) {
    const st = this.st, out = []; let glue = true;
    for (let i = 0; i < toks.length; i++) {
      const t = toks[i], n = toks[i + 1];
      if (!glue && (t.sp || t.nl)) out.push([null, " "]);
      glue = false;
      if (t.v === "(" && n && n.t === "id" && toks[i + 2] && toks[i + 2].v === ")" && toks[i + 3] && toks[i + 3].v === "=>") {
        out.push([null, n.v], [null, " "], ["arrow", st.lambda]); i += 3; continue;
      }
      if (t.t === "id") {
        if (t.v === "move") { out.push(["own", st.move]); glue = true; continue; }
        if (t.v === "mutable") { out.push(["own", st.mut]); glue = true; continue; }
        if (t.v === "map" && n && n.v === "{" && toks[i + 2] && toks[i + 2].v === "}") { out.push(["num", "∅"]); i += 2; continue; }
        if (t.v === "clone" && st.clone && n && n.v === "(") {
          let j = i + 2, depth = 0; const inner = [];
          for (; j < toks.length; j++) { const x = toks[j]; if (x.v === "(") depth++; if (x.v === ")") { if (depth === 0) break; depth--; } inner.push(x); }
          out.push(["own", st.clone], ...this.expr(inner)); i = j; continue;
        }
        if (t.v === "true" || t.v === "false") { out.push(["num", t.v]); continue; }
        out.push([n && n.v === "(" ? "call" : null, t.v]); continue;
      }
      if (t.t === "str") { out.push(...this.str(t.v)); continue; }
      if (t.t === "num") { out.push(["num", t.v]); continue; }
      if (t.v === "!") { out.push(["op", "¬"]); glue = true; continue; }
      out.push([OPS[t.v] ? "op" : null, OPS[t.v] || t.v]);
    }
    return out;
  }
  exprLines(toks) {          // split a multi-line expression into its source lines
    const lines = [[]];
    for (const t of toks) { if (t.nl && lines[lines.length - 1].length) lines.push([]); lines[lines.length - 1].push(t); }
    return lines.map((l) => this.expr(l));
  }
  args(list) { return [[null, "("], ...sep(list.map((e) => this.expr(e)), ", "), [null, ")"]]; }
  payload(list) { return [[null, " {"], ...sep(list.map((e) => this.expr(e)), ", "), [null, "}"]]; }

  // ---- types and contracts
  type(t) {
    const st = this.st;
    if (t.k === "ref") return [["own", st.ref + (t.mut ? st.mut : "")], ...this.type(t.inner)];
    if (t.k === "fn") { const ps = sep(t.params.map((p) => this.type(p)), ", "); return [...(t.params.length === 1 ? ps : [[null, "("], ...ps, [null, ")"]]), [null, " "], ["arrow", st.arrow], [null, " "], ...this.type(t.ret)]; }
    if (t.k === "ctl") return [[null, "("], ...sep(t.params.map((p) => this.type(p)), ", "), [null, ") "], ...this.exitsInline(t.exits)];
    if (t.name === "vector" && t.args.length === 1) return [["type", "["], ...this.type(t.args[0]), ["type", "]"]];
    if (t.name === "map" && t.args.length === 2) return [...this.type(t.args[0]), [null, " "], ["type", st.mapArrow], [null, " "], ...this.type(t.args[1])];
    const builtin = /^(u8|u16|u32|u64|i32|i64|bool|string|bytes|unit|tasks|option|slice|box)$/.test(t.name);
    const out = [[builtin ? "type" : null, t.name]];
    if (t.args.length) out.push([null, "<"], ...sep(t.args.map((a) => this.type(a)), ", "), [null, ">"]);
    return out;
  }
  field(f) { const out = [[null, f.name], [null, ": "], ...this.type(f.type)]; if (f.from) out.push([null, " "], ["own", this.st.isin], [null, " " + f.from]); return out; }
  payloadDecl(fs) { return [[null, " {"], ...sep(fs.map((f) => this.field(f)), ", "), [null, "}"]]; }
  alt(a) {
    if (a.group) return [["spread", this.st.spread], ["out", a.group]];
    const out = [["out", a.name]];
    if (a.payload) out.push(...this.payloadDecl(a.payload));
    if (a.when) out.push([null, " "], ["proof", this.st.when], [null, " "], ...this.expr(a.when));
    return out;
  }
  exitsInline(ex) {
    const st = this.st;
    if (ex.auto) return [["g", st.exit], [null, " "], st.auto];
    return [["g", st.exit], [null, " "], ...sep(ex.alts.map((a) => this.alt(a)), st.altSep)];
  }
  // signature: head segments, then exits; wraps to continuation lines when long
  signature(head, params, exits, opener) {
    const st = this.st, ps = params.map((p) => [[null, p.name], [null, ": "], ...this.type(p.type)]);
    let first = [...head, [null, "("], ...sep(ps, ", "), [null, ")"]];
    const lines = [];
    if (len(first) > WIDTH) {            // wrap parameters greedily
      first = [...head, [null, "("]]; lines.push(first); let cur = [[null, "    "]];
      ps.forEach((p, i) => {
        const piece = [...p, [null, i < ps.length - 1 ? "," : ""]];
        if (len(cur) > 4 && len(cur) + len(piece) + 1 > WIDTH) { lines.push(cur); cur = [[null, "    "]]; }
        if (len(cur) > 4) cur.push([null, " "]);
        cur.push(...piece);
      });
      lines.push(cur); lines.push([[null, ")"]]);
    } else lines.push(first);
    if (exits) {
      const perLine = !exits.auto && exits.alts.length > 1 && exits.alts.some((a) => a.when);
      const inline = this.exitsInline(exits), last = lines[lines.length - 1];
      if (perLine) for (const a of exits.alts) lines.push([[null, "    "], ["g", st.exit], [null, " "], ...this.alt(a)]);
      else if (len(last) + 1 + len(inline) + (opener ? 2 : 0) <= WIDTH) last.push([null, " "], ...inline);
      else lines.push([[null, "    "], ...inline]);
    }
    if (opener) { if (lines.length === 1) lines[0].push([null, " {"]); else lines.push([[null, "{"]]); }
    lines.forEach((l) => this.L(l));
  }

  // ---- declarations
  file(ds) {
    ds.forEach((d) => this.decl(d));
    while (this.out.length && len(this.out[this.out.length - 1]) === 0) this.out.pop();
    return this.out;
  }
  head(d, word) { const st = this.st, h = []; if (d.pub) h.push(st.pub, [null, " "]); if (d.async) h.push(st.async, [null, st.async[0] === "g" ? "" : " "]); h.push(word, [null, " "]); return h; }
  decl(d) {
    const st = this.st;
    if (d.k === "blank") return this.L([]);
    if (d.k === "comment") return this.L([["com", st.comment + " " + d.text]]);
    if (d.k === "import") return this.L(st.importForm === "arrow" ? [[null, d.name + " "], ["g", "⇠"], [null, " "], ["str", d.path]] : [["kw", "import"], [null, " "], ["str", d.path], [null, " "], ["kw", "as"], [null, " " + d.name]]);
    if (d.k === "outcome") return this.outcomeDecl(d);
    if (d.k === "function") {
      const head = [...this.head(d, st.func), ["call", d.name]];
      const ps = d.params.map((p) => [[null, p.name], [null, ": "], ...this.type(p.type)]);
      const sig = [...head, [null, "("], ...sep(ps, ", "), [null, "): "], ...this.type(d.ret), [null, " " + st.bind]];
      const body = this.expr(d.expr);
      if (len(sig) + 1 + len(body) <= WIDTH) return this.L([...sig, [null, " "], ...body]);
      this.L(sig); return this.L([[null, "    "], ...body]);
    }
    if (d.k === "extern") {
      const head = [st.extern, [null, " "]]; if (d.async) head.push(st.async, [null, st.async[0] === "g" ? "" : " "]);
      head.push(st.control, [null, " "], ["call", d.name]);
      this.signature(head, d.params, d.exits, false);
      if (d.uses.length) this.L([[null, "    "], st.uses, [null, " " + d.uses.join(", ")]]);
      return;
    }
    if (d.k === "resource") {
      this.L([st.extern, [null, " "], st.resource, [null, " " + d.name]]);
      const chunks = [[]];
      for (const t of d.protocol) { chunks[chunks.length - 1].push(t); if (t.v === ";") chunks.push([]); }
      chunks.filter((c) => c.length).forEach((c, i) => {
        const text = c.map((t, j) => (j && t.sp ? " " : "") + (t.v === "|" && st.layout === "rails" ? "│" : t.v)).join("");
        this.L([[null, "    "], i ? [null, "         "] : ["kw", "protocol"], [null, " " + text]]);
      });
      if (d.onFault) this.L([[null, "    "], ["kw", "on fault release"]]);
      return;
    }
    if (d.k === "control") {
      const head = [...this.head(d, st.control), ["call", d.name]];
      if (d.generics.length) head.push([null, "<"], ...sep(d.generics.map((g) => [[null, g.name + (g.bound ? ": " + g.bound : "")]]), ", "), [null, ">"]);
      this.signature(head, d.params, d.exits, this.braces());
      this.nest("body", () => this.stmts(d.body));
      if (this.braces()) this.L([[null, "}"]]);
      else if (st.layout === "rails") this.L([["rail", "╵"]]);
    }
  }
  outcomeDecl(d) {
    const st = this.st, head = [...this.head(d, st.outcome), ["out", d.name]];
    const alts = d.alts.map((a) => { const s = [["out", a.name]]; if (a.payload) s.push(...this.payloadDecl(a.payload)); if (a.status !== null) s.push([null, " "], ["arrow", st.status], [null, " "], ["num", a.status]); return s; });
    if (d.inline) return this.L(this.braces() ? [...head, [null, " { "], ...sep(alts, ", "), [null, " }"]] : [...head, [null, ": "], ...sep(alts, st.altSep)]);
    this.L(this.braces() ? [...head, [null, " {"]] : head);
    alts.forEach((a, i) => {
      const lastOne = i === alts.length - 1;
      if (st.layout === "rails") this.L([["rail", lastOne ? "└ " : "├ "], ...a]);
      else this.L([[null, "    "], ...(st.armBullet ? [["g", st.armBullet], [null, " "]] : []), ...a, ...(this.braces() && !lastOne ? [[null, ","]] : [])]);
    });
    if (this.braces()) this.L([[null, "}"]]);
  }

  // ---- statements
  stmts(list) {
    for (let i = 0; i < list.length; i++) {
      const s = list[i];
      if (s.k === "label" && s.inline) {       // align a run of one-line labels
        let j = i; while (j < list.length && list[j].k === "label" && list[j].inline) j++;
        const heads = list.slice(i, j).map((x) => this.labelHead(x, true)), w = Math.max(...heads.map((h) => len(h.segs)));
        for (let k = i; k < j; k++) {
          const h = heads[k - i], body = sep(list[k].body.map((b) => this.simple(b)), "; ");
          this.L([...h.segs, [null, " ".repeat(w - len(h.segs) + 1)], ...body], h.pre);
        }
        i = j - 1; continue;
      }
      this.stmt(s);
    }
  }
  mask(m) { return m.map((x) => (x.op ? ["labelop", x.op === "&" ? " ∧ " : " ∨ "] : ["path", x.name])); }
  labelHead(s, inline) {
    const st = this.st, m = this.mask(s.mask);
    let segs, pre = null;
    if (st.label === "bracket") segs = [["path", "["], ...m, ["path", "]:"]];
    else if (st.label === "white") segs = [["path", "⟦"], ...m, ["path", "⟧"], ...(inline ? [[null, ":"]] : [])];
    else if (st.label === "pointer") segs = [["g", "▸"], [null, " "], ...m, ...(inline ? [[null, ":"]] : [])];
    else {                                    // junction on the enclosing rail
      const last = this.pre[this.pre.length - 1];
      pre = [...this.pre.slice(0, -1), ["rail", last && last[1] === "║ " ? "╠═" : "╞═"]];
      segs = [[null, " "], ...m, ...(inline ? [[null, ":"]] : [])];
    }
    return { segs, pre };
  }
  isSimple(s) { return ["leave", "again", "forward", "emit", "spawn", "cancel", "bind", "call"].includes(s.k) || (s.k === "exit" && !s.adapter) || (s.k === "delta" && s.inline && s.arms.every((a) => !a.target || a.target.k !== "block")); }
  outcomeRef(glyph, r) {
    const out = [["g", glyph]];
    if (r.name) { out.push([null, " "], [this.captured.has(r.name) ? null : "out", r.name]); if (r.payload) out.push(...this.payload(r.payload)); }
    return out;
  }
  simple(s) {
    const st = this.st;
    switch (s.k) {
      case "exit": return this.outcomeRef(st.exit, s);
      case "leave": return this.outcomeRef(st.leave, s);
      case "again": return [["g", st.again], [null, " "], ["call", s.name], ...(s.args ? this.args(s.args) : [])];
      case "forward": return [st.forward, [null, " "], ...this.expr(s.call)];
      case "emit": return st.emit ? [["g", st.emit], [null, " "], ...this.expr(s.arg)] : [["kw", "emit"], [null, "("], ...this.expr(s.arg), [null, ")"]];
      case "spawn": return st.spawn ? [["g", st.spawn], [null, " "], ...this.expr(s.call), [null, " "], ["arrow", st.arrow], [null, " " + s.into]] : [["kw", "spawn"], [null, " "], ...this.expr(s.call), [null, " "], ["kw", "into"], [null, " " + s.into]];
      case "cancel": return [st.cancel, [null, " " + s.name]];
      case "call": return this.expr(s.expr);
      case "bind": {
        const out = [];
        if (s.mut) out.push(["own", st.mut]);
        out.push([null, s.name]);
        if (s.type) out.push([null, ": "], ...this.type(s.type));
        out.push([null, " " + st.bind + " "]);
        if (s.init.capture) { this.captured.add(s.name); out.push(st.capture, [null, " "]); if (s.init.run) out.push(st.run, [null, " "]); out.push(...this.expr(s.init.call)); }
        else out.push(...this.expr(s.init.expr));
        return out;
      }
      case "delta": return [...this.deltaHead(s), ...this.inlineArms(s)];
    }
    throw new Error("not simple: " + s.k);
  }
  deltaHead(d) {
    const st = this.st, out = [["g", st.delta], [null, " "]];
    if (d.operand.await) out.push(st.await, [null, " "]);
    if (d.operand.run) out.push(st.run, [null, " "]);
    if (d.operand.next && st.next) out.push(st.next, [null, " "]);
    out.push(...this.expr(d.operand.expr));
    return out;
  }
  armHead(a) {
    if (a.group) return [["spread", this.st.spread], ["out", a.group]];
    const bool = a.name === "true" || a.name === "false";
    const out = [[bool ? "num" : "out", a.name]];
    if (a.pattern) out.push([null, " {"], ...sep(a.pattern.map((p) => [...(p.mut ? [["own", this.st.mut]] : []), [null, p.name]]), ", "), [null, "}"]);
    return out;
  }
  armTarget(t) {
    const st = this.st;
    switch (t.k) {
      case "path": return [["path", t.name]];
      case "exit": return this.outcomeRef(st.exit, t);
      case "leave": return this.outcomeRef(st.leave, t);
      case "again": return [["g", st.again], [null, " "], ["call", t.name], ...(t.args ? this.args(t.args) : [])];
      case "impossible": return [["proof", st.impossible]];
      case "forward": return [st.forward];
      case "make": return [["out", t.name], ...(t.payload ? this.payload(t.payload) : [])];
    }
  }
  armLine(a, pad) {
    const h = this.armHead(a);
    if (!a.target) return h;
    const out = [...h, [null, " ".repeat(Math.max(0, pad - len(h)) + 1)], ["arrow", this.st.arrow]];
    if (a.target.k !== "block") out.push([null, " "], ...this.armTarget(a.target));
    return out;
  }
  inlineArms(d) {
    const st = this.st, arms = d.arms.map((a) => this.armLine(a, 0));
    if (this.braces()) return [[null, " { "], ...sep(arms, ", "), [null, " }"]];
    return [[null, ": "], ...sep(arms, st.layout === "rails" ? " │ " : " | ")];
  }
  arms(d) {                                  // vertical arms under the current line
    const st = this.st, heads = d.arms.filter((a) => a.target).map((a) => len(this.armHead(a)));
    const pad = heads.length ? Math.min(Math.max(...heads), 26) : 0;
    d.arms.forEach((a, i) => {
      const lastOne = i === d.arms.length - 1, line = this.armLine(a, pad), isBlock = a.target && a.target.k === "block";
      if (st.layout === "rails") {
        this.L([["rail", lastOne ? "└ " : "├ "], ...line]);
        if (isBlock) { this.pre.push(this.blockSeg(lastOne ? "armLast" : "armMid")); this.nest("cond", () => this.stmts(a.target.body)); this.pre.pop(); }
      } else {
        const bullet = st.armBullet ? [["g", st.armBullet], [null, " "]] : [];
        this.L([[null, "    "], ...bullet, ...line, ...(isBlock && this.braces() ? [[null, " {"]] : []), ...(this.braces() && !isBlock ? [[null, ","]] : [])]);
        if (isBlock) { this.pre.push([null, "    "]); this.nest("cond", () => this.stmts(a.target.body)); this.pre.pop(); if (this.braces()) this.L([[null, "    },"]]); }
      }
    });
  }
  stmt(s) {
    const st = this.st;
    if (s.k === "blank") return this.L([]);
    if (s.k === "comment") return this.L([["com", st.comment + " " + s.text]]);
    if (s.k === "label") {
      const h = this.labelHead(s, false);
      this.L(h.segs, h.pre);
      return this.nest("cond", () => this.stmts(s.body));
    }
    if (this.isSimple(s)) return this.L(this.simple(s));
    if (s.k === "delta" || (s.k === "exit" && s.adapter)) {
      const d = s.k === "delta" ? s : s.adapter;
      const head = [...(s.k === "exit" ? [["g", st.exit], [null, " "]] : []), ...this.deltaHead(d)];
      if (d.inline && d.arms.every((a) => !a.target || a.target.k !== "block")) return this.L([...head, ...this.inlineArms(d)]);
      this.L(this.braces() ? [...head, [null, " {"]] : head);
      this.arms(d);
      if (this.braces()) this.L([[null, "}"]]);
      return;
    }
    if (s.k === "if") {
      const lines = this.exprLines(s.cond), guard = [["g", st.guard], [null, " "]];
      const one = s.oneLine && s.then.length === 1 && this.isSimple(s.then[0]);
      lines.forEach((l, i) => {
        const lastOne = i === lines.length - 1;
        const tail = lastOne ? [[null, " "], ["arrow", st.arrow], ...(one ? [[null, " "], ...this.simple(s.then[0])] : this.braces() ? [[null, " {"]] : [])] : [];
        this.L([...(i ? [[null, "  "]] : guard), ...l, ...tail]);
      });
      if (!one) { this.nest("cond", () => this.stmts(s.then)); if (this.braces()) this.L([[null, "}"]]); }
      return;
    }
    if (s.k === "loop" || s.k === "for") {
      let head;
      if (s.k === "for") head = [["g", st.forall], [null, " " + s.name + " "], ["g", st.isin], [null, " "], ...this.expr(s.iter)];
      else {
        head = [["g", st.loop], [null, " "], ["call", s.name]];
        if (s.params.length) head.push([null, "("], ...sep(s.params.map((p) => [...(p.mut ? [["own", st.mut]] : []), [null, p.name + ": "], ...this.type(p.type), [null, " " + st.bind + " "], ...this.expr(p.init)]), ", "), [null, ")"]);
      }
      const inv = s.where ? [["proof", st.where], [null, " "], ...this.expr(s.where)] : null;
      if (this.braces()) {
        if (inv) { this.L(head); this.L([[null, "    "], ...inv]); this.L([[null, "{"]]); } else this.L([...head, [null, " {"]]);
        this.nest("loop", () => this.stmts(s.body)); this.L([[null, "}"]]);
      } else {
        this.L(head);
        this.nest("loop", () => { if (inv) this.L(inv); this.stmts(s.body); });
      }
      return;
    }
    throw new Error("unhandled statement " + s.k);
  }
}

function render(source, styleId) {
  const ast = new Parser(lex(source)).file();
  const lines = new Printer(STYLES[styleId]).file(ast);
  // trim trailing spaces that are not rails
  return lines.map((l) => { const out = l.slice(); while (out.length && out[out.length - 1][0] === null && /^\s*$/.test(out[out.length - 1][1])) out.pop(); if (out.length && out[out.length - 1][0] === "rail") out[out.length - 1] = ["rail", out[out.length - 1][1].replace(/\s+$/, "")]; return out; });
}
const text = (lines) => lines.map((l) => l.map((s) => s[1]).join("")).join("\n");
module.exports = { render, text, STYLES, lex, Parser };

if (require.main === module) {
  const fs = require("fs");
  const [file, style] = process.argv.slice(2);
  console.log(text(render(fs.readFileSync(file, "utf8"), style || "notation")));
}

// Builds villanelle-glyph-studies.html: six programs x five renderings, with an embedded font subset.
const fs = require("fs"), path = require("path"), cp = require("child_process");
const here = __dirname, lab = require("./lab.js");
const syn = path.join(here, "..", "syntax");
const b = fs.readFileSync(path.join(syn, "build.js"), "utf8");
const { tokenizeVn } = new Function(b.slice(b.indexOf("const KW"), b.indexOf("function tokenizeJson")) + "; return { tokenizeVn };")();
const esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
const src = (f) => fs.readFileSync(path.join(syn, "v2", f), "utf8").replace(/\s+$/, "") + "\n";

const programs = [
  { id: "file", title: "Read a file, always close it", files: [["playground/read_file.vn", "read_file.vn"]] },
  { id: "settings", title: "Settings: load, set, get", files: [["applications/settings/settings.vn", "settings.vn"]] },
  { id: "search", title: "Binary search", files: [["library/ordered_search.vn", "ordered_search.vn"]] },
  { id: "first", title: "First match, and its caller", files: [["library/search.vn", "search.vn"], ["examples/first_match/main.vn", "first_match.vn"]] },
  { id: "server", title: "Server accept loop", files: [["applications/settings/server.vn", "server.vn"]] },
  { id: "contracts", title: "Contracts for host operations", files: [["host contracts", "contracts.vn"]] },
];
const styles = [
  { id: "words", name: "Words", note: "The proposal from the previous page. Every other panel is generated from this text." },
  { id: "notation", name: "Notation", note: "Braces stay. Flow keywords and operators become glyphs; declarations and rare operations keep their words. The smallest step, closest to Lean or Julia." },
  { id: "flow", name: "Flow", note: "No braces: indentation carries structure. Statement kinds take flowchart shapes, and task operations become glyphs too." },
  { id: "rails", name: "Rails", note: "Notation's glyphs, with structure drawn in box characters. A solid rail is a body, a double rail repeats, a dashed rail runs only on some paths, and a decision forks into branches." },
  { id: "glyph", name: "Glyph", note: "The furthest point. Declarations become glyphs as well, labels use white brackets and arms are bulleted." },
];

const wordLines = (code) => {
  const lines = [[]];
  for (const { c, t } of tokenizeVn(code)) t.split("\n").forEach((p, k) => { if (k) lines.push([]); if (p) lines[lines.length - 1].push([c, p]); });
  while (lines.length && !lines[lines.length - 1].length) lines.pop();
  return lines;
};
const html = (lines) => lines.map((l) => `<span class="l">${l.map(([c, t]) => (c ? `<span class="t-${c}">${esc(t)}</span>` : esc(t))).join("")}</span>`).join("");
const count = (lines) => lines.filter((l) => l.map((s) => s[1]).join("").replace(/[│║┆╵\s]/g, "").length).length;

let used = new Set(" !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~");
const note = (lines) => lines.forEach((l) => l.forEach((s) => [...s[1]].forEach((ch) => used.add(ch))));

const sections = programs.map((p) => {
  const panes = styles.map((st) => {
    let total = 0;
    const blocks = p.files.map(([label, file]) => {
      const lines = st.id === "words" ? wordLines(src(file)) : lab.render(src(file), st.id);
      note(lines); total += count(lines);
      return `<div class="file">${esc(label)}</div><pre class="code" tabindex="0"><code>${html(lines)}</code></pre>`;
    }).join("");
    return `<figure class="pane ${st.id}"><figcaption><span class="tag">${st.name}</span><span class="meta">${total} lines</span></figcaption>${blocks}</figure>`;
  }).join("\n      ");
  return `<section id="${p.id}"><h2>${esc(p.title)}</h2><div class="grid">\n      ${panes}\n    </div></section>`;
}).join("\n  ");

// vocabulary: concept, then Words | Notation | Flow | Rails | Glyph ("=" repeats the cell to its left)
const vocab = [
  ["Decide on an operation", "delta", "Δ", "◇", "Δ", "Δ"],
  ["Route an alternative", "=>", "→", "=", "=", "="],
  ["Alternatives", "{ a, b }", "{ a, b }", "indented, or a | b", "├ a  └ b", "▹ a  ▹ b"],
  ["Exit the control", "exit, exits", "■", "●", "■", "■"],
  ["Leave a loop with an outcome", "leave", "□", "○", "□", "□"],
  ["Loop, and go round again", "loop, again", "↻  ↑", "=", "↻ ↑ on a ║ rail", "↻  ↑"],
  ["For each element", "for x in xs", "∀ x ∈ xs", "=", "=", "="],
  ["Boolean test", "if c { … }", "? c → …", "=", "=", "="],
  ["On these paths", "[a & b]:", "[a ∧ b]:", "▸ a ∧ b", "╞═ a ∧ b, then ┆", "⟦a ∧ b⟧"],
  ["Must be proved here", "where", "⊢", "=", "=", "="],
  ["Outcome occurs when", "when", "⇐", "=", "=", "="],
  ["Cannot occur", "impossible", "⊥", "=", "=", "="],
  ["Shared reference", "ref T", "&T", "=", "=", "="],
  ["Mutable", "mutable", "~x  &~T", "=", "=", "="],
  ["Give the value away", "move x", "▾x", "=", "=", "="],
  ["Borrowed from", "from s", "∈ s", "=", "=", "="],
  ["Bind a name", "T x = e", "x: T := e", "=", "=", "="],
  ["Compare and combine", "== != <= >= && || !", "= ≠ ≤ ≥ ∧ ∨ ¬", "=", "=", "="],
  ["Integer division", "/", "÷", "=", "=", "="],
  ["Vector and map types", "vector<T>  map<K, V>", "[T]  K ⇀ V", "=", "=", "="],
  ["Function value", "(x) => e", "x ↦ e", "=", "=", "="],
  ["Named group of outcomes", "..group", "…group", "=", "=", "="],
  ["Process status", "@exit 0 done", "done ↦ 0", "=", "=", "="],
  ["Empty map", "map {}", "∅", "=", "=", "="],
  ["Block", "{ … }", "{ … }", "indentation", "│ rail, ╵ end", "indentation"],
  ["Comment", "//", "//", "//", "┄", "⍝"],
  ["Emit", "emit(x)", "emit(x)", "» x", "emit(x)", "» x"],
  ["Start a task in a set", "spawn f() into s", "spawn f() into s", "⇉ f() → s", "spawn f() into s", "⇉ f() → s"],
  ["Await, cancel", "await  cancel s", "await  cancel s", "⋯  ⊘ s", "await  cancel s", "⋯  ⊘ s"],
  ["Capture, forward", "capture  forward", "capture  forward", "◈  ↠", "capture  forward", "◈  ↠"],
  ["Declarations", "control function outcome", "control function outcome", "=", "=", "⌥  λ  ⊎"],
  ["Public, async, host", "pub  async  extern", "pub  async  extern", "=", "=", "+  ⋯  ⌂"],
  ["Import", "import \"p\" as n", "import \"p\" as n", "=", "=", "n ⇠ \"p\""],
  ["Copy", "clone(x)", "clone(x)", "=", "=", "≡x"],
];
vocab.forEach((r) => r.slice(1).forEach((c) => [...c].forEach((ch) => used.add(ch))));
const vocabRows = vocab.map((r) => {
  const cells = []; let prev = "";
  r.slice(1).forEach((c) => { const v = c === "=" ? prev : c; prev = v; cells.push(`<td${c === "=" ? ' class="same"' : ""}>${esc(v)}</td>`); });
  return `<tr><th scope="row">${esc(r[0])}</th>${cells.join("")}</tr>`;
}).join("\n        ");

// ---- embedded font: DejaVu Sans Mono, subset to the characters used
const home = process.env.HOME, chars = [...used].join("");
fs.writeFileSync(path.join(here, "chars.txt"), chars);
const font = (file, out) => {
  cp.execFileSync("python3", ["-m", "fontTools.subset", path.join(home, "Library/Fonts", file), `--text-file=${path.join(here, "chars.txt")}`, "--flavor=woff2", `--output-file=${path.join(here, out)}`, "--layout-features=", "--no-hinting"], { env: { ...process.env, PYTHONPATH: path.join(here, "py") }, stdio: ["ignore", "ignore", "inherit"] });
  return fs.readFileSync(path.join(here, out)).toString("base64");
};
const regular = font("DejaVu Sans Mono for Powerline.ttf", "vn-regular.woff2");
const bold = font("DejaVu Sans Mono Bold for Powerline.ttf", "vn-bold.woff2");

const page = fs.readFileSync(path.join(here, "template.html"), "utf8")
  .replace("/*FONTS*/", `@font-face{font-family:"VN Mono";font-weight:400;src:url(data:font/woff2;base64,${regular}) format("woff2")}\n@font-face{font-family:"VN Mono";font-weight:700;src:url(data:font/woff2;base64,${bold}) format("woff2")}`)
  .replace("<!--NAV-->", programs.map((p) => `<a href="#${p.id}">${esc(p.title)}</a>`).join(""))
  .replace("<!--TOGGLES-->", styles.map((s) => `<button type="button" id="show-${s.id}" data-style="${s.id}" aria-pressed="true">${s.name}</button>`).join(""))
  .replace("<!--STYLES-->", styles.map((s) => `<div class="style"><dt>${s.name}</dt><dd>${esc(s.note)}</dd></div>`).join("\n      "))
  .replace("<!--VOCAB-->", vocabRows)
  .replace("<!--SECTIONS-->", sections);
fs.writeFileSync(path.join(here, "villanelle-glyph-studies.html"), page);
console.log("chars", used.size, "regular", Math.round(regular.length / 1024) + "KB", "bold", Math.round(bold.length / 1024) + "KB", "page", Math.round(page.length / 1024) + "KB");

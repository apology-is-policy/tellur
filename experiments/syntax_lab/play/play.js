const fs = require("fs"), path = require("path");
const here = __dirname, lab = require("../glyphs/lab.js");
const src = (f) => fs.readFileSync(path.join(here, "..", "syntax", "v2", f), "utf8");
const rails = (f) => lab.text(lab.render(src(f), "rails"));
const EXAMPLES = [
  { title: "Read a file, always close it", text: rails("read_file.vn") },
  { title: "Settings: load, set, get", text: rails("settings.vn") },
  { title: "Binary search", text: rails("ordered_search.vn") },
  { title: "First match, and its caller", text: rails("search.vn") + "\n\n" + rails("first_match.vn") },
  { title: "Server accept loop", text: rails("server.vn") },
  { title: "Contracts for host operations", text: rails("contracts.vn") },
].map((x) => ({ title: x.title, text: x.text + "\n" }));
const fonts = JSON.parse(fs.readFileSync(path.join(here, "fonts.json"), "utf8"));
const codicon = fs.readFileSync(path.join(here, "codicon.ttf")).toString("base64");
const css = fs.readFileSync(path.join(here, "monaco.css"), "utf8").replace(/url\(\.\.\/base\/browser\/ui\/codicons\/codicon\/codicon\.ttf\)/, `url(data:font/ttf;base64,${codicon})`);
if (css.includes("</style")) throw new Error("stylesheet would close the style element");
const js = (v) => JSON.stringify(v).replace(/</g, "\\u003c");
const page = fs.readFileSync(path.join(here, "template.html"), "utf8")
  .replace("/*FONTS*/", () => `@font-face{font-family:"VN Mono";font-weight:400;src:url(data:font/woff2;base64,${fonts.regular}) format("woff2")}\n@font-face{font-family:"VN Mono";font-weight:700;src:url(data:font/woff2;base64,${fonts.bold}) format("woff2")}`)
  .replace("/*MONACO_CSS*/", () => css)
  .replace("/*EXAMPLES*/[]", () => js(EXAMPLES))
  .replace("/*PALETTE*/[]", () => js(fonts.palette));
fs.writeFileSync(path.join(here, "villanelle-rails-playground.html"), page);
console.log("page", Math.round(page.length / 1024) + " KB");
// self-test of the tokenizer on every example line
const script = page.slice(page.lastIndexOf("<script>") + 8, page.lastIndexOf("</script>"));
new Function(script);
const tokSrc = script.slice(script.indexOf("var ROLES"), script.indexOf("window.__vnTokenize"));
const tok = new Function(tokSrc + "; return function (line) { R = defaults(); return tokenizeLine(line); };")();
let n = 0; const seen = {};
for (const ex of EXAMPLES) for (const line of ex.text.split("\n")) {
  const t = tok(line); n++;
  t.forEach((x, i) => { const s = line.slice(x.start, i + 1 < t.length ? t[i + 1].start : line.length); if (x.cls && x.cls !== "com" && x.cls !== "str") (seen[x.cls] = seen[x.cls] || new Set()).add(s); });
  if (t.length && t[0].start !== 0) throw new Error("gap at line start: " + line);
}
console.log("tokenized", n, "lines");
for (const k of ["g", "proof", "own", "arrow", "rail", "sym", "path", "out"]) console.log(k.padEnd(6), [...(seen[k] || [])].slice(0, 60).join(" "));

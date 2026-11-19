const fs = require("fs"), { JSDOM, VirtualConsole } = require("jsdom");
const body = fs.readFileSync("villanelle-rails-playground.html", "utf8");
const html = '<!doctype html><html data-theme="light"><head><meta charset="utf-8"><style>body{margin:0;font:14px system-ui}[hidden]{display:none!important}</style></head><body style="width:1400px">' + body + "</body></html>";
const errors = []; const vc = new VirtualConsole(); vc.on("jsdomError", (e) => errors.push(String(e.message || e).slice(0, 200))); vc.on("error", (e) => errors.push(String(e).slice(0, 200)));
const dom = new JSDOM(html, { runScripts: "dangerously", pretendToBeVisual: true, virtualConsole: vc, url: "https://example.invalid/", beforeParse(w) { w.matchMedia = () => ({ matches: false, addEventListener() {} }); } });
const d = dom.window.document, $ = (id) => d.getElementById(id);
const report = () => ({ options: $("example").options.length, roles: $("roles").querySelectorAll("input").length, glyphs: d.querySelectorAll(".gl").length, lines: $("fb-text").value.split("\n").length, spans: $("fb-pre").querySelectorAll("span").length, status: $("status").textContent });
console.log("after load", JSON.stringify(report()));
// swap the exit glyph through its role field
const exit = $("role-exit"); exit.value = "◆"; exit.dispatchEvent(new dom.window.Event("input", { bubbles: true }));
const v1 = $("fb-text").value;
console.log("exit swap:", (v1.match(/◆/g) || []).length, "new,", (v1.match(/■/g) || []).length, "old; first line with it:", v1.split("\n").find((l) => l.includes("◆")));
// arm a role and assign from the palette
const again = $("role-again"); again.dispatchEvent(new dom.window.Event("focus")); const tri = [...d.querySelectorAll(".gl")].find((b) => b.getAttribute("data-g") === "▲"); tri.click();
console.log("palette assign:", again.value, ($("fb-text").value.match(/▲/g) || []).length, "occurrences");
// insert at cursor in the simple editor
$("fb-text").dispatchEvent(new dom.window.Event("focus")); $("fb-text").selectionStart = $("fb-text").selectionEnd = 0; [...d.querySelectorAll(".gl")].find((b) => b.getAttribute("data-g") === "◈").click();
console.log("insert at cursor:", JSON.stringify($("fb-text").value.slice(0, 12)));
// load another program, then reset roles
$("example").value = "2"; $("example").dispatchEvent(new dom.window.Event("change"));
const v2 = $("fb-text").value; console.log("program 3 keeps roles:", v2.includes("◆ position") && v2.includes("▲ search"), "| status:", $("status").textContent);
$("roles-reset").click(); const v3 = $("fb-text").value;
console.log("roles reset:", v3.includes("■ position") && v3.includes("↑ search") && !v3.includes("◆"), "| errors:", errors.length ? errors : "none");
fs.writeFileSync("snapshot.html", dom.serialize());

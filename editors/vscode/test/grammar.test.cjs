const { test, before } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { Registry, parseRawGrammar, INITIAL } = require('vscode-textmate');
const { loadWASM, OnigScanner, OnigString } = require('vscode-oniguruma');
let grammar;
before(async () => {
  const wasm = fs.readFileSync(require.resolve('vscode-oniguruma/release/onig.wasm'));
  await loadWASM(wasm.buffer.slice(wasm.byteOffset, wasm.byteOffset + wasm.byteLength));
  const registry = new Registry({ onigLib: Promise.resolve({ createOnigScanner: p => new OnigScanner(p), createOnigString: s => new OnigString(s) }), loadGrammar: async () => parseRawGrammar(fs.readFileSync(path.join(__dirname, '../syntaxes/tellur.tmLanguage.json'), 'utf8'), 'tellur.tmLanguage.json') });
  grammar = await registry.loadGrammar('source.tellur');
});
function tokens(text) {
  let stack = INITIAL;
  return text.split('\n').flatMap(line => {
    const result = grammar.tokenizeLine(line, stack); stack = result.ruleStack;
    return result.tokens.map(t => ({ text: line.slice(t.startIndex, t.endIndex), scopes: t.scopes }));
  });
}
function has(list, text, scope) { assert.ok(list.some(t => t.text === text && t.scopes.includes(scope)), `${JSON.stringify(text)} should have ${scope}: ${JSON.stringify(list)}`); }
test('payload inference ellipsis is one operator beside an outcome label', () => {
  const ts=tokens('control lookup() exits ready..., missing { exit ready { "hello" }; }');
  has(ts,'ready','entity.name.type.outcome.tellur');has(ts,'...','keyword.operator.tellur');
  assert.ok(!tokens('"ready..." // ready...').some(t=>t.scopes.includes('keyword.operator.tellur')));
});
test('regions, selectors, decisions and payloads have distinct scopes', () => {
  const ts = tokens('region load(path: String) exits Done {\n [Open & ReadBad]: Δ close(handle) -> [Closed => OK, Failed(e) => Bad];\n [OK | Bad]: exit Done;\n}');
  has(ts, 'load', 'entity.name.function.tellur'); has(ts, 'String', 'storage.type.tellur');
  has(ts, 'ReadBad', 'constant.other.tellur'); has(ts, 'Δ', 'keyword.control.tellur');
  has(ts, 'Closed', 'entity.name.type.outcome.tellur'); has(ts, 'OK', 'constant.other.tellur');
  has(ts, 'e', 'variable.other.tellur'); has(ts, 'close', 'entity.name.function.tellur');
});
test('comments and strings do not become code', () => {
  const ts = tokens('// [Fake]: delta\n[Entry]: emit("delta [Fake] \\"quoted\\" 😀");');
  assert.ok(ts.filter(t => t.text.includes('Fake')).every(t => t.scopes.some(s => s.startsWith('comment.') || s.startsWith('string.'))));
});
test('retry loop, u32 and ASCII delta are highlighted', () => {
  const ts = tokens(fs.readFileSync(path.join(__dirname, '../../../examples/fetch_retry.tellur'), 'utf8'));
  has(ts, 'Attempt', 'entity.name.function.tellur'); has(ts, '1u32', 'constant.numeric.tellur'); has(ts, 'again', 'keyword.control.tellur');
  has(tokens('[Entry]: delta true -> [true => Yes, false => No];'), 'delta', 'keyword.control.tellur');
});
test('decision arms can start on the line after the arrow', () => {
  const ts = tokens('[Entry]: delta true ->\n [true => Yes, false => No];');
  has(ts, 'Yes', 'constant.other.tellur'); has(ts, 'true', 'entity.name.type.outcome.tellur');
});
test('all source fixtures finish outside strings and comments', () => {
  const directory = path.join(__dirname, '../../../examples');
  for (const file of fs.readdirSync(directory).filter(f => f.endsWith('.tellur'))) {
    const ts = tokens(fs.readFileSync(path.join(directory, file), 'utf8'));
    has(ts, '}', 'punctuation.section.tellur');
  }
});
test('record fields and qualified constructors remain readable', () => {
  const ts = tokens('[Entry]: first := pair.left; emit(pair.label); error := LoadError.Both(re, ce);');
  has(ts, 'first', 'variable.other.definition.tellur');
  has(ts, 'pair', 'variable.other.tellur');
  has(ts, 'left', 'variable.other.tellur');
  has(ts, '.', 'punctuation.separator.tellur');
  has(ts, 'LoadError', 'entity.name.type.tellur');
  has(ts, 'Both', 'entity.name.type.tellur');
});
test('resource options and constructor payloads keep their syntax scopes', () => {
  const ts = tokens('region take(file: Option<Handle>) exits Done {\n[Entry]: delta file -> [None => Absent, Some(handle) => Present];\n[Present]: wrapped := Option.Some(handle);\n}');
  has(ts, 'Option', 'storage.type.tellur');
  has(ts, 'Handle', 'entity.name.type.tellur');
  has(ts, 'Some', 'entity.name.type.outcome.tellur');
  has(ts, 'handle', 'variable.other.tellur');
  has(ts, 'Absent', 'constant.other.tellur');
});
test('snake case controls, declarations and brace outcomes have distinct scopes', () => {
  const ts = tokens('control parse_port(string input) exits auto {\n[entry]: delta parse_u32(input) {\n number { u32 number } => parsed,\n malformed => invalid,\n}\n[parsed]: string text = u32_text(number); exit ready { number };\n}');
  has(ts, 'control', 'keyword.control.tellur');
  has(ts, 'parse_port', 'entity.name.function.tellur');
  has(ts, 'string', 'storage.type.tellur');
  has(ts, 'text', 'variable.other.definition.tellur');
  has(ts, 'number', 'entity.name.type.outcome.tellur');
  has(ts, 'number', 'variable.other.definition.tellur');
  has(ts, 'parsed', 'constant.other.tellur');
  has(ts, 'ready', 'entity.name.type.outcome.tellur');
  has(ts, 'u32_text', 'entity.name.function.tellur');
});
test('new playground sources tokenize without swallowing later controls or selectors', () => {
  const directory = path.join(__dirname, '../../../playground');
  for (const file of fs.readdirSync(directory).filter(f => f.endsWith('.tellur'))) {
    const ts = tokens(fs.readFileSync(path.join(directory, file), 'utf8'));
    has(ts, 'control', 'keyword.control.tellur');
    if (ts.some(t => t.text === 'entry')) has(ts, 'entry', 'constant.other.tellur');
    has(ts, 'exit', 'keyword.control.tellur');
  }
});
test('ownership, reference types and dereference operators stay visually distinct', () => {
  const ts = tokens('control duplicate(&string input) exits auto {\n[entry]: mutable string owner = clone(input); &mutable string view = &mutable owner; *view = "after"; emit(view); drop(view); exit done {owner};\n}');
  has(ts, 'mutable', 'storage.modifier.tellur');
  has(ts, '&', 'keyword.operator.tellur');
  has(ts, '*', 'keyword.operator.tellur');
  has(ts, 'owner', 'variable.other.definition.tellur');
  for (const name of ['clone', 'emit', 'drop']) has(ts, name, 'support.function.tellur');
});

test('collection literals remain distinct from path selections', () => {
  const ts = tokens('[entry]: vector<string> words = ["one", "two"]; array<u32, 2> values = [1, 2];\n[ready]: vector<vector<u32>> rows = [\n [1], [2]\n];');
  has(ts, 'entry', 'constant.other.tellur');
  has(ts, 'one', 'string.quoted.double.tellur');
  has(ts, '1', 'constant.numeric.tellur');
  has(ts, 'words', 'variable.other.definition.tellur');
  assert.ok(ts.filter(t => ['one', '1', '2'].includes(t.text)).every(t => !t.scopes.includes('meta.selector.tellur')));
  const names = tokens('[entry]: vector<string> values = [first, second];');
  has(names, 'first', 'variable.other.tellur');
  assert.ok(names.filter(t => t.text === 'first').every(t => t.scopes.includes('meta.collection.literal.tellur')));
});

test('inline deltas, named branches and capture have readable scopes', () => {
  const ts = tokens('control main() exits auto {\n auto saved = capture choose(true);\n exit delta saved {\n ready { string value } => [available]: done {value},\n empty => delta fallback() { ready {auto value} => done {value}, empty => empty {} }\n };\n}');
  has(ts, 'capture', 'keyword.control.tellur');
  const deltas=ts.filter(t => t.text === 'delta');
  assert.equal(deltas.length,2);
  assert.ok(deltas.every(t => t.scopes.includes('keyword.control.tellur')));
  has(ts, 'available', 'constant.other.tellur');
  assert.ok(!ts.some(t => ['done','empty'].includes(t.text) && t.scopes.includes('constant.other.tellur')));
});

test('stored adapters distinguish declarations, decisions and explicit contract types', () => {
  const ts = tokens('auto result = delta choose(true) {ready {string text} => ready {text}, empty => empty {}};\noutcome<choose> forwarded = delta result {ready {auto text}=>ready {text},empty=>empty {}};');
  has(ts, 'result', 'variable.other.definition.tellur');
  has(ts, 'forwarded', 'variable.other.definition.tellur');
  has(ts, 'outcome<choose>', 'storage.type.tellur');
  assert.ok(ts.filter(t => t.text === 'delta').every(t => t.scopes.includes('keyword.control.tellur')));
});

test('named outcome declarations preserve payload types and following controls', () => {
  const ts=tokens('outcome message {\n ready {string text},\n empty\n}\ncontrol main() exits message {message saved=ready {"hello"};exit saved;}');
  has(ts,'outcome','keyword.declaration.tellur');
  has(ts,'message','entity.name.type.tellur');
  has(ts,'ready','entity.name.type.outcome.tellur');
  has(ts,'empty','entity.name.type.outcome.tellur');
  has(ts,'text','variable.other.definition.tellur');
  has(ts,'main','entity.name.function.tellur');
  has(ts,'saved','variable.other.definition.tellur');
  assert.ok(ts.filter(t=>t.text==='main').every(t=>!t.scopes.includes('meta.outcome.contract.tellur')));
});

test('forwarding and single-completion calls have distinct scopes', () => {
  const ts=tokens('display(first); forward display(second); exit delta saved {ready => forward, empty => forward};');
  has(ts,'display','entity.name.function.tellur');
  const forwards=ts.filter(t=>t.text==='forward');
  assert.equal(forwards.length,3);
  assert.ok(forwards.every(t=>t.scopes.includes('keyword.control.tellur')));
  has(tokens('forward forward();'),'forward','entity.name.function.tellur');
});
test('source structs and variants distinguish declaration names, fields and alternatives', () => {
  const ts=tokens('struct user_id {\n u32 value;\n}\nstruct person {user_id id;string name;}\nvariant contact {known {person identity},anonymous}\ncontrol main() exits done {person value=person {user_id {7},"Ada"};exit done;}');
  has(ts,'struct','keyword.declaration.tellur');has(ts,'user_id','entity.name.type.tellur');
  has(ts,'value','variable.other.definition.tellur');has(ts,'id','variable.other.definition.tellur');
  has(ts,'variant','keyword.declaration.tellur');has(ts,'contact','entity.name.type.tellur');
  has(ts,'known','entity.name.type.outcome.tellur');has(ts,'identity','variable.other.definition.tellur');
  has(ts,'main','entity.name.function.tellur');has(ts,'exit','keyword.control.tellur');
});
test('file imports, namespaces and public declarations retain distinct scopes',()=>{
 const ts=tokens('import "people.tellur" as people;\npub struct person {pub u32 age;}\npub control show(people.person value) exits done {exit done;}');
 has(ts,'import','keyword.declaration.tellur');has(ts,'as','keyword.declaration.tellur');has(ts,'people','entity.name.namespace.tellur');
 has(ts,'pub','storage.modifier.tellur');has(ts,'person','entity.name.type.tellur');has(ts,'people.person','storage.type.tellur');has(ts,'value','variable.other.definition.tellur');
});

test('named record fields distinguish labels from initializer expressions', () => {
  const ts=tokens('auto value=people.person {name: clone(&owner),age: 36,id: user_id {value: 7}};');
  for (const name of ['name','age','id','value']) has(ts,name,'variable.other.property.tellur');
  has(ts,'clone','support.function.tellur');
  has(ts,'owner','variable.other.tellur');
  has(ts,'36','constant.numeric.tellur');
});

test('generic data declarations highlight parameters and preserve following controls', () => {
  const ts=tokens('struct box<t> { t value; }\nvariant choice<item> { some {item value},none }\noutcome result<item,error> { ready {item value},failed {error cause} }\ncontrol main() exits done {auto value=box<string> {value:"Ada"};exit done;}');
  for (const name of ['box','choice','result']) has(ts,name,'entity.name.type.tellur');
  for (const name of ['t','item','error']) has(ts,name,'entity.name.type.parameter.tellur');
  has(ts,'main','entity.name.function.tellur');has(ts,'string','storage.type.tellur');
});

test('generic control parameters and calls are readable beside ordinary comparisons', () => {
 const ts=tokens('control identity<item>(item value) exits auto {exit done {value};}\ncontrol main() exits auto {bool smaller=1<2;forward identity<box<string>>(box<string>{value:"Ada"});}');
 has(ts,'item','entity.name.type.parameter.tellur');has(ts,'identity','entity.name.function.tellur');has(ts,'smaller','variable.other.definition.tellur');has(ts,'main','entity.name.function.tellur');
});

test('sealed capability bounds have distinct scopes beside type parameters',()=>{
 const ts=tokens('control choose<item: copy + clone + drop,other: drop>(item value,other unused) exits auto {exit ready {value,value};}');
 for(const name of ['copy','clone','drop']) has(ts,name,'support.type.capability.tellur');
 for(const name of ['item','other']) has(ts,name,'entity.name.type.parameter.tellur');
 has(ts,'choose','entity.name.function.tellur');
});

test('traits, implementations and associated types retain distinct declarations',()=>{
 const ts=tokens('trait reader {type item; control read(&self input) exits ready {item value};}\nimpl reader for text_source {type item=string; control read(&self input) exits ready {item value} {exit ready {"Ada"};}}\ncontrol main() exits done {reader<text_source>.item text="Ada";forward reader<text_source>.read(&text);}');
 has(ts,'reader','entity.name.type.trait.tellur');has(ts,'item','entity.name.type.associated.tellur');has(ts,'self','storage.type.tellur');
 has(ts,'impl','keyword.declaration.tellur');has(ts,'for','keyword.declaration.tellur');has(ts,'read','entity.name.function.tellur');has(ts,'text','variable.other.definition.tellur');
});

test('generic trait requirements and associated capabilities are highlighted',()=>{
 const ts=tokens('trait reader {type item: clone;}\ncontrol read_one<r: reader + clone>(&r input) exits auto {forward reader<r>.read(input);}');
 has(ts,'reader','entity.name.type.trait.tellur');has(ts,'clone','support.type.capability.tellur');has(ts,'r','entity.name.type.parameter.tellur');has(ts,'item','entity.name.type.associated.tellur');
});

test('borrow origins remain visible in trait and generic result contracts',()=>{
 const ts=tokens('trait reader {type item; control peek(&self input) exits found {&item value from input};}\ncontrol peek<r:reader>(&r input) exits found {&reader<r>.item value from input} {forward reader<r>.peek(input);}');
 has(ts,'from','keyword.control.tellur');has(ts,'peek','entity.name.function.tellur');
});

test('reference words and full implement spelling have distinct roles',()=>{
 const ts=tokens('trait reader {type t_item; control peek(ref self input) exits found {ref t_item value from input};}\nimplement reader for message {type t_item=string; control peek(ref self input) exits auto {exit found {borrow input.text};}}\ncontrol update(ref mutable string input) exits done {deref input="changed";exit done;}\ncontrol main() exits done {mutable string owner="before";ref mutable string editor=borrow mutable owner;update(editor);exit done;}');
 has(ts,'implement','keyword.declaration.tellur');has(ts,'ref','storage.type.tellur');has(ts,'borrow','keyword.operator.tellur');has(ts,'deref','keyword.operator.tellur');has(ts,'mutable','storage.modifier.tellur');has(ts,'editor','variable.other.definition.tellur');
});
test('origin groups, arrows and result bindings remain distinct from path selectors', () => {
  const ts=tokens("struct pair['a, 'b] {\n ref string left <- 'a;\n ref string right <- 'b;\n}\ncontrol pair(ref string a,ref string b) exits ready {pair[a,b] value} {exit ready {pair {a,b}};}\n[ready]: exit done;");
  has(ts,"'a",'entity.name.type.origin.tellur');has(ts,"'b",'entity.name.type.origin.tellur');has(ts,'<-','keyword.operator.tellur');has(ts,'ready','constant.other.tellur');
  assert.ok(!tokens('"\'a <-" // \'a <-').some(t=>t.scopes.includes('entity.name.type.origin.tellur')||t.scopes.includes('keyword.operator.tellur')));
});
test('contained origin selectors keep group spelling in nested result maps', () => {
  const ts=tokens("control relay(pair input) exits ready {pair[input['a],input['b]] value} {exit ready {input};}\ncontrol first(pair input) exits found {ref string value from input['a]} {exit found {input.left};}");
  has(ts,"'a",'entity.name.type.origin.tellur');has(ts,"'b",'entity.name.type.origin.tellur');has(ts,'from','keyword.control.tellur');
});

test('variant schemas highlight groups and payload origin maps without swallowing controls', () => {
  const ts=tokens("variant view<item>['a] {found {ref item text <- 'a},empty}\ncontrol pass(view<string> input) exits ready {view<string>[input['a]] value} {exit ready {input};}");
  has(ts,'view','entity.name.type.tellur');has(ts,'item','entity.name.type.parameter.tellur');
  has(ts,"'a",'entity.name.type.origin.tellur');has(ts,'<-','keyword.operator.tellur');
  has(ts,'text','variable.other.definition.tellur');has(ts,'pass','entity.name.function.tellur');
});

test('named outcome origin groups and optional result maps remain distinct from selectors', () => {
  const ts = tokens("outcome reply['a] {found {ref string text <- 'a},empty}\ncontrol produce(ref string input) exits reply[input] {exit found {input};}\ncontrol relay(reply input) exits ready {option<reply>[input['a]] value} {exit ready {some {input}};}");
  has(ts, 'reply', 'entity.name.type.tellur');
  has(ts, "'a", 'entity.name.type.origin.tellur');
  has(ts, '<-', 'keyword.operator.tellur');
  has(ts, 'produce', 'entity.name.function.tellur');
  has(ts, 'relay', 'entity.name.function.tellur');
});

test('process statuses are annotations on alternatives, outside payloads', () => {
  const ts = tokens('outcome command_result {\n @exit 0 done,\n @exit 2 failed { string message }\n}');
  has(ts, '@exit', 'storage.modifier.annotation.tellur');
  has(ts, '0', 'constant.numeric.tellur'); has(ts, '2', 'constant.numeric.tellur');
  has(ts, 'done', 'entity.name.type.outcome.tellur');
  assert.ok(!tokens('// @exit 0\n"@exit 1"').some(t => t.scopes.includes('storage.modifier.annotation.tellur')));
});

test('optional path groups keep nested labels and following statements distinct', () => {
  const ts = tokens(fs.readFileSync(path.join(__dirname, '../../../playground/path_groups.tellur'), 'utf8'));
  for (const name of ['present', 'absent', 'full', 'brief']) has(ts, name, 'constant.other.tellur');
  has(ts, 'heading', 'variable.other.definition.tellur');
  has(ts, 'describe', 'entity.name.function.tellur');
  has(ts, 'exit', 'keyword.control.tellur');
  has(ts, '}', 'punctuation.section.tellur');
});

test('direct arm exits distinguish keywords, final outcomes and continuing paths', () => {
  const ts = tokens(fs.readFileSync(path.join(__dirname, '../../../playground/arm_exits.tellur'), 'utf8'));
  has(ts, 'exit', 'keyword.control.tellur');
  has(ts, 'invalid', 'entity.name.type.outcome.tellur');
  has(ts, 'parsed', 'constant.other.tellur');
  has(ts, 'value', 'variable.other.definition.tellur');
  assert.ok(!ts.some(t => t.text === 'exit' && t.scopes.includes('constant.other.tellur')));
  const legacy = tokens('[entry]: delta flag -> [true=>exit Done,false=>exit Done];');
  has(legacy, 'exit', 'keyword.control.tellur');
  has(legacy, 'Done', 'entity.name.type.outcome.tellur');
});

test('fault permissions and terminations are control keywords, not paths or types', () => {
  const ts = tokens('control fail(string reason) faults { fault reason; }\ncontrol check(bool ok) exits done faults {delta ok {true=>exit done,false=>fault "broken"}}');
  has(ts, 'faults', 'keyword.control.tellur');
  has(ts, 'fault', 'keyword.control.tellur');
  assert.ok(ts.filter(t => t.text === 'fault').every(t => t.scopes.includes('keyword.control.tellur')));
});

test('boundaries and contingency permissions have distinct declarations and keywords', () => {
  const ts = tokens('@contingent\npub control report(string reason) exits done {emit(reason);exit done;}\nboundary request(string context) retains context {fault "broken";}\ncontingency(string reason) {report(reason);terminate;}\ncontrol main() exits done {supervise request("context");exit done;}');
  has(ts, '@contingent', 'storage.modifier.annotation.tellur');
  for (const word of ['boundary', 'retains', 'contingency', 'terminate', 'supervise']) has(ts, word, 'keyword.control.tellur');
});

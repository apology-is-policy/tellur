const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');
async function waitFor(predicate, description, timeout = 15000) {
  const until = Date.now() + timeout;
  while (Date.now() < until) { if (await predicate()) return; await new Promise(r => setTimeout(r, 50)); }
  throw new Error(`Timed out: ${description}`);
}
async function replace(document, text) {
  const edit = new vscode.WorkspaceEdit();
  edit.replace(document.uri, new vscode.Range(document.positionAt(0), document.positionAt(document.getText().length)), text);
  assert.ok(await vscode.workspace.applyEdit(edit));
}
// The editor fixtures that exercise anonymous inference supply explicit launcher policy.
async function buildWithStatus(command, options, exitCodes) {
  const root = vscode.workspace.workspaceFolders[0].uri.fsPath;
  const file = path.join(root, 'manifest.json');
  const before = await fs.readFile(file, 'utf8');
  const config = JSON.parse(before);
  config.launch = { main: { exit_codes: exitCodes } };
  await fs.writeFile(file, JSON.stringify(config));
  try { return await vscode.commands.executeCommand(command, options); }
  finally { await fs.writeFile(file, before); }
}
exports.run = async function () {
  const root = vscode.workspace.workspaceFolders[0].uri.fsPath;
  const uri = vscode.Uri.file(path.join(root, 'load.path'));
  const document = await vscode.workspace.openTextDocument(uri);
  await vscode.window.showTextDocument(document);
  assert.equal(document.languageId, 'tellur');
  await vscode.extensions.getExtension('tellur-language-local.tellur-language').activate();
  const original = document.getText();
  const diagnostics = () => vscode.languages.getDiagnostics(uri).filter(d => d.source === 'tellur');
  const code = d => typeof d.code === 'object' ? d.code.value : d.code;
  try {
    const autoSource = 'control main() exits auto {emit("😀");mutable string first="before";ref mutable auto first_b=borrow mutable first;deref first_b="after";auto count=1;exit done;}';
    await replace(document, autoSource);
    const expectAutoHover = async (marker, expected) => {
      const column = document.getText().indexOf(marker);
      await waitFor(async () => {
        const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, new vscode.Position(0, column + 1));
        return diagnostics().length === 0 && hovers?.some(h =>
          h.range?.start.character === column && h.range?.end.character === column + 4 &&
          h.contents.some(c => c.value.includes(expected)));
      }, `auto hover: ${expected}`);
    };
    await expectAutoHover('auto first_b', 'ref mutable string first_b');
    await expectAutoHover('auto count', 'u32 count');
    await expectAutoHover('auto {', 'exits done');
    await replace(document, autoSource.replace('count=1', 'count=true'));
    await expectAutoHover('auto count', 'bool count');
    await replace(document, autoSource.replace('count=1', 'count=unknown'));
    await waitFor(() => diagnostics().some(d => code(d) === 'E_NAME'), 'invalid auto initializer');
    const invalidHovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, new vscode.Position(0, document.getText().indexOf('auto count') + 1));
    assert.equal(invalidHovers?.length ?? 0, 0, 'invalid source must not show a stale inferred type');
    await replace(document, autoSource);
    await expectAutoHover('auto count', 'u32 count');
    console.log('TELLUR_AUTO_HOVER_EDITOR_TESTS_PASSED: types, mutable reference, exits auto, UTF-16 range, unsaved update, invalidation and repair');
    await replace(document, original.replace(', Failed(re) => ReadBad', ''));
    await waitFor(() => diagnostics().some(d => code(d) === 'E_ARMS'), 'missing outcome diagnostic from unsaved edit');
    await replace(document, original);
    await waitFor(() => diagnostics().length === 0, 'diagnostic clears after unsaved repair');
    const symbols = await vscode.commands.executeCommand('vscode.executeDocumentSymbolProvider', uri);
    assert.ok(symbols.some(s => s.name === 'load'));
    await replace(document, original.replace('[ReadBad & CloseBad]: exit Failed(Both(re, ce));', ''));
    await waitFor(() => diagnostics().some(d => code(d) === 'E_UNCOVERED' && /ReadBad.*CloseBad/.test(d.message)), 'uncovered family witness');
    const unicode = 'region test() exits Done { [Entry]: emit("😀"); # }';
    await replace(document, unicode);
    await waitFor(() => diagnostics().some(d => code(d) === 'E_LEX'), 'Unicode source diagnostic');
    assert.equal(diagnostics()[0].range.start.character, unicode.indexOf('#'));
    await replace(document, original);
    await waitFor(() => diagnostics().length === 0, 'repair after Unicode error');
    const manifestPath = path.join(root, 'manifest.json');
    const manifest = await fs.readFile(manifestPath, 'utf8');
    try {
      const edited = JSON.parse(manifest);
      edited.primitives.find(p => p.name === 'close').outcomes.push({ name: 'Interrupted', fields: [] });
      await fs.writeFile(manifestPath, JSON.stringify(edited));
      await waitFor(() => diagnostics().some(d => code(d) === 'E_ARMS' && d.message.includes('Interrupted')), 'manifest watcher rechecks source');
    } finally { await fs.writeFile(manifestPath, manifest); }
    await waitFor(() => diagnostics().length === 0, 'manifest repair clears source diagnostics');
    await vscode.commands.executeCommand('tellur.restartServer');
    await replace(document, original.replace(', Failed(re) => ReadBad', ''));
    await waitFor(() => diagnostics().some(d => code(d) === 'E_ARMS'), 'diagnostics after server restart');
    await replace(document, original);
    await waitFor(() => diagnostics().length === 0, 'repair after server restart');
    const ownership = await fs.readFile(path.join(__dirname, '../../../examples/partial_moves.tellur'), 'utf8');
    await replace(document, ownership);
    await waitFor(async () => {
      const outline = await vscode.commands.executeCommand('vscode.executeDocumentSymbolProvider', uri);
      return outline?.some(s => s.name === 'close_pair') && diagnostics().length === 0;
    }, 'resource record example accepted');
    for (const [expression, expected] of [['pair', 'E_PARTIAL_MOVE'], ['pair.left', 'E_MOVED']]) {
      await replace(document, ownership.replace('first := pair.left;', `first := pair.left; duplicate := ${expression};`));
      await waitFor(() => diagnostics().some(d => code(d) === expected && d.message.includes('pair.left')), `${expected} identifies moved field`);
      await replace(document, ownership);
      await waitFor(() => diagnostics().length === 0, 'field ownership diagnostic clears');
    }
    const optional = await fs.readFile(path.join(__dirname, '../../../examples/optional_handle.tellur'), 'utf8');
    await replace(document, optional);
    await waitFor(async () => {
      const outline = await vscode.commands.executeCommand('vscode.executeDocumentSymbolProvider', uri);
      return outline?.some(s => s.name === 'optional_file') && diagnostics().length === 0;
    }, 'resource option example accepted');
    for (const [insert, expected, name] of [
      ['[Available]: duplicate := file;', 'E_MOVED', 'file'],
      ['[Absent]: duplicate := handle;', 'E_UNAVAILABLE', 'handle'],
    ]) {
      await replace(document, optional.replace('    // Try `[Available]', `    ${insert}\n    // Try \`[Available]`));
      await waitFor(() => diagnostics().some(d => code(d) === expected && d.message.includes(name)), `${expected} from owned variant decision`);
      await replace(document, optional);
      await waitFor(() => diagnostics().length === 0, 'variant diagnostic clears');
    }
    const buildUri = vscode.Uri.file(path.join(root, 'editor build.tellur'));
    const revised = 'control main(string message) exits auto { [entry]: delta (message == "") { true => empty, false => text } [empty]: exit empty; [text]: string output = message; emit(output); exit done; }';
    await fs.writeFile(buildUri.fsPath, revised);
    const buildDocument = await vscode.workspace.openTextDocument(buildUri);
    assert.equal(buildDocument.languageId, 'tellur');
    await vscode.window.showTextDocument(buildDocument);
    const revisedDiagnostics = () => vscode.languages.getDiagnostics(buildUri).filter(d => d.source === 'tellur');
    await waitFor(async () => {
      const hover = await vscode.commands.executeCommand('vscode.executeHoverProvider', buildUri, new vscode.Position(0, 9));
      return hover?.some(h => h.contents.some(c => /Inferred/.test(c.value))) && revisedDiagnostics().length === 0;
    }, 'inferred control contract hover');
    await replace(buildDocument, revised.replace('string output', 'u32 output'));
    await waitFor(() => revisedDiagnostics().some(d => code(d) === 'E_TYPE'), 'typed declaration diagnostic');
    const ranged = 'control main() exits done {\r\n emit("😀"); u32 value = "first 😀\r\nsecond";\r\n exit done;\r\n}';
    await replace(buildDocument, ranged);
    await waitFor(() => revisedDiagnostics().some(d => code(d) === 'E_TYPE' && d.range.end.line === 2), 'multiline expression range');
    const typeError = revisedDiagnostics().find(d => code(d) === 'E_TYPE');
    assert.equal(buildDocument.getText(typeError.range).replace(/\r\n/g, '\n'), '"first 😀\nsecond"');
    assert.equal(typeError.range.start.character, ranged.split('\n')[1].indexOf('"first'));
    console.log('TELLUR_EXPRESSION_RANGES_EDITOR_TESTS_PASSED: multiline UTF-16 diagnostics and unsaved repair');
    await replace(buildDocument, revised);
    await waitFor(() => revisedDiagnostics().length === 0, 'typed declaration repair');
    const owned = 'control main() exits auto { [entry]: mutable string text="before"; auto ref=&mutable text; *ref="after"; emit(ref); drop(ref); exit done {text}; }';
    await replace(buildDocument, owned.replace('drop(ref);', 'auto moved=text; emit(ref);'));
    await waitFor(() => revisedDiagnostics().some(d => code(d) === 'E_LOAN'), 'live reference prevents owner move');
    await replace(buildDocument, owned.replace('*ref="after";', 'auto moved=ref; *ref="after";'));
    await waitFor(() => revisedDiagnostics().some(d => code(d) === 'E_MOVED'), 'exclusive reference move diagnostic');
    await replace(buildDocument, owned);
    await waitFor(() => revisedDiagnostics().length === 0, 'reference diagnostic repair');
    const ownedRun = await buildWithStatus('tellur.buildAndRun', { output: path.join(root, 'native output', 'ownership') }, {"done": 0});
    assert.equal(ownedRun.code, 0, ownedRun.stderr);
    assert.ok(ownedRun.stdout.includes('after'));
    const referenceSource = 'control main() exits auto { [entry]: mutable string owner="before"; delta rewrite(&mutable owner) {view {auto ref}=>ready} [ready]: emit(ref); owner="after last use"; exit done {owner}; }\ncontrol rewrite(&mutable string text) exits auto { [entry]: *text="from helper"; exit view {text}; }';
    await replace(buildDocument, referenceSource);
    await waitFor(async () => {
      const hover = await vscode.commands.executeCommand('vscode.executeHoverProvider', buildUri, new vscode.Position(1, 10));
      return hover?.some(h => h.contents.some(c => c.value.includes('payload 1 borrows `text`'))) && revisedDiagnostics().length === 0;
    }, 'checked return lifetime hover');
    await replace(buildDocument, referenceSource.replace('owner="after last use";', 'owner="after last use"; emit(ref);'));
    await waitFor(() => revisedDiagnostics().some(d => code(d) === 'E_LOAN'), 'future reference use preserves the loan');
    await replace(buildDocument, 'control main() exits auto { [entry]: string local="local"; exit view {&local}; }');
    await waitFor(() => revisedDiagnostics().some(d => code(d) === 'E_LIFETIME'), 'local reference escape rejected');
    await replace(buildDocument, referenceSource);
    await waitFor(() => revisedDiagnostics().length === 0, 'return lifetime repair');
    const referenceRun = await buildWithStatus('tellur.buildAndRun', { output: path.join(root, 'native output', 'references') }, {"done": 0});
    assert.equal(referenceRun.code, 0, referenceRun.stderr);
    assert.ok(referenceRun.stdout.includes('from helper'));
    const collectionSource = 'control main() exits auto { [entry]: mutable vector<string> words=["before"]; delta vector_get(&words,0) {found {auto word}=>read,out_of_bounds=>empty} [read]: emit(word); delta vector_push(&mutable words,"after") {pushed=>added} [added]: exit done {words}; [empty]: exit empty; }';
    await replace(buildDocument, collectionSource.replace('emit(word); delta vector_push', 'delta vector_push').replace('[added]:', '[added]: emit(word);'));
    await waitFor(() => revisedDiagnostics().some(d => code(d) === 'E_LOAN'), 'collection element loan prevents insertion');
    await replace(buildDocument, collectionSource);
    await waitFor(() => revisedDiagnostics().length === 0, 'collection loan repair');
    const collectionRun = await buildWithStatus('tellur.buildAndRun', { output: path.join(root, 'native output', 'collections') }, {"done": 0, "empty": 1});
    assert.equal(collectionRun.code, 0, collectionRun.stderr);
    assert.ok(collectionRun.stdout.includes('before'));
    const adapterSource = 'control main() exits auto {auto saved=capture choose(true);emit("between");exit delta saved {ready {string value}=>done {value},empty=>empty {}};}\ncontrol choose(bool flag) exits auto {emit("producer");exit delta flag {true=>[available]:ready {"result"},false=>empty {}};}';
    await replace(buildDocument, adapterSource);
    await waitFor(async () => {
      const hover = await vscode.commands.executeCommand('vscode.executeHoverProvider', buildUri, new vscode.Position(0,9));
      return revisedDiagnostics().length === 0 && hover?.some(h => h.contents.some(c => c.value.includes('done { string')));
    }, 'inline adapter inferred contract hover');
    await replace(buildDocument, adapterSource.replace('exit delta saved {ready {string value}=>done {value},empty=>empty {}};', 'exit done {};'));
    await waitFor(() => revisedDiagnostics().some(d => code(d) === 'E_OUTCOME'), 'unhandled captured outcome diagnostic');
    await replace(buildDocument, adapterSource);
    await waitFor(() => revisedDiagnostics().length === 0, 'captured outcome repair');
    const adapterRun = await buildWithStatus('tellur.buildAndRun', {output:path.join(root,'native output','adapters')}, {"done": 0, "empty": 1});
    assert.equal(adapterRun.code,0,adapterRun.stderr);
    assert.ok(adapterRun.stdout.indexOf('producer') < adapterRun.stdout.indexOf('between'));
    assert.ok(adapterRun.stdout.includes('result'));
    console.log('TELLUR_ADAPTERS_EDITOR_TESTS_PASSED: implicit entry, nested syntax, contract hover, captured outcome obligation, repair, native effect order');
    const storedSource = 'control main() exits auto {auto saved=delta choose(true) {ready {string value}=>ready {value},empty=>empty {}};emit("between");exit saved;}\ncontrol choose(bool flag) exits auto {emit("producer");exit delta flag {true=>ready {"stored result"},false=>empty {}};}';
    await replace(buildDocument, storedSource);
    await waitFor(async () => {
      const hover = await vscode.commands.executeCommand('vscode.executeHoverProvider', buildUri, new vscode.Position(0,9));
      return revisedDiagnostics().length === 0 && hover?.some(h => h.contents.some(c => c.value.includes('ready { string')));
    }, 'stored adapter inferred contract hover');
    await replace(buildDocument, storedSource.replace('exit saved;', 'exit discarded;'));
    await waitFor(() => revisedDiagnostics().some(d => code(d) === 'E_OUTCOME'), 'unhandled stored adapter');
    await replace(buildDocument, storedSource.replace('ready {value}', 'ready {value,value}'));
    await waitFor(() => revisedDiagnostics().some(d => code(d) === 'E_MOVED'), 'duplicate move in stored adapter');
    await replace(buildDocument, storedSource);
    await waitFor(() => revisedDiagnostics().length === 0, 'stored adapter repair');
    const storedRun = await buildWithStatus('tellur.buildAndRun', {output:path.join(root,'native output','stored-adapters')}, {"ready": 0, "empty": 1});
    assert.equal(storedRun.code,0,storedRun.stderr);
    assert.ok(storedRun.stdout.indexOf('producer') < storedRun.stdout.indexOf('between'));
    assert.ok(storedRun.stdout.includes('stored result'));
    console.log('TELLUR_STORED_ADAPTERS_EDITOR_TESTS_PASSED: inferred contract, obligation, moves, repair, native effect order');
    const namedSource = 'outcome message {ready {string text},empty}\ncontrol main() exits message {auto saved=capture choose();exit saved;}\ncontrol choose() exits message {message result=ready {"named result"};exit result;}';
    await replace(buildDocument, namedSource);
    await waitFor(async () => {
      const hover=await vscode.commands.executeCommand('vscode.executeHoverProvider',buildUri,new vscode.Position(0,9));
      const outline=await vscode.commands.executeCommand('vscode.executeDocumentSymbolProvider',buildUri);
      return revisedDiagnostics().length===0 && outline?.some(s=>s.name==='message') && hover?.some(h=>h.contents.some(c=>c.value.includes('outcome message')));
    }, 'named outcome declaration hover and outline');
    await replace(buildDocument,namedSource.replace('exit saved;','exit empty;'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_OUTCOME'),'named outcome handling obligation');
    await replace(buildDocument,namedSource.replace('string text','u32 text'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_TYPE'),'named contract payload mismatch');
    await replace(buildDocument,namedSource);
    await waitFor(()=>revisedDiagnostics().length===0,'named outcome repair');
    const namedRun=await buildWithStatus('tellur.buildAndRun',{output:path.join(root,'native output','named-outcomes')}, {"ready": 0, "empty": 1});
    assert.equal(namedRun.code,0,namedRun.stderr);
    assert.ok(namedRun.stdout.includes('named result'));
    console.log('TELLUR_NAMED_OUTCOMES_EDITOR_TESTS_PASSED: declaration hover, outline, obligation, payload typing, repair, native execution');
    const dataSource='struct user_id {u32 value;}\nstruct person {user_id id;string name;}\nvariant contact {known {person identity},anonymous}\ncontrol main() exits done {person value=person {user_id {7},"Ada"};contact saved=contact.known {value};delta saved {known {person identity}=>available,anonymous=>missing}[available]:emit(identity.name);[entry]:exit done;}';
    await replace(buildDocument,dataSource);
    await waitFor(async()=>{
      const hover=await vscode.commands.executeCommand('vscode.executeHoverProvider',buildUri,new vscode.Position(0,9));
      const outline=await vscode.commands.executeCommand('vscode.executeDocumentSymbolProvider',buildUri);
      return revisedDiagnostics().length===0 && outline?.some(s=>s.name==='person') && outline?.some(s=>s.name==='contact') && hover?.some(h=>h.contents.some(c=>c.value.includes('Copy: true')));
    },'source struct/variant hover and outline');
    await replace(buildDocument,dataSource.replace('user_id id;','missing id;'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_MANIFEST' && d.range.start.line===1),'source type diagnostic location');
    await replace(buildDocument,dataSource.replace('contact.known {value}', 'contact.known {value.id}'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_TYPE'),'nominal payload mismatch');
    await replace(buildDocument,dataSource);
    await waitFor(()=>revisedDiagnostics().length===0,'source data repair');
    const dataRun=await buildWithStatus('tellur.buildAndRun',{output:path.join(root,'native output','data-types')}, {"done": 0});
    assert.equal(dataRun.code,0,dataRun.stderr);assert.ok(dataRun.stdout.includes('Ada'));
    console.log('TELLUR_DATA_EDITOR_TESTS_PASSED: structs, variants, wrapper hover, outline, unsaved nominal/layout errors, repair, native execution');
    const namedFieldsSource='struct pair {string first;string second;} control main() exits done {string owner="Named Ada";auto value=pair {second:clone(&owner),first:owner};emit(value.first);emit(value.second);exit done;}';
    await replace(buildDocument,namedFieldsSource);
    await waitFor(()=>revisedDiagnostics().length===0,'named field initialization accepted');
    await replace(buildDocument,namedFieldsSource.replace('second:clone(&owner),first:owner','first:owner'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_FIELD' && d.message.includes('second')),'missing named field');
    await replace(buildDocument,namedFieldsSource.replace('second:clone(&owner),first:owner','first:owner,second:clone(&owner)'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_MOVED'),'named initializer move order');
    await replace(buildDocument,namedFieldsSource);
    await waitFor(()=>revisedDiagnostics().length===0,'named field repair');
    const namedFieldsRun=await buildWithStatus('tellur.buildAndRun',{output:path.join(root,'native output','named-fields')}, {"done": 0});
    assert.equal(namedFieldsRun.code,0,namedFieldsRun.stderr);assert.equal(namedFieldsRun.stdout.trim(),'Named Ada\nNamed Ada\ndone');
    console.log('TELLUR_NAMED_FIELDS_EDITOR_TESTS_PASSED: field errors, move order, repair, native execution');


    const genericSource='struct box<t>{t value;}\noutcome answer<t>{ready {t value},empty}\ncontrol make() exits answer<box<string>> {exit ready {box<string>{value:"Generic Ada"}};}\ncontrol main() exits done {delta make() {ready {auto value}=>available,empty=>missing}[available]:emit(value.value);[entry]:exit done;}';
    await replace(buildDocument,genericSource);
    await waitFor(async()=>{
      const hover=await vscode.commands.executeCommand('vscode.executeHoverProvider',buildUri,new vscode.Position(0,8));
      return revisedDiagnostics().length===0 && hover?.some(h=>h.contents.some(c=>c.value.includes('box<t>') && c.value.includes('Generic nominal')));
    },'generic template hover and concrete construction');
    await replace(buildDocument,genericSource.replace('value:"Generic Ada"','value:7'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_TYPE'),'generic field type mismatch');
    await replace(buildDocument,genericSource.replace('answer<box<string>>','answer<box<string>,u32>'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_GENERIC'),'generic argument arity');
    await replace(buildDocument,genericSource);
    await waitFor(()=>revisedDiagnostics().length===0,'generic repair');
    const genericRun=await buildWithStatus('tellur.buildAndRun',{output:path.join(root,'native output','generic-data')}, {"done": 0});
    assert.equal(genericRun.code,0,genericRun.stderr);assert.ok(genericRun.stdout.includes('Generic Ada'));
    console.log('TELLUR_GENERIC_DATA_EDITOR_TESTS_PASSED: template hover, type and arity errors, repair, native execution');

    const genericControlSource='control identity<item>(item value) exits auto {exit delivered {value};}\ncontrol main() exits done {delta identity<string>("Generic control Ada") {delivered {auto value}=>received}[received]:emit(value);[entry]:exit done;}';
    await replace(buildDocument,genericControlSource);
    await waitFor(async()=>{
      const hover=await vscode.commands.executeCommand('vscode.executeHoverProvider',buildUri,new vscode.Position(0,10));
      return revisedDiagnostics().length===0 && hover?.some(h=>h.contents.some(c=>c.value.includes('identity<item>') && c.value.includes('delivered { item')));
    },'generic control parametric hover');
    await replace(buildDocument,genericControlSource.replace('exit delivered {value};','exit delivered;'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_RESOURCE' && d.message.includes('generic definition check')),'generic definition ownership error');
    await replace(buildDocument,genericControlSource);
    await waitFor(()=>revisedDiagnostics().length===0,'generic definition repair');
    const genericControlRun=await buildWithStatus('tellur.buildAndRun',{output:path.join(root,'native output','generic-controls')}, {"done": 0});
    assert.equal(genericControlRun.code,0,genericControlRun.stderr);assert.ok(genericControlRun.stdout.includes('Generic control Ada'));
    console.log('TELLUR_GENERIC_CONTROLS_EDITOR_TESTS_PASSED: parametric hover, universal ownership diagnostic, repair, native execution');
    const bounded = await fs.readFile(path.join(__dirname, '../../../playground/capabilities.tellur'), 'utf8');
    await replace(buildDocument, bounded);
    let boundedState;
    await waitFor(async () => {
      const h=await vscode.commands.executeCommand('vscode.executeHoverProvider',buildUri,new vscode.Position(3,10));
      boundedState={hover:h?.map(h=>h.contents.map(c=>c.value)),diagnostics:revisedDiagnostics()};
      return revisedDiagnostics().length===0 && h?.some(h=>h.contents.some(c=>c.value.includes('item: clone')));
    }, 'bounded control hover and checked example').catch(error=>{console.error('BOUNDED_EDITOR_STATE',JSON.stringify(boundedState));throw error;});
    await replace(buildDocument,bounded.replace('item: clone','item: drop'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_CLONE'),'drop-only definition cannot clone');
    await replace(buildDocument,bounded.replace('duplicate<item: clone>','duplicate<item: copy>'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_BOUND'),'concrete string record fails copy requirement');
    await replace(buildDocument,bounded);
    await waitFor(()=>revisedDiagnostics().length===0,'bounded generic repair');
    await buildDocument.save();
    const boundedRun=await vscode.commands.executeCommand('tellur.buildAndRun',{output:path.join(root,'native output','capabilities')});
    assert.equal(boundedRun.code,0,boundedRun.stderr);
    assert.equal(boundedRun.stdout.trim(),'Ada\ndone');
    console.log('TELLUR_CAPABILITIES_EDITOR_TESTS_PASSED: bounds hover, definition permissions, concrete requirements, repair, native execution');

    const traitSource=await fs.readFile(path.join(__dirname,'../../../playground/traits.tellur'),'utf8');
    await replace(buildDocument,traitSource);
    await waitFor(async()=>{
      const h=await vscode.commands.executeCommand('vscode.executeHoverProvider',buildUri,new vscode.Position(7,8));
      const outline=await vscode.commands.executeCommand('vscode.executeDocumentSymbolProvider',buildUri);
      return revisedDiagnostics().length===0 && h?.some(h=>h.contents.some(c=>c.value.includes('read_result<item>'))) && outline?.some(s=>s.name==='reader' && s.children.length===2);
    },'trait hover, associated members and implementation outline');
    await replace(buildDocument,traitSource.replace('control read(ref self input) exits read_result<item> {','control read(self input) exits read_result<item> {'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_TRAIT_SIGNATURE'),'trait implementation ownership mismatch');
    await replace(buildDocument,traitSource.replace('type item = string;','type item = u32;'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_TYPE'),'associated assignment changes checked method result type');
    await replace(buildDocument,traitSource);
    await waitFor(()=>revisedDiagnostics().length===0,'trait repair');
    const traitRun=await vscode.commands.executeCommand('tellur.buildAndRun',{output:path.join(root,'native output','traits')});
    assert.equal(traitRun.code,0,traitRun.stderr);assert.equal(traitRun.stdout.trim(),'Hello through a trait\ndone');
    console.log('TELLUR_TRAITS_EDITOR_TESTS_PASSED: trait hover and outline, ownership conformance, associated types, repair, native execution');

    const traitBoundSource=await fs.readFile(path.join(__dirname,'../../../playground/trait_bounds.tellur'),'utf8');
    await replace(buildDocument,traitBoundSource);
    await waitFor(async()=>{
      const h=await vscode.commands.executeCommand('vscode.executeHoverProvider',buildUri,new vscode.Position(14,10));
      return revisedDiagnostics().length===0 && h?.some(h=>h.contents.some(c=>c.value.includes('r: reader') && c.value.includes('reader<r>.item')));
    },'trait-bound generic signature hover');
    await replace(buildDocument,traitBoundSource.replace('r: reader','r'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_BOUND' && !d.message.includes('_path_poly_')),'missing trait bound at generic definition');
    await replace(buildDocument,traitBoundSource.replace('type item: clone;','type item: copy;'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_BOUND'),'associated permission checked at implementation');
    await replace(buildDocument,traitBoundSource);
    await waitFor(()=>revisedDiagnostics().length===0,'trait-bound repair');
    const traitBoundRun=await vscode.commands.executeCommand('tellur.buildAndRun',{output:path.join(root,'native output','trait-bounds')});
    assert.equal(traitBoundRun.code,0,traitBoundRun.stderr);assert.equal(traitBoundRun.stdout.trim(),'Hello through a generic trait\ndone');
    console.log('TELLUR_TRAIT_BOUNDS_EDITOR_TESTS_PASSED: generic trait hover, missing bound, associated permissions, repair, native execution');

    const borrowedTraitSource=await fs.readFile(path.join(__dirname,'../../../playground/borrowed_traits.tellur'),'utf8');
    await replace(buildDocument,borrowedTraitSource);
    await waitFor(async()=>{
      const h=await vscode.commands.executeCommand('vscode.executeHoverProvider',buildUri,new vscode.Position(16,10));
      return revisedDiagnostics().length===0 && h?.some(h=>h.contents.some(c=>c.value.includes('from input') && c.value.includes('ref reader<r>.t_item')));
    },'borrowed generic trait origin hover');
    await replace(buildDocument,borrowedTraitSource.replaceAll('from input','from missing'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_LIFETIME'),'unknown declared origin');
    await replace(buildDocument,borrowedTraitSource.replace('exit found { borrow input.text };','string local="bad"; exit found { &local };'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_LIFETIME'),'borrowed trait local escape');
    await replace(buildDocument,borrowedTraitSource);
    await waitFor(()=>revisedDiagnostics().length===0,'borrowed trait repair');
    const borrowedTraitRun=await vscode.commands.executeCommand('tellur.buildAndRun',{output:path.join(root,'native output','borrowed-traits')});
    assert.equal(borrowedTraitRun.code,0,borrowedTraitRun.stderr);assert.equal(borrowedTraitRun.stdout.trim(),'Hello through a borrowed trait\ndone');
    console.log('TELLUR_BORROWED_TRAITS_EDITOR_TESTS_PASSED: origin hover, unknown owner, local escape, repair, native execution');

    const wordSource=await fs.readFile(path.join(__dirname,'../../../playground/ownership.tellur'),'utf8');
    await replace(buildDocument,wordSource);
    await waitFor(()=>revisedDiagnostics().length===0,'word-spelled exclusive reference and dereference');
    await replace(buildDocument,wordSource.replace('ref mutable string edit','ref string edit'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_TYPE' && d.message.includes('ref mutable string')),'reference word type mismatch');
    await replace(buildDocument,wordSource.replace('deref edit = "after";','message="conflict"; deref edit = "after";'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_LOAN'),'word-spelled exclusive loan blocks owner access');
    await replace(buildDocument,wordSource);
    await waitFor(()=>revisedDiagnostics().length===0,'reference word repair');
    const wordRun=await vscode.commands.executeCommand('tellur.buildAndRun',{output:path.join(root,'native output','reference-words')});
    assert.equal(wordRun.code,0,wordRun.stderr);assert.ok(wordRun.stdout.includes('after'));
    console.log('TELLUR_REFERENCE_WORDS_EDITOR_TESTS_PASSED: word syntax, exclusive types, loan conflicts, repair, native execution');

    const storedReferenceSource=await fs.readFile(path.join(__dirname,'../../../playground/stored_references.tellur'),'utf8');
    await replace(buildDocument,storedReferenceSource);
    await waitFor(()=>revisedDiagnostics().length===0,'stored reference records');
    await replace(buildDocument,storedReferenceSource.replace('message_view second = first;','original.text="bad";message_view second = first;'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_LOAN'),'stored reference blocks owner mutation');
    await replace(buildDocument,storedReferenceSource.replace('emit(second.label);','emit(first.label);'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_MOVED'),'stored record move diagnostic');
    await replace(buildDocument,storedReferenceSource);
    await waitFor(()=>revisedDiagnostics().length===0,'stored reference repair');
    const storedReferenceRun=await vscode.commands.executeCommand('tellur.buildAndRun',{output:path.join(root,'native output','stored-references')});
    assert.equal(storedReferenceRun.code,0,storedReferenceRun.stderr);assert.ok(storedReferenceRun.stdout.includes('Changed through a moved reference field'));
    console.log('TELLUR_STORED_REFERENCES_EDITOR_TESTS_PASSED: record loans, move diagnostics, repair, native execution');

    const referenceAutoSource='control main() exits done {mutable string first="before";ref mutable auto first_b=borrow mutable first;deref first_b="inferred mutable reference";emit(first);exit done;}';
    await replace(buildDocument,referenceAutoSource);
    await waitFor(()=>revisedDiagnostics().length===0,'reference auto declaration');
    await replace(buildDocument,referenceAutoSource.replace('ref mutable auto','ref auto'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_TYPE'),'reference auto mode mismatch');
    await replace(buildDocument,referenceAutoSource);
    await waitFor(()=>revisedDiagnostics().length===0,'reference auto repair');
    const referenceAutoRun=await buildWithStatus('tellur.buildAndRun',{output:path.join(root,'native output','reference-auto')}, {"done": 0});
    assert.equal(referenceAutoRun.code,0,referenceAutoRun.stderr);assert.ok(referenceAutoRun.stdout.includes('inferred mutable reference'));
    console.log('TELLUR_REFERENCE_AUTO_EDITOR_TESTS_PASSED: inferred referent, mode mismatch, repair, native execution');

    const inferredPayloadSource=await fs.readFile(path.join(__dirname,'../../../playground/inferred_payloads.tellur'),'utf8');
    await replace(buildDocument,inferredPayloadSource);
    await waitFor(async()=>{
      const hover=await vscode.commands.executeCommand('vscode.executeHoverProvider',buildUri,new vscode.Position(0,10));
      return revisedDiagnostics().length===0 && hover?.some(h=>h.contents.some(c=>c.value.includes('ready { string')));
    },'inferred payload hover');
    await replace(buildDocument,inferredPayloadSource.replace('ready { "Hello through an inferred payload" }','ready {}'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_INFER'),'empty inferred payload rejected');
    await replace(buildDocument,inferredPayloadSource);
    await waitFor(()=>revisedDiagnostics().length===0,'inferred payload repair');
    const inferredPayloadRun=await vscode.commands.executeCommand('tellur.buildAndRun',{output:path.join(root,'native output','inferred-payloads')});
    assert.equal(inferredPayloadRun.code,0,inferredPayloadRun.stderr);assert.ok(inferredPayloadRun.stdout.includes('Hello through an inferred payload'));
    console.log('TELLUR_INFERRED_PAYLOADS_EDITOR_TESTS_PASSED: inferred hover, nonempty contract, repair, native execution');

    const originGroupSource=await fs.readFile(path.join(__dirname,'../../../tests/fixtures/origin_groups.tellur'),'utf8');
    await replace(buildDocument,originGroupSource);
    await waitFor(async()=>{
      const hover=await vscode.commands.executeCommand('vscode.executeHoverProvider',buildUri,new vscode.Position(5,10));
      return revisedDiagnostics().length===0 && hover?.some(h=>h.contents.some(c=>c.value.includes('text_pair[a, b]') && c.value.includes("['a]")));
    },'grouped origin hover');
    await replace(buildDocument,originGroupSource.replace('text_pair[a, b] value','text_pair[b, a] value'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_LIFETIME'),'swapped origin contract rejected');
    await replace(buildDocument,originGroupSource.replace('emit(words.left);','first = "too early"; emit(words.left);'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_LOAN'),'still-live field owner protected');
    await replace(buildDocument,originGroupSource);
    await waitFor(()=>revisedDiagnostics().length===0,'origin group repair');
    const originGroupRun=await buildWithStatus('tellur.buildAndRun',{output:path.join(root,'native output','origin-groups')}, {"done": 0});
    assert.equal(originGroupRun.code,0,originGroupRun.stderr);assert.ok(originGroupRun.stdout.includes('The left source is available again'));assert.ok(originGroupRun.stdout.includes('The right source is available again'));
    console.log('TELLUR_ORIGIN_GROUPS_EDITOR_TESTS_PASSED: grouped hover, contract and loan diagnostics, field last use, repair, native execution');

    const recordInputSource=await fs.readFile(path.join(__dirname,'../../../tests/fixtures/record_inputs.tellur'),'utf8');
    await replace(buildDocument,recordInputSource);
    const recordInputLine=recordInputSource.split('\n').findIndex(l=>l.startsWith('control heading_of'));
    await waitFor(async()=>{
      const hover=await vscode.commands.executeCommand('vscode.executeHoverProvider',buildUri,new vscode.Position(recordInputLine,10));
      return revisedDiagnostics().length===0 && hover?.some(h=>h.contents.some(c=>c.value.includes("input['heading]")));
    },'record input contained-origin hover');
    await replace(buildDocument,recordInputSource.replace("from input['heading]","from input['content]"));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_LIFETIME'),'record input incorrect origin');
    await replace(buildDocument,recordInputSource.replace("from input['heading]","from input['caption]"));
    await waitFor(()=>revisedDiagnostics().some(d=>d.message.includes('unknown public origin group')),'record input unknown public group');
    await replace(buildDocument,recordInputSource);
    await waitFor(()=>revisedDiagnostics().length===0,'record input repair');
    const recordInputRun=await buildWithStatus('tellur.buildAndRun',{output:path.join(root,'native output','record-inputs')}, {"done": 0});
    assert.equal(recordInputRun.code,0,recordInputRun.stderr);assert.ok(recordInputRun.stdout.includes('Content survives its carrier'));assert.ok(recordInputRun.stdout.includes('Heading is available again'));
    console.log('TELLUR_RECORD_INPUTS_EDITOR_TESTS_PASSED: contained-origin hover, unsaved errors, repair, native execution');

    const carrierSource=await fs.readFile(path.join(__dirname,'../../../tests/fixtures/carrier_reborrows.tellur'),'utf8');
    await replace(buildDocument,carrierSource);
    const carrierLine=carrierSource.split('\n').findIndex(l=>l.startsWith('control reborrow'));
    await waitFor(async()=>{
      const hover=await vscode.commands.executeCommand('vscode.executeHoverProvider',buildUri,new vscode.Position(carrierLine,10));
      return revisedDiagnostics().length===0 && hover?.some(h=>h.contents.some(c=>c.value.includes('authorizing carrier borrow')));
    },'exclusive carrier authorization hover');
    await replace(buildDocument,carrierSource.replace('ref mutable text_editors input','ref text_editors input'));
    await waitFor(()=>revisedDiagnostics().some(d=>d.message.includes('exclusive field cannot borrow a shared input origin')),'shared carrier cannot grant exclusive fields');
    await replace(buildDocument,carrierSource.replace('emit(children.first);','drop(parents);emit(children.first);'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_LOAN'),'live child retains carrier');
    await replace(buildDocument,carrierSource);
    await waitFor(()=>revisedDiagnostics().length===0,'carrier permission repair');
    const carrierRun=await buildWithStatus('tellur.buildAndRun',{output:path.join(root,'native output','carrier-reborrows')}, {"done": 0});
    assert.equal(carrierRun.code,0,carrierRun.stderr);assert.ok(carrierRun.stdout.includes('First child edit'));assert.ok(carrierRun.stdout.includes('Second child edit'));
    console.log('TELLUR_CARRIER_REBORROWS_EDITOR_TESTS_PASSED: authorization hover, mode and carrier diagnostics, repair, native execution');

    const splitSource=await fs.readFile(path.join(__dirname,'../../../tests/fixtures/opaque_splits.tellur'),'utf8');
    await replace(buildDocument,splitSource);
    const splitLine=splitSource.split('\n').findIndex(l=>l.startsWith('control edit'));
    await waitFor(async()=>{
      const hover=await vscode.commands.executeCommand('vscode.executeHoverProvider',buildUri,new vscode.Position(splitLine,10));
      return revisedDiagnostics().length===0 && hover?.some(h=>h.contents.some(c=>c.value.includes('disjoint access')));
    },'opaque split permissions hover');
    await replace(buildDocument,splitSource.replace('borrow mutable input.body','borrow mutable input.title'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_ALIAS'),'duplicate exclusive result rejected');
    await replace(buildDocument,splitSource.replace('emit(title);','drop(draft);emit(title);'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_LOAN'),'opaque split retains owner');
    await replace(buildDocument,splitSource);
    await waitFor(()=>revisedDiagnostics().length===0,'opaque split repair');
    const splitRun=await buildWithStatus('tellur.buildAndRun',{output:path.join(root,'native output','opaque-splits')}, {"done": 0});
    assert.equal(splitRun.code,0,splitRun.stderr);assert.ok(splitRun.stdout.includes('A new body'));assert.ok(splitRun.stdout.includes('Final title'));
    console.log('TELLUR_OPAQUE_SPLITS_EDITOR_TESTS_PASSED: permissions hover, alias and owner diagnostics, repair, native execution');

    const borrowedSource=await fs.readFile(path.join(__dirname,'../../../tests/fixtures/borrowed_outcomes.tellur'),'utf8');
    await replace(buildDocument,borrowedSource);
    const borrowedLine=borrowedSource.split('\n').findIndex(l=>l.startsWith('outcome first_word'));
    await waitFor(async()=>{
      const hover=await vscode.commands.executeCommand('vscode.executeHoverProvider',buildUri,new vscode.Position(borrowedLine,10));
      return revisedDiagnostics().length===0 && hover?.some(h=>h.contents.some(c=>c.value.includes('ref string')));
    },'borrowed outcome payload hover');
    await replace(buildDocument,borrowedSource.replace('first_word message =','drop(words);first_word message ='));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_LOAN'),'stored borrowed outcome retains its owner');
    await replace(buildDocument,'control peek(ref string input) exits found {ref string text from input} {exit found {input};} control main() exits done {string word="x";auto saved=capture peek(borrow word);exit done;}');
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_OUTCOME'),'borrowed capture still requires handling');
    await replace(buildDocument,borrowedSource);
    await waitFor(()=>revisedDiagnostics().length===0,'borrowed outcome repair');
    const borrowedRun=await buildWithStatus('tellur.buildAndRun',{output:path.join(root,'native output','borrowed-outcomes')}, {"done": 0});
    assert.equal(borrowedRun.code,0,borrowedRun.stderr);assert.ok(borrowedRun.stdout.includes('A borrowed word'));assert.ok(borrowedRun.stdout.includes('"value":2'));
    console.log('TELLUR_BORROWED_OUTCOMES_EDITOR_TESTS_PASSED: payload hover, owner and obligation diagnostics, repair, native execution');

    const outcomeOrigins = await fs.readFile(path.join(__dirname, '../../../tests/fixtures/outcome_origins.tellur'), 'utf8');
    await replace(buildDocument, outcomeOrigins);
    const outcomeLine = outcomeOrigins.split('\n').findIndex(l => l.startsWith('outcome text_result'));
    await waitFor(async () => {
      const hover = await vscode.commands.executeCommand('vscode.executeHoverProvider', buildUri, new vscode.Position(outcomeLine, 10));
      return revisedDiagnostics().length === 0 && hover?.some(h => h.contents.some(c => c.value.includes("text_result['title, 'body]") && c.value.includes("text_views['title, 'body]")));
    }, 'named outcome schema groups on hover');
    const producerLine = outcomeOrigins.split('\n').findIndex(l => l.startsWith('control lookup_text'));
    const producerHover = await vscode.commands.executeCommand('vscode.executeHoverProvider', buildUri, new vscode.Position(producerLine, 10));
    assert.ok(producerHover?.some(h => h.contents.some(c => c.value.includes('exits text_result[title, body]'))));
    await replace(buildDocument, outcomeOrigins.replace('emit(fields.title);', 'title="too early";emit(fields.title);'));
    await waitFor(() => revisedDiagnostics().some(d => code(d) === 'E_LOAN'), 'named outcome/option owner conflict');
    await replace(buildDocument, outcomeOrigins);
    await waitFor(() => revisedDiagnostics().length === 0, 'named outcome/option repair');
    const outcomeRun = await buildWithStatus('tellur.buildAndRun', {output:path.join(root, 'native output', 'outcome-origins')}, {done: 0});
    assert.equal(outcomeRun.code, 0, outcomeRun.stderr);
    assert.ok(outcomeRun.stdout.includes('title released independently') && outcomeRun.stdout.includes('edited body'));
    console.log('TELLUR_OUTCOME_ORIGINS_EDITOR_TESTS_PASSED: schemas, completion bindings, owner conflicts, repair and native execution');
    const sumCarriers = await fs.readFile(path.join(__dirname, '../../../tests/fixtures/sum_carriers.tellur'), 'utf8');
    await replace(buildDocument, sumCarriers);
    await waitFor(() => revisedDiagnostics().length === 0, 'borrowed sum carrier source');
    for (const [binding, expected] of [['auto preview', 'ref string preview'], ['auto editable', 'ref mutable string editable']]) {
      const line = sumCarriers.split('\n').findIndex(l => l.includes(binding));
      const column = sumCarriers.split('\n')[line].indexOf(binding) + 1;
      await waitFor(async () => {
        const hover = await vscode.commands.executeCommand('vscode.executeHoverProvider', buildUri, new vscode.Position(line, column));
        return hover?.some(h => h.contents.some(c => c.value.includes(expected)));
      }, `borrowed payload hover: ${expected}`);
    }
    await replace(buildDocument, sumCarriers.replace('deref editable =', 'owner = "too early"; deref editable ='));
    await waitFor(() => revisedDiagnostics().some(d => code(d) === 'E_LOAN'), 'borrowed sum owner conflict');
    await replace(buildDocument, sumCarriers);
    await waitFor(() => revisedDiagnostics().length === 0, 'borrowed sum repair');
    const sumRun = await buildWithStatus('tellur.buildAndRun', {output:path.join(root, 'native output', 'sum-carriers')}, {done: 0});
    assert.equal(sumRun.code, 0, sumRun.stderr);
    assert.ok(sumRun.stdout.includes('before') && sumRun.stdout.includes('after'));
    console.log('TELLUR_SUM_CARRIERS_EDITOR_TESTS_PASSED: reborrow mode hover, owner conflicts, repair and native execution');
    const carrierResults = await fs.readFile(path.join(__dirname, '../../../tests/fixtures/carrier_results.tellur'), 'utf8');
    await replace(buildDocument, carrierResults);
    const lendLine = carrierResults.split('\n').findIndex(l => l.startsWith('control lend('));
    await waitFor(async () => {
      const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', buildUri, new vscode.Position(lendLine, 9));
      return revisedDiagnostics().length === 0 && hovers?.some(h => h.contents.some(c => c.value.includes('borrows `input`.') && c.value.includes("input['source]")));
    }, 'returned carrier storage and contained origins');
    const returnedLine = carrierResults.split('\n').findIndex(l => l.includes('auto returned'));
    await waitFor(async () => {
      const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', buildUri, new vscode.Position(returnedLine, carrierResults.split('\n')[returnedLine].indexOf('auto returned') + 1));
      return hovers?.some(h => h.contents.some(c => c.value.includes('ref mutable option<ref mutable string> returned')));
    }, 'returned carrier auto hover');
    await replace(buildDocument, carrierResults.replace('deref view.text =', 'drop(saved); deref view.text ='));
    await waitFor(() => revisedDiagnostics().some(d => code(d) === 'E_LOAN'), 'returned carrier storage protection');
    await replace(buildDocument, carrierResults);
    await waitFor(() => revisedDiagnostics().length === 0, 'returned carrier repair');
    const resultRun = await buildWithStatus('tellur.buildAndRun', {output:path.join(root, 'native output', 'carrier-results')}, {done: 0});
    assert.equal(resultRun.code, 0, resultRun.stderr);
    assert.ok(resultRun.stdout.includes('edited through returned view') && resultRun.stdout.includes('edited through returned option'));
    console.log('TELLUR_CARRIER_RESULTS_EDITOR_TESTS_PASSED: both lifetime layers, auto hover, storage conflict, repair and native execution');
    const storedCarriers = await fs.readFile(path.join(__dirname, '../../../tests/fixtures/stored_carriers.tellur'), 'utf8');
    await replace(buildDocument, storedCarriers);
    await waitFor(() => revisedDiagnostics().length === 0, 'stored carrier source');
    for (const [binding, expected] of [['auto view', 'ref mutable text_view view'], ['auto job', 'ref mutable edit_job job'], ['auto transferred', 'outcome<lend> transferred']]) {
      const line = storedCarriers.split('\n').findIndex(l => l.includes(binding));
      const column = storedCarriers.split('\n')[line].indexOf(binding) + 1;
      await waitFor(async () => {
        const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', buildUri, new vscode.Position(line, column));
        return hovers?.some(h => h.contents.some(c => c.value.includes(expected)));
      }, `stored carrier hover: ${expected}`);
    }
    await replace(buildDocument, storedCarriers.replace('delta transferred', 'drop(saved); delta transferred'));
    await waitFor(() => revisedDiagnostics().some(d => code(d) === 'E_LOAN'), 'captured carrier storage conflict');
    await replace(buildDocument, storedCarriers);
    await waitFor(() => revisedDiagnostics().length === 0, 'stored carrier repair');
    const storedCarrierRun = await buildWithStatus('tellur.buildAndRun', {output:path.join(root, 'native output', 'stored-carriers')}, {done: 0});
    assert.equal(storedCarrierRun.code, 0, storedCarrierRun.stderr);
    assert.ok(storedCarrierRun.stdout.includes('edited after capture') && storedCarrierRun.stdout.includes('edited through stored job'));
    console.log('TELLUR_STORED_CARRIERS_EDITOR_TESTS_PASSED: capture and nested reference hover, retained storage, repair and native execution');
    // The legacy fixture manifest lacks the modern formatting operation used here.
    const publicContractManifest = JSON.parse(manifest);
    const playgroundManifest = JSON.parse(await fs.readFile(path.join(__dirname, '../../../playground/manifest.json'), 'utf8'));
    if (!publicContractManifest.primitives.some(p => p.name === 'u32_text')) {
      publicContractManifest.primitives.push(playgroundManifest.primitives.find(p => p.name === 'u32_text'));
    }
    await fs.writeFile(manifestPath, JSON.stringify(publicContractManifest));
    try {
      const publicContracts = await fs.readFile(path.join(__dirname, '../../../tests/fixtures/public_contracts.tellur'), 'utf8');
      await replace(buildDocument, publicContracts);
      await waitFor(() => revisedDiagnostics().length === 0, 'explicit public contract');
      await replace(buildDocument, publicContracts.replace('exits ready { u32 value }, invalid', 'exits auto'));
      await waitFor(() => revisedDiagnostics().some(d => code(d) === 'E_PUBLIC_CONTRACT'), 'public inference diagnostic');
      await replace(buildDocument, publicContracts.replace('exit invalid;', 'exit unexpected;'));
      await waitFor(() => revisedDiagnostics().some(d => code(d) === 'E_OUTCOME' && d.relatedInformation?.some(n => n.message.includes('produced by')) && d.relatedInformation?.some(n => n.message.includes('forwarded from'))), 'outcome origin and forwarding locations');
      await replace(buildDocument, publicContracts);
      await waitFor(() => revisedDiagnostics().length === 0, 'public contract repair');
      const publicContractRun = await buildWithStatus('tellur.buildAndRun', {output:path.join(root, 'native output', 'public-contracts')}, {done: 0});
      assert.equal(publicContractRun.code, 0, publicContractRun.stderr);
      assert.ok(publicContractRun.stdout.includes('42'));
      console.log('TELLUR_PUBLIC_CONTRACTS_EDITOR_TESTS_PASSED: explicit public contracts, contributor locations, repair and native execution');
    } finally { await fs.writeFile(manifestPath, manifest); }
    const borrowedCollections = await fs.readFile(path.join(__dirname, '../../../tests/fixtures/borrowed_collections.tellur'), 'utf8');
    await replace(buildDocument, borrowedCollections);
    await waitFor(() => revisedDiagnostics().length === 0, 'borrowed collection source');
    const borrowedWordLine = borrowedCollections.split('\n').findIndex(l => l.includes('auto word'));
    await waitFor(async () => {
      const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', buildUri, new vscode.Position(borrowedWordLine, borrowedCollections.split('\n')[borrowedWordLine].indexOf('auto word') + 1));
      return hovers?.some(h => h.contents.some(c => c.value.includes('ref string word')));
    }, 'borrowed collection direct reference hover');
    await replace(buildDocument, borrowedCollections.replace('emit(word)', 'first = "early"; emit(word)'));
    await waitFor(() => revisedDiagnostics().some(d => code(d) === 'E_LOAN'), 'collection source retained after holder drop');
    await replace(buildDocument, borrowedCollections);
    await waitFor(() => revisedDiagnostics().length === 0, 'borrowed collection repair');
    const borrowedCollectionRun = await buildWithStatus('tellur.buildAndRun', {output:path.join(root, 'native output', 'borrowed-collections')}, {done: 0});
    assert.equal(borrowedCollectionRun.code, 0, borrowedCollectionRun.stderr);
    assert.ok(borrowedCollectionRun.stdout.includes('first released') && borrowedCollectionRun.stdout.includes('second released'));
    console.log('TELLUR_BORROWED_COLLECTIONS_EDITOR_TESTS_PASSED: direct reference hover, retained sources, repair and native execution');
    const aggregateTraits = await fs.readFile(path.join(__dirname, '../../../tests/fixtures/aggregate_traits.tellur'), 'utf8');
    await replace(buildDocument, aggregateTraits);
    await waitFor(() => revisedDiagnostics().length === 0, 'aggregate trait contracts');
    const aggregateJobLine = aggregateTraits.split('\n').findIndex(l => l.includes('auto job'));
    await waitFor(async () => {
      const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', buildUri, new vscode.Position(aggregateJobLine, aggregateTraits.split('\n')[aggregateJobLine].indexOf('auto job') + 1));
      return hovers?.some(h => h.contents.some(c => c.value.includes('edit_job<string> job')));
    }, 'aggregate trait inferred payload');
    await replace(buildDocument, aggregateTraits.replace("preparation<item>[owner, owner['text]]", "preparation<item>[owner['text], owner]"));
    await waitFor(() => revisedDiagnostics().some(d => code(d) === 'E_ORIGIN'), 'trait origin conformance');
    await replace(buildDocument, aggregateTraits);
    await waitFor(() => revisedDiagnostics().length === 0, 'trait contract repair');
    const aggregateRun = await buildWithStatus('tellur.buildAndRun', {output:path.join(root, 'native output', 'aggregate-traits')}, {done: 0});
    assert.equal(aggregateRun.code, 0, aggregateRun.stderr);
    assert.ok(aggregateRun.stdout.includes('edited through a trait contract') && aggregateRun.stdout.includes('released after the job'));
    console.log('TELLUR_AGGREGATE_TRAITS_EDITOR_TESTS_PASSED: associated generic contracts, inferred hover, conformance repair and native execution');
    const publicCarriers = await fs.readFile(path.join(__dirname, '../../../tests/fixtures/public_carriers.tellur'), 'utf8');
    await replace(buildDocument, publicCarriers);
    await waitFor(() => revisedDiagnostics().length === 0, 'public carrier source');
    for (const [binding, expected] of [['auto job', 'edit_job job'], ['struct edit_job', "ref mutable text_view['text] target <- 'storage"]]) {
      const line = publicCarriers.split('\n').findIndex(l => l.includes(binding));
      const column = publicCarriers.split('\n')[line].indexOf(binding) + (binding.startsWith('struct') ? 8 : 1);
      await waitFor(async () => {
        const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', buildUri, new vscode.Position(line, column));
        return hovers?.some(h => h.contents.some(c => c.value.includes(expected)));
      }, `public carrier hover: ${expected}`);
    }
    await replace(buildDocument, publicCarriers.replace('deref job.target.text =', 'drop(saved); deref job.target.text ='));
    await waitFor(() => revisedDiagnostics().some(d => code(d) === 'E_LOAN'), 'public carrier storage conflict');
    await replace(buildDocument, publicCarriers);
    await waitFor(() => revisedDiagnostics().length === 0, 'public carrier repair');
    const publicCarrierRun = await buildWithStatus('tellur.buildAndRun', {output:path.join(root, 'native output', 'public-carriers')}, {done: 0});
    assert.equal(publicCarrierRun.code, 0, publicCarrierRun.stderr);
    assert.ok(publicCarrierRun.stdout.includes('edited through public contract'));
    console.log('TELLUR_PUBLIC_CARRIERS_EDITOR_TESTS_PASSED: public schemas, nested reference hover, retained storage, repair and native execution');
    const publicSource=await fs.readFile(path.join(__dirname,'../../../tests/fixtures/public_variants.tellur'),'utf8');
    await replace(buildDocument,publicSource);
    const publicLine=publicSource.split('\n').findIndex(l=>l.startsWith('variant lookup'));
    await waitFor(async()=>{
      const hovers=await vscode.commands.executeCommand('vscode.executeHoverProvider',buildUri,new vscode.Position(publicLine,9));
      return revisedDiagnostics().length===0 && hovers?.some(h=>h.contents.some(c=>c.value.includes("lookup['title, 'body]") && c.value.includes("text_views['title, 'body]")));
    },'public variant schema hover');
    await replace(buildDocument,publicSource.replace('emit(fields.title);','title="too soon";emit(fields.title);'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_LOAN'),'public variant owner retention');
    await replace(buildDocument,publicSource);
    await waitFor(()=>revisedDiagnostics().length===0,'public variant repair');
    const publicRun=await buildWithStatus('tellur.buildAndRun',{output:path.join(root,'native output','public-variants')}, {"done": 0});
    assert.equal(publicRun.code,0,publicRun.stderr);assert.ok(publicRun.stdout.includes('edited body'));assert.ok(publicRun.stdout.includes('title released independently'));
    console.log('TELLUR_PUBLIC_VARIANTS_EDITOR_TESTS_PASSED: grouped schema hover, owner conflict, repair, native execution');
    const nestedSource=await fs.readFile(path.join(__dirname,'../../../tests/fixtures/nested_borrows.tellur'),'utf8');
    await replace(buildDocument,nestedSource);
    const nestedLine=nestedSource.split('\n').findIndex(l=>l.includes('auto values'));
    const nestedColumn=nestedSource.split('\n')[nestedLine].indexOf('auto');
    await waitFor(async()=>{
      const hover=await vscode.commands.executeCommand('vscode.executeHoverProvider',buildUri,new vscode.Position(nestedLine,nestedColumn+1));
      return revisedDiagnostics().length===0 && hover?.some(h=>h.contents.some(c=>c.value.includes('editors values')));
    },'nested borrowed payload inferred hover');
    await replace(buildDocument,nestedSource.replace('delta pending','title="too soon";delta pending'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_LOAN'),'nested outcome retains original owner');
    await replace(buildDocument,nestedSource.replace('title = "published title"','body = "too soon"'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_LOAN'),'nested fields retain independent last uses');
    await replace(buildDocument,nestedSource.replace('delta pending','drop(pending);delta pending'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_RESOURCE'),'nested outcome still requires handling');
    await replace(buildDocument,nestedSource);
    await waitFor(()=>revisedDiagnostics().length===0,'nested borrowed storage repair');
    const nestedRun=await buildWithStatus('tellur.buildAndRun',{output:path.join(root,'native output','nested-borrows')}, {"done": 0});
    assert.equal(nestedRun.code,0,nestedRun.stderr);assert.ok(nestedRun.stdout.includes('published title'));assert.ok(nestedRun.stdout.includes('revised body'));
    console.log('TELLUR_NESTED_BORROWS_EDITOR_TESTS_PASSED: inferred hover, owner and field lifetime diagnostics, mandatory handling, repair, native execution');

    const moduleFile=path.join(root,'people.path');
    const moduleUri=vscode.Uri.file(moduleFile);
    const moduleSource='pub struct person {string name;pub u32 age;}\npub control create(string name,u32 age) exits ready {person value} {exit ready {person {name,age}};}\npub control name(&person value) exits found {&string text from value} {exit found {&(*value).name};}';
    await fs.writeFile(moduleFile,moduleSource);
    const importing='import "people.path" as people;\ncontrol main() exits done {delta people.create("Module Ada",37) {ready {auto value}=>created}[created]:delta people.name(&value) {found {auto text}=>named}[named]:emit(text);exit done;}';
    await replace(buildDocument,importing);
    await waitFor(()=>revisedDiagnostics().length===0,'file imports and borrowed result');
    await replace(buildDocument,importing.replace('emit(text);','emit(value.name);'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_VISIBILITY'),'private imported field');
    await replace(buildDocument,importing);
    const moduleDocument=await vscode.workspace.openTextDocument(moduleUri);
    await vscode.window.showTextDocument(moduleDocument);
    await replace(moduleDocument,moduleSource.replace('exit ready {person {name,age}};','u32 wrong="bad";exit ready {person {name,age}};'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_TYPE' && d.relatedInformation?.length),'unsaved dependency error linked from importer');
    await replace(moduleDocument,moduleSource);
    await waitFor(()=>revisedDiagnostics().length===0,'unsaved dependency repair');
    await moduleDocument.save();
    await vscode.window.showTextDocument(buildDocument);
    const moduleRun=await buildWithStatus('tellur.buildAndRun',{output:path.join(root,'native output','modules')}, {done: 0});
    assert.equal(moduleRun.code,0,moduleRun.stderr);assert.ok(moduleRun.stdout.includes('Module Ada'));
    console.log('TELLUR_MODULE_EDITOR_TESTS_PASSED: imports, private fields, unsaved dependency diagnostics with related locations, repair, native execution');
    const forwardSource='control main() exits auto {display();forward relay();}\ncontrol display() exits done {emit("forwarding");exit done;}\ncontrol relay() exits auto {exit delta choose() {ready=>forward};}\ncontrol choose() exits ready {string text} {exit ready {"forward result"};}';
    await replace(buildDocument,forwardSource);
    await waitFor(async()=>{
      const hover=await vscode.commands.executeCommand('vscode.executeHoverProvider',buildUri,new vscode.Position(0,9));
      return revisedDiagnostics().length===0 && hover?.some(h=>h.contents.some(c=>c.value.includes('ready { string')));
    },'forward inferred contract');
    await replace(buildDocument,forwardSource.replace('display() exits done','display() exits done,failed'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_DISPOSITION'),'single completion contract growth');
    await replace(buildDocument,forwardSource.replace('main() exits auto','main() exits done'));
    await waitFor(()=>revisedDiagnostics().some(d=>code(d)==='E_OUTCOME'),'incompatible forwarding');
    await replace(buildDocument,forwardSource);
    await waitFor(()=>revisedDiagnostics().length===0,'forward repair');
    const forwardRun=await buildWithStatus('tellur.buildAndRun',{output:path.join(root,'native output','forwarding')}, {"ready": 0});
    assert.equal(forwardRun.code,0,forwardRun.stderr);
    assert.ok(forwardRun.stdout.includes('forwarding'));
    assert.ok(forwardRun.stdout.includes('forward result'));
    console.log('TELLUR_FORWARD_EDITOR_TESTS_PASSED: inferred contract, contract growth, incompatible forwarding, repair, native execution');
    console.log('TELLUR_COLLECTIONS_EDITOR_TESTS_PASSED: typed literal, borrowed lookup, mutation conflict, last-use repair, native build/run');
    console.log('TELLUR_REFERENCES_EDITOR_TESTS_PASSED: mutable call, returned borrow, lifetime hover, last use, local escape, native run');
    await replace(buildDocument, revised);
    await waitFor(() => revisedDiagnostics().length === 0, 'return to argument test');
    console.log('TELLUR_OWNERSHIP_EDITOR_TESTS_PASSED: loan conflict, reference move, unsaved repair, native mutation');
    console.log('TELLUR_REVISION_EDITOR_TESTS_PASSED: control syntax, inferred contract hover, typed declarations, unsaved repair');
    const message = 'Native editor test: $HOME $(false); quotes \" and 😀';
    const nativeOutput = path.join(root, 'native output', 'program with spaces');
    const built = await buildWithStatus('tellur.build', { output: nativeOutput }, {"done": 0, "empty": 1});
    assert.equal(built.code, 0, built.stderr);
    const ran = await buildWithStatus('tellur.buildAndRun', { output: nativeOutput, arguments: [message] }, {"done": 0, "empty": 1});
    assert.equal(ran.code, 0, ran.stderr);
    assert.ok(ran.stdout.includes(message));
    assert.ok(ran.stdout.includes('done'));
    assert.ok((await fs.readFile(nativeOutput + '.ll', 'utf8')).includes('target triple'));
    // Existing command IDs and explicit legacy language associations still work.
    await vscode.languages.setTextDocumentLanguage(buildDocument, 'path');
    await vscode.window.showTextDocument(buildDocument);
    const legacyRun = await buildWithStatus('path.buildAndRun', { arguments: [message] }, {"done": 0, "empty": 1});
    assert.equal(legacyRun.code, 0, legacyRun.stderr);
    assert.equal(path.basename(legacyRun.executable), process.platform === 'win32' ? 'editor build.exe' : 'editor build');
    assert.ok(legacyRun.stdout.includes(message));
    await fs.access(legacyRun.executable + '.tellur-build/src/main.rs');
    await vscode.commands.executeCommand('path.restartServer');
    await waitFor(async () => {
      const hover = await vscode.commands.executeCommand('vscode.executeHoverProvider', buildUri, new vscode.Position(0, 9));
      return hover?.some(h => h.contents.some(c => c.value.includes('control main')));
    }, 'legacy language ID works after legacy restart command');
    console.log('TELLUR_MIGRATION_EDITOR_TESTS_PASSED: both extensions, legacy language ID and commands, canonical default output name');
    console.log('TELLUR_NATIVE_EDITOR_TESTS_PASSED: build, run, argument boundaries, paths with spaces, LLVM output');
    const contributorSource = `control leaf() exits auto {exit missing;}
control main() exits done {
 auto saved=capture leaf();
 auto mapped=delta saved {missing=>unavailable {}};
 exit delta mapped {unavailable=>done {}};
}`;
    await replace(buildDocument, contributorSource);
    await waitFor(() => revisedDiagnostics().length === 0, 'stored contributor baseline');
    await replace(buildDocument, contributorSource.replace('{unavailable=>done {}}', '{}'));
    await waitFor(() => revisedDiagnostics().some(d => code(d) === 'E_ARMS' && d.relatedInformation?.some(n => n.location.range.start.line === 0) && d.relatedInformation?.some(n => n.location.range.start.line === 3)), 'stored contributor source and adapter locations');
    await replace(buildDocument, contributorSource);
    await waitFor(() => revisedDiagnostics().length === 0, 'stored contributor repair');
    console.log('TELLUR_STORED_CONTRIBUTORS_EDITOR_TESTS_PASSED: capture, adapter, related source locations, unsaved repair');
    await buildDocument.save();
    const jsonCheck = await require('node:util').promisify(require('node:child_process').execFile)(path.join(__dirname, '../server/tellurc'), ['check', buildUri.fsPath, '--json']);
    const checkReport = JSON.parse(jsonCheck.stdout);
    assert.equal(checkReport.schema, 'tellur.check.v1');
    assert.equal(checkReport.ok, true);
    assert.deepEqual(checkReport.diagnostics, []);
    assert.equal(jsonCheck.stderr, '');
    console.log('TELLUR_JSON_CHECK_EDITOR_TESTS_PASSED: bundled compiler versioned report and clean stdout');
    const settingsFolder = path.join(root, 'settings-app');
    await fs.mkdir(settingsFolder, {recursive:true});
    for (const file of ['main.tellur', 'settings.tellur', 'server.tellur', 'http.tellur', 'manifest.json', 'host.rs', 'Cargo.lock']) {
      await fs.copyFile(path.join(__dirname, '../../../applications/settings', file), path.join(settingsFolder, file));
    }
    const settingsUri = vscode.Uri.file(path.join(settingsFolder, 'main.tellur'));
    const settingsDocument = await vscode.workspace.openTextDocument(settingsUri);
    await vscode.window.showTextDocument(settingsDocument);
    const settingsDiagnostics = () => vscode.languages.getDiagnostics(settingsUri).filter(d => d.source === 'tellur');
    const settingsSource = settingsDocument.getText();
    await replace(settingsDocument, settingsSource.replace('found { string value }', 'found { u32 value }'));
    await waitFor(() => settingsDiagnostics().some(d => code(d) === 'E_TYPE'), 'settings application unsaved error');
    await replace(settingsDocument, settingsSource);
    await waitFor(() => settingsDiagnostics().length === 0, 'settings application repair');
    const settingsData = path.join(settingsFolder, 'editor-data.json');
    await fs.rm(settingsData, {force:true});
    const settingsOutput = path.join(root, 'native output', 'settings-app');
    const settingsSet = await vscode.commands.executeCommand('tellur.buildAndRun', {entry:'set', arguments:[settingsData, 'theme', 'dark'], output:settingsOutput});
    assert.equal(settingsSet.code, 0, settingsSet.stderr);assert.ok(settingsSet.stdout.includes('saved'));
    assert.deepEqual(JSON.parse(await fs.readFile(settingsData, 'utf8')), {theme:'dark'});
    const settingsGet = await vscode.commands.executeCommand('tellur.buildAndRun', {entry:'get', arguments:[settingsData, 'theme'], output:settingsOutput});
    assert.equal(settingsGet.code, 0, settingsGet.stderr);assert.ok(settingsGet.stdout.includes('dark'));
    const statusLine = settingsSource.split('\n').findIndex(line => line.startsWith('outcome lookup_result'));
    await waitFor(async () => {
      const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', settingsUri, new vscode.Position(statusLine, 10));
      return hovers?.some(h => h.contents.some(c => c.value.includes('Process exit defaults') && c.value.includes('3')));
    }, 'annotated outcome hover');
    await replace(settingsDocument, settingsSource.replace('@exit 3 missing', '@exit 999 missing'));
    await waitFor(() => settingsDiagnostics().some(d => code(d) === 'E_EXIT_CODE'), 'invalid exit annotation');
    await replace(settingsDocument, settingsSource);
    await waitFor(() => settingsDiagnostics().length === 0, 'exit annotation repair');
    const settingsMissing = await vscode.commands.executeCommand('tellur.buildAndRun', {entry:'get', arguments:['--json', '--', settingsData, 'unknown'], output:settingsOutput});
    assert.equal(settingsMissing.code, 3, settingsMissing.stderr);
    assert.equal(JSON.parse(settingsMissing.stdout).outcome, 'missing');
    assert.equal(settingsMissing.stderr, '');
    console.log('TELLUR_EXIT_CODES_EDITOR_TESTS_PASSED: annotation hover, unsaved diagnostic, repair, exact nonzero status and JSON outcome');
    console.log('TELLUR_RUST_SETTINGS_EDITOR_TESTS_PASSED: manifest-relative Rust adapter, unsaved checking, repair, locked build, persistent update and reload');
    const serverUri = vscode.Uri.file(path.join(settingsFolder, 'server.tellur'));
    const serverDocument = await vscode.workspace.openTextDocument(serverUri);
    await vscode.window.showTextDocument(serverDocument);
    const serverSource = serverDocument.getText();
    await replace(serverDocument, serverSource.replace('http_listener_release(listener);', ''));
    await waitFor(() => vscode.languages.getDiagnostics(serverUri).some(d => code(d) === 'E_RESOURCE'), 'server listener completion diagnostic');
    await replace(serverDocument, serverSource);
    await waitFor(() => vscode.languages.getDiagnostics(serverUri).length === 0, 'server completion repair');
    await serverDocument.save();
    await vscode.window.showTextDocument(settingsDocument);
    const net = require('node:net');
    const reservation = net.createServer();
    await new Promise(resolve => reservation.listen(0, '127.0.0.1', resolve));
    const port = reservation.address().port;
    await new Promise(resolve => reservation.close(resolve));
    const serviceRun = vscode.commands.executeCommand('tellur.buildAndRun', {entry:'serve', arguments:[settingsData, String(port), '1', '3000'], output:settingsOutput});
    let httpBody;
    await waitFor(() => new Promise(resolve => {
      const request = require('node:http').get({host:'127.0.0.1', port, path:'/settings', agent:false}, response => {
        let body = ''; response.setEncoding('utf8'); response.on('data', part => body += part);
        response.on('end', () => { httpBody = body; resolve(response.statusCode === 200); });
        response.on('error', () => resolve(false));
      });
      request.setTimeout(1000, () => request.destroy()); request.on('error', () => resolve(false));
    }), 'settings server HTTP response', 45000);
    assert.deepEqual(JSON.parse(httpBody), {theme:'dark'});
    const serviceResult = await serviceRun;
    assert.equal(serviceResult.code, 0, serviceResult.stderr);
    assert.ok(serviceResult.stdout.includes('completed'));
    console.log('TELLUR_HTTP_SERVER_EDITOR_TESTS_PASSED: listener obligation, unsaved repair, build/run, live HTTP and bounded shutdown');
    const resourceFolder = path.join(root, 'retained-file');
    await fs.mkdir(resourceFolder, {recursive:true});
    for (const file of ['main.tellur', 'manifest.json', 'host.rs', 'Cargo.lock']) {
      await fs.copyFile(path.join(__dirname, '../../../examples/retained_file', file), path.join(resourceFolder, file));
    }
    const resourceUri = vscode.Uri.file(path.join(resourceFolder, 'main.tellur'));
    const resourceDocument = await vscode.workspace.openTextDocument(resourceUri);
    await vscode.window.showTextDocument(resourceDocument);
    const resourceSource = resourceDocument.getText();
    const resourceDiagnostics = () => vscode.languages.getDiagnostics(resourceUri).filter(d => d.source === 'tellur');
    await replace(resourceDocument, resourceSource.replace('file_release(file);', ''));
    await waitFor(() => resourceDiagnostics().some(d => code(d) === 'E_RESOURCE'), 'retained resource requires explicit completion');
    await replace(resourceDocument, resourceSource);
    await waitFor(() => resourceDiagnostics().length === 0, 'retained resource repair');
    const resourceInput = path.join(resourceFolder, 'input.txt');
    await fs.writeFile(resourceInput, 'retained file');
    const resourceRun = await vscode.commands.executeCommand('tellur.buildAndRun', {arguments:['--json', '--', resourceInput], output:path.join(root, 'native output', 'retained-file')});
    assert.equal(resourceRun.code, 0, resourceRun.stderr);
    const resourceReport = JSON.parse(resourceRun.stdout);
    assert.equal(resourceReport.outcome, 'data');
    assert.deepEqual(resourceReport.fields[0].value, Array.from(Buffer.from('retained file')));
    assert.deepEqual(resourceReport.events.map(event => event.operation), ['file_open', 'file_chunk', 'file_release']);
    console.log('TELLUR_RETAINED_RESOURCES_EDITOR_TESTS_PASSED: manifest mapping, unsaved completion diagnostic, repair, locked native build and explicit release');
    console.log('TELLUR_EDITOR_TESTS_PASSED: language recognition, unsaved diagnostics/repair, outline, witness, UTF-16, manifest watcher, restart, partial moves, owned variants');
  } finally {
    await replace(document, original);
    await document.save();
    await vscode.commands.executeCommand('workbench.action.closeAllEditors');
  }
};

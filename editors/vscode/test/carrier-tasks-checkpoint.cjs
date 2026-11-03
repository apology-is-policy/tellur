const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');
exports.run = async function (root) {
  const fixture = path.join(root, 'carrier-tasks');
  await fs.mkdir(fixture, {recursive:true});
  await fs.copyFile(path.join(__dirname, '../../../playground/manifest.json'), path.join(fixture, 'manifest.json'));
  const source = await fs.readFile(path.join(__dirname, '../../../playground/carrier_tasks.tellur'), 'utf8');
  const uri = vscode.Uri.file(path.join(fixture, 'main.tellur'));
  await fs.writeFile(uri.fsPath, source);
  const document = await vscode.workspace.openTextDocument(uri);
  await vscode.window.showTextDocument(document);
  await vscode.extensions.getExtension('tellur-language-local.tellur-language').activate();
  const diagnostics = () => vscode.languages.getDiagnostics(uri).filter(d => d.source === 'tellur');
  const code = d => typeof d.code === 'object' ? d.code.value : d.code;
  async function waitFor(predicate, description) {
    const until = Date.now() + 15000;
    while (Date.now() < until) {
      if (await predicate()) return;
      await new Promise(resolve => setTimeout(resolve, 50));
    }
    throw new Error(`Timed out: ${description}; ${JSON.stringify(diagnostics())}`);
  }
  async function replace(text) {
    const edit = new vscode.WorkspaceEdit();
    edit.replace(uri, new vscode.Range(document.positionAt(0), document.positionAt(document.getText().length)), text);
    assert.ok(await vscode.workspace.applyEdit(edit));
  }
  async function hover(needle) {
    const at = document.getText().lastIndexOf(needle);
    assert.ok(at >= 0);
    const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, document.positionAt(at));
    return (hovers ?? []).flatMap(h => h.contents.map(c => String(c.value))).join('\n');
  }
  await waitFor(async () => diagnostics().length === 0 && (await hover('auto pending')).includes('task<first>'), 'pending task type hover');
  assert.ok((await hover('auto value')).includes('ref string'));
  assert.ok((await hover('defer first')).includes('storage stays borrowed'));
  assert.ok((await hover('await pending')).includes('consumes a cold or started task'));
  const broken = source.replace('emit("carrier borrowed");', 'right = "too soon";');
  assert.notEqual(broken, source);
  await replace(broken);
  await waitFor(() => diagnostics().some(d => code(d) === 'E_LOAN'), 'pending loan conflict');
  assert.ok(!(await hover('auto value')).includes('ref string'));
  await replace(source);
  await waitFor(async () => diagnostics().length === 0 && (await hover('auto pending')).includes('task<first>'), 'repaired ownership');
  await document.save();
  const result = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'carrier-tasks')
  });
  assert.equal(result.code, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.equal(report.outcome, 'done');
  assert.deepEqual(report.events.filter(e => e.kind === 'emit').map(e => e.message), ['carrier borrowed', 'first', 'released at await', 'released after last use']);
  const structured = await fs.readFile(path.join(__dirname, '../../../playground/structured_tasks.tellur'), 'utf8');
  await replace(structured);
  await waitFor(async () => diagnostics().length === 0 && (await hover('spawn work')).includes('queues an invocation-owned child'), 'structured task hover');
  assert.ok((await hover('yield;')).includes('Host quotas do not cause source handoffs'));
  const missingJoin = structured.replace('await second;', '');
  assert.notEqual(missingJoin, structured);
  await replace(missingJoin);
  await waitFor(() => diagnostics().length > 0, 'unjoined child diagnostic');
  await replace(structured);
  await waitFor(async () => diagnostics().length === 0 && (await hover('spawn work')).includes('reference-free'), 'repaired structured ownership');
  await document.save();
  const tasks = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'structured-tasks')
  });
  assert.equal(tasks.code, 0, tasks.stderr);
  assert.deepEqual(JSON.parse(tasks.stdout).events.filter(e => e.kind === 'emit').map(e => e.message),
    ['queued', 'first', 'second', 'parent', 'first', 'second', 'joined first', 'joined second']);
  const helpers = await fs.readFile(path.join(__dirname, '../../../playground/structured_helpers.tellur'), 'utf8');
  await replace(helpers);
  await waitFor(async () => diagnostics().length === 0 && (await hover('auto value')).includes('message'), 'synchronous helper admission');
  const hiddenHandoff = helpers.replace('deref text = "updated";', 'fault boundary {} contingency [] {escalate;} deref text = "updated";');
  assert.notEqual(hiddenHandoff, helpers);
  await replace(hiddenHandoff);
  await waitFor(() => diagnostics().some(d => code(d) === 'E_ASYNC'), 'helper inline boundary invalidates callers');
  await replace(helpers);
  await waitFor(async () => diagnostics().length === 0 && (await hover('auto value')).includes('message'), 'repaired helper body');
  await document.save();
  const helperRun = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'structured-helpers')
  });
  assert.equal(helperRun.code, 0, helperRun.stderr);
  assert.deepEqual(JSON.parse(helperRun.stdout).events.filter(e => e.kind === 'emit').map(e => e.message),
    ['queued', 'constructed', 'updated', 'updated', 'child started', 'updated', 'child finished', 'updated']);
  const settled = 'async control finish(handle h) exits done {delta close(h) {closed=>closed,failed {close_error e}=>failed} [closed|failed]:yield;exit done;} async control startup(handle h) exits done {auto child=spawn finish(h);await child;exit done;}';
  await replace(settled);
  await waitFor(async () => diagnostics().length === 0 && (await hover('auto child')).includes('task<finish>'), 'producer completes owner before handoff');
  const suspendedOwner = settled.replace('delta close(h)', 'yield;delta close(h)').replace('[closed|failed]:yield;', '[closed|failed]:');
  await replace(suspendedOwner);
  await waitFor(() => diagnostics().some(d => code(d) === 'E_ASYNC' && d.message.includes('fault dispositions')), 'spawn refuses suspended owner without disposition');
  await replace(settled);
  await waitFor(async () => diagnostics().length === 0 && (await hover('spawn finish')).includes('producer owners retained across handoffs'), 'repaired producer stop obligations');
  const awaited = 'async control work() exits done {yield;exit done;} ' + settled.replace('auto child=spawn finish(h);', 'auto child=spawn work();await finish(h);');
  await replace(awaited);
  await waitFor(async () => diagnostics().length === 0 && (await hover('auto child')).includes('task<work>'), 'parent await beside owned child');
  await replace(awaited.replace('delta close(h)', 'yield;delta close(h)').replace('[closed|failed]:yield;', '[closed|failed]:'));
  await waitFor(() => diagnostics().some(d => code(d) === 'E_ASYNC' && d.message.includes('await beside started children')), 'awaited producer stop permission refresh');
  const parentAwait = await fs.readFile(path.join(__dirname, '../../../playground/parent_await.tellur'), 'utf8');
  await replace(parentAwait);
  await waitFor(async () => diagnostics().length === 0 && (await hover('spawn worker')).includes('Cold/direct awaits may retain parent borrows'), 'parent await repair and hover');
  await document.save();
  const parentRun = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'parent-await')
  });
  assert.equal(parentRun.code, 0, parentRun.stderr);
  assert.deepEqual(JSON.parse(parentRun.stdout).events.filter(e => e.kind === 'emit').map(e => e.message),
    ['worker started', 'editor started', 'worker done', 'edited', 'editor started']);
  const handing = await fs.readFile(path.join(__dirname, '../../../playground/cooperative_helpers.tellur'), 'utf8');
  await replace(handing);
  await waitFor(async () => diagnostics().length === 0 && (await hover('spawn worker')).includes('inline boundaries'), 'handing helper admission and hover');
  await document.save();
  const handingRun = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'cooperative-helpers')
  });
  assert.equal(handingRun.code, 0, handingRun.stderr);
  assert.deepEqual(JSON.parse(handingRun.stdout).events.filter(e => e.kind === 'emit').map(e => e.message),
    ['helper started', 'inline started', 'worker started', 'helper returned', 'edited', 'worker finished']);
  const borrowed = await fs.readFile(path.join(__dirname, '../../../playground/borrowed_spawn.tellur'), 'utf8');
  await replace(borrowed);
  await waitFor(async () => diagnostics().length === 0 && (await hover('spawn edit')).includes('stay live until the consuming join'), 'direct borrowed spawn admission');
  const conflicting = borrowed.replace('emit("parent");', 'emit(borrow values.left);');
  assert.notEqual(conflicting, borrowed);
  await replace(conflicting);
  await waitFor(() => diagnostics().some(d => code(d) === 'E_LOAN'), 'parent access rejected while child owns exclusive loan');
  await replace(borrowed);
  await waitFor(async () => diagnostics().length === 0 && (await hover('auto right')).includes('task<edit>'), 'borrowed spawn repair');
  await document.save();
  const borrowedRun = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'borrowed-spawn')
  });
  assert.equal(borrowedRun.code, 0, borrowedRun.stderr);
  assert.deepEqual(JSON.parse(borrowedRun.stdout).events.filter(e => e.kind === 'emit').map(e => e.message),
    ['queued', 'first', 'second', 'parent', 'shared', 'shared', 'left edited', 'right edited']);
  const resultSource = await fs.readFile(path.join(__dirname, '../../../playground/borrowed_spawn_results.tellur'), 'utf8');
  await replace(resultSource);
  await waitFor(async () => diagnostics().length === 0 && (await hover('spawn lend')).includes('Direct borrowed results retain their declared origins after join'), 'borrowed spawned result admission');
  const resultConflict = resultSource.replace('deref value = "edited through result";', 'left = "too soon";');
  assert.notEqual(resultConflict, resultSource);
  await replace(resultConflict);
  await waitFor(() => diagnostics().some(d => code(d) === 'E_LOAN'), 'result origin remains borrowed after join');
  await replace(resultSource);
  await waitFor(async () => diagnostics().length === 0 && (await hover('auto value')).includes('ref mutable string'), 'borrowed result repair');
  await document.save();
  const resultRun = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'borrowed-spawn-results')
  });
  assert.equal(resultRun.code, 0, resultRun.stderr);
  assert.deepEqual(JSON.parse(resultRun.stdout).events.filter(e => e.kind === 'emit').map(e => e.message),
    ['queued', 'released at join', 'edited through result', 'released after result', 'right']);
  const carrierSource = await fs.readFile(path.join(__dirname, '../../../playground/carrier_spawn.tellur'), 'utf8');
  await replace(carrierSource);
  await waitFor(async () => diagnostics().length === 0 && (await hover('spawn first')).includes('captures/results may nest'), 'flat carrier admission');
  const carrierConflict = carrierSource.replace('auto moved = move selected;', 'right = "too soon";auto moved = move selected;');
  assert.notEqual(carrierConflict, carrierSource);
  await replace(carrierConflict);
  await waitFor(() => diagnostics().some(d => code(d) === 'E_LOAN'), 'contained capture loan before join');
  await replace(carrierSource);
  await waitFor(async () => diagnostics().length === 0 && (await hover('auto changed')).includes('ref mutable string'), 'carrier loan repair');
  await document.save();
  const carrierRun = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'carrier-spawn')
  });
  assert.equal(carrierRun.code, 0, carrierRun.stderr);
  assert.deepEqual(JSON.parse(carrierRun.stdout).events.filter(e => e.kind === 'emit').map(e => e.message),
    ['queued', 'other capture', 'editing', 'released at join', 'selected', 'edited in child', 'released after edit']);
  const flatResultSource = await fs.readFile(path.join(__dirname, '../../../playground/carrier_spawn_results.tellur'), 'utf8');
  await replace(flatResultSource);
  await waitFor(async () => diagnostics().length === 0 && (await hover('spawn relay')).includes('selected result leaf loans'), 'flat result admission');
  const flatResultConflict = flatResultSource.replace('emit(result.right);', 'right = "too soon";emit(result.right);');
  assert.notEqual(flatResultConflict, flatResultSource);
  await replace(flatResultConflict);
  await waitFor(() => diagnostics().some(d => code(d) === 'E_LOAN'), 'result carrier retains its selected leaf loan');
  await replace(flatResultSource);
  await waitFor(async () => diagnostics().length === 0 && (await hover('auto edited')).includes('edit_view'), 'flat result repair');
  await document.save();
  const flatResultRun = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'carrier-spawn-results')
  });
  assert.equal(flatResultRun.code, 0, flatResultRun.stderr);
  assert.deepEqual(JSON.parse(flatResultRun.stdout).events.filter(e => e.kind === 'emit').map(e => e.message),
    ['queued', 'left', 'right', 'owned result', 'edited in child', 'released after result']);
  const nestedSource = await fs.readFile(path.join(__dirname, '../../../playground/nested_spawn.tellur'), 'utf8');
  await replace(nestedSource);
  await waitFor(async () => diagnostics().length === 0 && (await hover('spawn edit')).includes('captures/results may nest'), 'nested record admission');
  const nestedConflict = nestedSource.replace('emit(fields.right.text);', 'right = "too soon";emit(fields.right.text);');
  assert.notEqual(nestedConflict, nestedSource);
  await replace(nestedConflict);
  await waitFor(() => diagnostics().some(d => code(d) === 'E_LOAN'), 'partially moved nested result retains sibling loan');
  await replace(nestedSource);
  await waitFor(async () => diagnostics().length === 0 && (await hover('auto selected')).includes('cell'), 'nested result repair');
  await document.save();
  const nestedRun = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'nested-spawn')
  });
  assert.equal(nestedRun.code, 0, nestedRun.stderr);
  assert.deepEqual(JSON.parse(nestedRun.stdout).events.filter(e => e.kind === 'emit').map(e => e.message),
    ['shared', 'queued', 'worker', 'shared', 'worker', 'left edited', 'right edited']);
  const tagged = await fs.readFile(path.join(__dirname, '../../../playground/tagged_spawn.tellur'), 'utf8');
  await replace(tagged);
  await waitFor(async () => diagnostics().length === 0 && (await hover('spawn edit')).includes('Named outcomes retain mandatory handling'), 'tagged payload admission');
  await replace(tagged.replace('emit(a);', 'left="too soon";emit(a);'));
  await waitFor(() => diagnostics().some(d => code(d) === 'E_LOAN'), 'selected variant retains its own origin');
  await replace(tagged);
  await waitFor(async () => diagnostics().length === 0 && (await hover('auto selected')).includes('selection'), 'tagged payload repair');
  await document.save();
  const taggedRun = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'tagged-spawn')
  });
  assert.equal(taggedRun.code, 0, taggedRun.stderr);
  assert.deepEqual(JSON.parse(taggedRun.stdout).events.filter(e => e.kind === 'emit').map(e => e.message),
    ['left', 'right', 'empty', 'edited', 'none']);
  const outcomeSource = await fs.readFile(path.join(__dirname, '../../../playground/outcome_spawn.tellur'), 'utf8');
  await replace(outcomeSource);
  await waitFor(async () => diagnostics().length === 0 && (await hover('spawn edit')).includes('Named outcomes retain mandatory handling'), 'named outcome admission');
  await replace(outcomeSource.replace('auto selected = move value.selected;', 'drop(value.selected);auto selected = move value.selected;'));
  await waitFor(() => diagnostics().some(d => code(d) === 'E_RESOURCE'), 'nested outcome cannot be discarded');
  await replace(outcomeSource);
  await waitFor(async () => diagnostics().length === 0 && (await hover('auto selected')).includes('selection'), 'named outcome repair');
  await document.save();
  const outcomeRun = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'outcome-spawn')
  });
  assert.equal(outcomeRun.code, 0, outcomeRun.stderr);
  assert.deepEqual(JSON.parse(outcomeRun.stdout).events.filter(e => e.kind === 'emit').map(e => e.message),
    ['left', 'right', 'empty', 'edited', 'none']);
  const borrowedRecords = await fs.readFile(path.join(__dirname, '../../../playground/borrowed_record_spawn.tellur'), 'utf8');
  await replace(borrowedRecords);
  await waitFor(async () => diagnostics().length === 0 && (await hover('spawn first')).includes('Borrowed record captures retain whole-holder storage'), 'borrowed record admission');
  await replace(borrowedRecords.replace('auto moved = move child;', 'drop(saved);auto moved = move child;'));
  await waitFor(() => diagnostics().some(d => code(d) === 'E_LOAN'), 'borrowed holder retained until join');
  await replace(borrowedRecords.replace('emit(value);', 'left = "too soon";emit(value);'));
  await waitFor(() => diagnostics().some(d => code(d) === 'E_LOAN'), 'selected borrowed record leaf survives join');
  await replace(borrowedRecords);
  await waitFor(async () => diagnostics().length === 0 && (await hover('auto value')).includes('ref string'), 'borrowed record repair');
  await document.save();
  const borrowedRecordRun = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'borrowed-record-spawn')
  });
  assert.equal(borrowedRecordRun.code, 0, borrowedRecordRun.stderr);
  assert.deepEqual(JSON.parse(borrowedRecordRun.stdout).events.filter(e => e.kind === 'emit').map(e => e.message),
    ['queued', 'reader', 'released at join', 'selected', 'edited']);
  const borrowedTags = await fs.readFile(path.join(__dirname, '../../../playground/borrowed_tagged_spawn.tellur'), 'utf8');
  await replace(borrowedTags);
  await waitFor(async () => diagnostics().length === 0 && (await hover('spawn first')).includes('Borrowed option/variant captures'), 'borrowed tagged admission');
  await replace(borrowedTags.replace('auto moved = move pending;', 'drop(saved);auto moved = move pending;'));
  await waitFor(() => diagnostics().some(d => code(d) === 'E_LOAN'), 'borrowed tagged holder retained until join');
  await replace(borrowedTags.replace('emit(a);', 'left = "too soon";emit(a);'));
  await waitFor(() => diagnostics().some(d => code(d) === 'E_LOAN'), 'borrowed selected tag survives join');
  await replace(borrowedTags);
  await waitFor(async () => diagnostics().length === 0 && (await hover('auto a')).includes('ref string'), 'borrowed tagged repair');
  await document.save();
  const borrowedTagRun = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'borrowed-tagged-spawn')
  });
  assert.equal(borrowedTagRun.code, 0, borrowedTagRun.stderr);
  assert.deepEqual(JSON.parse(borrowedTagRun.stdout).events.filter(e => e.kind === 'emit').map(e => e.message),
    ['left', 'right', 'empty', 'edited', 'none']);
  const borrowedOutcomes = await fs.readFile(path.join(__dirname, '../../../playground/borrowed_outcome_spawn.tellur'), 'utf8');
  await replace(borrowedOutcomes);
  await waitFor(async () => diagnostics().length === 0 && (await hover('spawn first')).includes('Borrowed named outcomes retain the owning caller handling obligation'), 'borrowed outcome admission');
  await replace(borrowedOutcomes.replaceAll('consume_envelope<string>(saved);', ''));
  await waitFor(() => diagnostics().some(d => code(d) === 'E_OUTCOME'), 'borrowed reader leaves caller handling obligation');
  await replace(borrowedOutcomes.replace('auto moved = move pending;', 'consume_envelope<string>(saved);auto moved = move pending;'));
  await waitFor(() => diagnostics().some(d => code(d) === 'E_LOAN'), 'borrowed outcome holder retained until join');
  await replace(borrowedOutcomes);
  await waitFor(async () => diagnostics().length === 0 && (await hover('auto a')).includes('ref string'), 'borrowed outcome repair');
  await document.save();
  const borrowedOutcomeRun = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'borrowed-outcome-spawn')
  });
  assert.equal(borrowedOutcomeRun.code, 0, borrowedOutcomeRun.stderr);
  assert.deepEqual(JSON.parse(borrowedOutcomeRun.stdout).events.filter(e => e.kind === 'emit').map(e => e.message),
    ['left', 'right', 'empty', 'edited', 'none']);
  const wholeRecords = await fs.readFile(path.join(__dirname, '../../../playground/borrowed_record_spawn_results.tellur'), 'utf8');
  await replace(wholeRecords);
  await waitFor(async () => diagnostics().length === 0 && (await hover('spawn choose')).includes('Whole borrowed record results retain holder storage'), 'whole record result admission');
  assert.ok((await hover('auto a')).includes('ref view'));
  await replace(wholeRecords.replace('auto copy = a;', 'drop(first);auto copy = a;'));
  await waitFor(() => diagnostics().some(d => code(d) === 'E_LOAN'), 'joined whole record still protects holder storage');
  await replace(wholeRecords.replace('deref value.text = "after";', 'drop(saved);deref value.text = "after";'));
  await waitFor(() => diagnostics().some(d => code(d) === 'E_LOAN'), 'projected exclusive record keeps enclosing holder');
  await replace(wholeRecords);
  await waitFor(async () => diagnostics().length === 0 && (await hover('auto a')).includes('ref view'), 'whole record result repair');
  await document.save();
  const wholeRecordRun = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'borrowed-record-results')
  });
  assert.equal(wholeRecordRun.code, 0, wholeRecordRun.stderr);
  assert.deepEqual(JSON.parse(wholeRecordRun.stdout).events.filter(e => e.kind === 'emit').map(e => e.message),
    ['left', 'right', 'after', 'available']);
  const wholeTags = await fs.readFile(path.join(__dirname, '../../../playground/borrowed_tagged_spawn_results.tellur'), 'utf8');
  await replace(wholeTags);
  await waitFor(async () => diagnostics().length === 0 && (await hover('spawn first')).includes('Whole borrowed option/variant results retain active or empty holder storage'), 'whole tagged result admission');
  assert.ok((await hover('auto result')).includes('ref choice'));
  await replace(wholeTags.replace('auto copy = move result;', 'drop(saved);auto copy = move result;'));
  await waitFor(() => diagnostics().some(d => code(d) === 'E_LOAN'), 'whole variant result keeps holder storage');
  await replace(wholeTags.replace('delta value {', 'drop(saved);delta value {'));
  await waitFor(() => diagnostics().some(d => code(d) === 'E_LOAN'), 'whole option result keeps holder through matching');
  await replace(wholeTags);
  await waitFor(async () => diagnostics().length === 0 && (await hover('auto result')).includes('ref choice'), 'whole tagged result repair');
  await document.save();
  const wholeTagRun = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'borrowed-tagged-results')
  });
  assert.equal(wholeTagRun.code, 0, wholeTagRun.stderr);
  assert.deepEqual(JSON.parse(wholeTagRun.stdout).events.filter(e => e.kind === 'emit').map(e => e.message),
    ['left', 'right', 'empty', 'edited', 'none']);
  const wholeOutcomes = await fs.readFile(path.join(__dirname, '../../../playground/borrowed_outcome_spawn_results.tellur'), 'utf8');
  await replace(wholeOutcomes);
  await waitFor(async () => diagnostics().length === 0 && (await hover('spawn first')).includes('Whole borrowed named-outcome results retain holder storage'), 'whole outcome result admission');
  assert.ok((await hover('auto result')).includes('ref choice'));
  await replace(wholeOutcomes.replace('auto copy = move result;', 'consume_choice<string>(saved);auto copy = move result;'));
  await waitFor(() => diagnostics().some(d => code(d) === 'E_LOAN'), 'whole outcome result keeps holder storage');
  await replace(wholeOutcomes.replace('delta value {', 'consume_reply(saved);delta value {'));
  await waitFor(() => diagnostics().some(d => code(d) === 'E_LOAN'), 'whole exclusive outcome result keeps holder through matching');
  await replace(wholeOutcomes.replaceAll('consume_choice<string>(saved);', ''));
  await waitFor(() => diagnostics().some(d => code(d) === 'E_OUTCOME'), 'whole outcome result leaves owning caller handling');
  await replace(wholeOutcomes);
  await waitFor(async () => diagnostics().length === 0 && (await hover('auto result')).includes('ref choice'), 'whole outcome result repair');
  await document.save();
  const wholeOutcomeRun = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'borrowed-outcome-results')
  });
  assert.equal(wholeOutcomeRun.code, 0, wholeOutcomeRun.stderr);
  assert.deepEqual(JSON.parse(wholeOutcomeRun.stdout).events.filter(e => e.kind === 'emit').map(e => e.message),
    ['left', 'right', 'empty', 'edited', 'none']);
  const recordChains = await fs.readFile(path.join(__dirname, '../../../playground/record_chain_spawn.tellur'), 'utf8');
  await replace(recordChains);
  await waitFor(async () => diagnostics().length === 0 && (await hover('spawn choose')).includes('Record-only reference chains retain every intermediate holder'), 'record reference chain admission');
  assert.ok((await hover('auto a')).includes('ref view'));
  await replace(recordChains.replace('auto moved = move pending;', 'drop(first);auto moved = move pending;'));
  await waitFor(() => diagnostics().some(d => code(d) === 'E_LOAN'), 'record chain keeps intermediate holder');
  await replace(recordChains.replace('emit(detached);', 'left = "bad";emit(detached);'));
  await waitFor(() => diagnostics().some(d => code(d) === 'E_LOAN'), 'record chain keeps selected supplier');
  await replace(recordChains);
  await waitFor(async () => diagnostics().length === 0 && (await hover('auto a')).includes('ref view'), 'record chain repair');
  await document.save();
  const recordChainRun = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'record-chains')
  });
  assert.equal(recordChainRun.code, 0, recordChainRun.stderr);
  assert.deepEqual(JSON.parse(recordChainRun.stdout).events.filter(e => e.kind === 'emit').map(e => e.message), ['left', 'right']);
  const taggedChains = await fs.readFile(path.join(__dirname, '../../../playground/tagged_chain_spawn.tellur'), 'utf8');
  await replace(taggedChains);
  await waitFor(async () => diagnostics().length === 0 && (await hover('spawn choose')).includes('Option and closed-variant reference chains retain active or empty holder storage'), 'tagged reference chain admission');
  assert.ok((await hover('auto a')).includes('ref view'));
  await replace(taggedChains.replace('auto moved = move pending;', 'drop(first);auto moved = move pending;'));
  await waitFor(() => diagnostics().some(d => code(d) === 'E_LOAN'), 'tagged chain keeps intermediate holder');
  await replace(taggedChains.replace('emit(detached);', 'left = "bad";emit(detached);'));
  await waitFor(() => diagnostics().some(d => code(d) === 'E_LOAN'), 'tagged chain keeps selected supplier');
  await replace(taggedChains);
  await waitFor(async () => diagnostics().length === 0 && (await hover('auto a')).includes('ref view'), 'tagged chain repair');
  await document.save();
  const taggedChainRun = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'tagged-chains')
  });
  assert.equal(taggedChainRun.code, 0, taggedChainRun.stderr);
  assert.deepEqual(JSON.parse(taggedChainRun.stdout).events.filter(e => e.kind === 'emit').map(e => e.message), ['left', 'right']);
  await fs.writeFile(path.join(fixture, 'passed.txt'), 'cold and started tasks, synchronous helpers, diagnostics, unsaved repair and native execution passed\n');
  console.log('TELLUR_CARRIER_TASKS_EDITOR_TESTS_PASSED: pending type hover, diagnostic repair and native run');
};

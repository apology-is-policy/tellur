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
  await waitFor(async () => diagnostics().length === 0 && (await hover('spawn first')).includes('Owned record, option and variant captures/results'), 'flat carrier admission');
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
  await waitFor(async () => diagnostics().length === 0 && (await hover('spawn edit')).includes('option and variant'), 'tagged payload admission');
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
  await fs.writeFile(path.join(fixture, 'passed.txt'), 'cold and started tasks, synchronous helpers, diagnostics, unsaved repair and native execution passed\n');
  console.log('TELLUR_CARRIER_TASKS_EDITOR_TESTS_PASSED: pending type hover, diagnostic repair and native run');
};

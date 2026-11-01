const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');

exports.run = async function (root) {
  const fixture = path.join(root, 'symbolic-constructor-hover');
  await fs.mkdir(fixture, {recursive:true});
  await fs.copyFile(path.join(__dirname, '../../../playground/manifest.json'), path.join(fixture, 'manifest.json'));
  const source = await fs.readFile(path.join(__dirname, '../../../playground/generic_construction.tellur'), 'utf8');
  const uri = vscode.Uri.file(path.join(fixture, 'generic_construction.tellur'));
  await fs.writeFile(uri.fsPath, source);
  const document = await vscode.workspace.openTextDocument(uri);
  await vscode.window.showTextDocument(document);
  await vscode.extensions.getExtension('tellur-language-local.tellur-language').activate();
  const diagnostics = () => vscode.languages.getDiagnostics(uri).filter(d => d.source === 'tellur');
  async function waitFor(predicate, description) {
    const until = Date.now() + 15000;
    while (Date.now() < until) {
      if (await predicate()) return;
      await new Promise(resolve => setTimeout(resolve, 50));
    }
    throw new Error(`Timed out: ${description}`);
  }
  async function replace(text) {
    const edit = new vscode.WorkspaceEdit();
    edit.replace(uri, new vscode.Range(document.positionAt(0), document.positionAt(document.getText().length)), text);
    assert.ok(await vscode.workspace.applyEdit(edit));
  }
  async function hover(needle = 'box<item>(move input)') {
    const at = document.getText().indexOf(needle);
    assert.ok(at >= 0);
    const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, document.positionAt(at));
    return (hovers ?? []).flatMap(h => h.contents.map(c => String(c.value))).join('\n');
  }
  await waitFor(async () => diagnostics().length === 0 && (await hover()).includes('Symbolic call contract'), 'checked symbolic constructor');
  const initial = await hover();
  assert.ok(initial.includes('constructor(item input)'), initial);
  assert.ok(initial.includes('Selected histories'), initial);
  assert.ok(!initial.includes('_path_') && !initial.includes('$0$') && !initial.includes('string'), initial);
  const borrowed = await hover('view<item>(input)');
  assert.ok(borrowed.includes('constructor(ref item input)[input]'), borrowed);
  assert.ok(borrowed.includes("['source]"), borrowed);

  await replace(source.replace('self.value = move input;', ''));
  await waitFor(() => diagnostics().length > 0, 'incomplete generic initialization');
  assert.ok(!(await hover()).includes('Constructor contract'));
  const renamed = source.replace('constructor(item input)', 'constructor(item fresh)').replace('self.value = move input;', 'self.value = move fresh;');
  await replace(renamed);
  await waitFor(async () => diagnostics().length === 0 && (await hover()).includes('constructor(item fresh)'), 'fresh constructor parameter name');
  await replace(source);
  await waitFor(async () => diagnostics().length === 0 && (await hover()) === initial, 'restored constructor contract');
  await document.save();
  const result = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'symbolic-constructor-hover')
  });
  assert.equal(result.code, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.equal(report.outcome, 'done');
  assert.deepEqual(report.events.filter(e => e.kind === 'emit').map(e => e.message), ['Hello through generic construction']);
  await fs.writeFile(path.join(fixture, 'passed.txt'), 'symbolic constructor inputs, ordered origins, invalidation/repair, refreshed parameter names and native generic construction passed\n');
  console.log('TELLUR_SYMBOLIC_CONSTRUCTOR_HOVER_EDITOR_TESTS_PASSED: symbolic constructor contracts, origins, edits and native execution');
};

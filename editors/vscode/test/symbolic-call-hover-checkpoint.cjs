const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');

exports.run = async function (root) {
  const fixture = path.join(root, 'symbolic-call-hover');
  await fs.mkdir(fixture, {recursive:true});
  await fs.copyFile(path.join(__dirname, '../../../playground/manifest.json'), path.join(fixture, 'manifest.json'));
  const source = await fs.readFile(path.join(__dirname, '../../../playground/generic_controls.tellur'), 'utf8') +
    '\ncontrol inspect<item>(ref item input) exits auto {\n    forward view<item>(input);\n}\n';
  const uri = vscode.Uri.file(path.join(fixture, 'generic_controls.tellur'));
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
  async function hover(needle) {
    const at = document.getText().indexOf(needle);
    assert.ok(at >= 0);
    const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, document.positionAt(at));
    return (hovers ?? []).flatMap(h => h.contents.map(c => String(c.value))).join('\n');
  }
  const call = 'identity<item>(value)';
  await waitFor(async () => diagnostics().length === 0 && (await hover(call)).includes('universally checked'), 'checked symbolic call');
  const initial = await hover(call);
  assert.ok(initial.includes('control identity<item>(item value) exits delivered<item>'), initial);
  assert.ok(initial.includes('Contract alternatives: `delivered { item value }`'), initial);
  assert.ok(initial.includes('Selected histories'), initial);
  assert.ok(!initial.includes('_path_') && !initial.includes('$0$'), initial);
  const borrowed = await hover('view<item>(input)');
  assert.ok(borrowed.includes('ref item') && borrowed.includes('payload 1 borrows `value`'), borrowed);

  await replace(source.replace('exit delivered { value };', 'exit delivered { true };'));
  await waitFor(() => diagnostics().some(d => (typeof d.code === 'object' ? d.code.value : d.code) === 'E_TYPE'), 'failed generic definition');
  assert.ok(!(await hover(call)).includes('Control contract'));
  const renamed = source.replace('control identity<item>(item value)', 'control identity<item>(item input)')
    .replace('exit delivered { value };', 'exit delivered { input };');
  await replace(renamed);
  await waitFor(async () => diagnostics().length === 0 && (await hover(call)).includes('identity<item>(item input)'), 'fresh generic parameter name');
  await replace(source);
  await waitFor(async () => diagnostics().length === 0 && (await hover(call)) === initial, 'restored generic definition');
  await document.save();
  const result = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'symbolic-call-hover')
  });
  assert.equal(result.code, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.equal(report.outcome, 'done');
  assert.deepEqual(report.events.filter(e => e.kind === 'emit').map(e => e.message), ['Ada']);
  await fs.writeFile(path.join(fixture, 'passed.txt'), 'symbolic named contracts, checked borrow origins, invalidation/repair, refreshed parameter names and native generic execution passed\n');
  console.log('TELLUR_SYMBOLIC_CALL_HOVER_EDITOR_TESTS_PASSED: symbolic contracts, lifetime evidence, edits and native execution');
};

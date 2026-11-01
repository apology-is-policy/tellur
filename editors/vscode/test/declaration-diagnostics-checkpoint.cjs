const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');

exports.run = async function (root) {
  const fixture = path.join(root, 'declaration-diagnostics');
  await fs.mkdir(fixture, {recursive:true});
  await fs.copyFile(path.join(__dirname, '../../../playground/manifest.json'), path.join(fixture, 'manifest.json'));
  const source = await fs.readFile(path.join(__dirname, '../../../playground/declaration_diagnostics.tellur'), 'utf8');
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
    throw new Error(`Timed out: ${description}`);
  }
  async function replace(text) {
    const edit = new vscode.WorkspaceEdit();
    edit.replace(uri, new vscode.Range(document.positionAt(0), document.positionAt(document.getText().length)), text);
    assert.ok(await vscode.workspace.applyEdit(edit));
  }
  async function hover() {
    const at = document.positionAt(document.getText().indexOf('shifted(10'));
    const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, at);
    return (hovers ?? []).flatMap(h => h.contents.map(c => String(c.value))).join('\n');
  }
  await waitFor(async () => diagnostics().length === 0 && (await hover()).includes('Control contract'), 'initial call contract');
  const duplicate = source.replace('u32 offset', 'u32 value').replace('u32 factor', 'u32 value');
  assert.notEqual(duplicate, source);
  await replace(duplicate);
  await waitFor(() => diagnostics().length === 2 && diagnostics().every(d => code(d) === 'E_NAME'), 'two declaration errors');
  assert.ok(!(await hover()).includes('Control contract'));
  await replace(source.replace('u32 factor', 'u32 value'));
  await waitFor(() => diagnostics().length === 1 && code(diagnostics()[0]) === 'E_NAME', 'one declaration repaired');
  assert.ok(!(await hover()).includes('Control contract'));
  const inferred = source.replace('exits sum { u32 value }, overflow', 'exits auto')
    .replace('exits product { u32 value }, overflow', 'exits auto');
  assert.notEqual(inferred, source);
  await replace(inferred);
  await waitFor(() => diagnostics().length === 2 && diagnostics().every(d => code(d) === 'E_PUBLIC_CONTRACT'), 'two public shape errors');
  await replace(source);
  await waitFor(async () => diagnostics().length === 0 && (await hover()).includes('u32 offset'), 'full declaration repair and hover');
  await document.save();
  const result = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'declaration-diagnostics')
  });
  assert.equal(result.code, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.equal(report.outcome, 'done');
  assert.deepEqual(report.fields, [{kind:'u32',value:36}]);
  await fs.writeFile(path.join(fixture, 'passed.txt'), 'two public shape errors, two signature errors, independent repairs, no stale hover, native result 36 passed\n');
  console.log('TELLUR_DECLARATION_DIAGNOSTICS_EDITOR_TESTS_PASSED: declaration batches, phase repair, fresh hover and native result 36');
};

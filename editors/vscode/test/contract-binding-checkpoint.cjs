const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');

exports.run = async function (root) {
  const fixture = path.join(root, 'contract-binding');
  await fs.mkdir(fixture, {recursive:true});
  await fs.copyFile(path.join(__dirname, '../../../playground/manifest.json'), path.join(fixture, 'manifest.json'));
  const source = await fs.readFile(path.join(__dirname, '../../../playground/contract_binding_diagnostics.tellur'), 'utf8');
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
  async function hover(needle) {
    const offset = document.getText().indexOf(needle);
    assert.ok(offset >= 0, needle);
    const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, document.positionAt(offset));
    return (hovers ?? []).flatMap(h => h.contents.map(c => String(c.value))).join('\n');
  }
  await waitFor(async () => diagnostics().length === 0 && (await hover('first(borrow')).includes('Control contract'), 'initial borrowed call contract');
  assert.ok((await hover('auto text')).includes('ref string'));
  const broken = source.replaceAll('exits view[input]', 'exits view[input, input]');
  assert.notEqual(broken, source);
  await replace(broken);
  await waitFor(() => diagnostics().length === 2 && diagnostics().every(d => code(d) === 'E_ORIGIN'), 'two named completion binding errors');
  assert.ok(!(await hover('first(borrow')).includes('Control contract'));
  assert.ok(!(await hover('auto text')).includes('ref string'));
  await replace(broken.replace('exits view[input, input]', 'exits view[input]'));
  await waitFor(() => diagnostics().length === 1 && code(diagnostics()[0]) === 'E_ORIGIN', 'one binding repaired');
  assert.ok(!(await hover('first(borrow')).includes('Control contract'));
  await replace(source);
  await waitFor(async () => diagnostics().length === 0 && (await hover('first(borrow')).includes('exits view[input]'), 'both bindings repaired');
  assert.ok((await hover('auto text')).includes('ref string'));
  await document.save();
  const result = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'contract-binding')
  });
  assert.equal(result.code, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.equal(report.outcome, 'done');
  assert.deepEqual(report.fields, []);
  assert.deepEqual(report.events.filter(e => e.kind === 'emit').map(e => e.message), ['First borrowed view', 'Second borrowed view']);
  await fs.writeFile(path.join(fixture, 'passed.txt'), 'two binding errors, independent repairs, fresh call and auto hover, native borrowed views passed\n');
  console.log('TELLUR_CONTRACT_BINDING_EDITOR_TESTS_PASSED: binding batches, partial/full repair, fresh hover and native borrowed views');
};

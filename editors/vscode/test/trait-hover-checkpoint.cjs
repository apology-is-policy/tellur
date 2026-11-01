const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');

exports.run = async function (root) {
  const fixture = path.join(root, 'trait-hover');
  await fs.mkdir(fixture, {recursive:true});
  await fs.copyFile(path.join(__dirname, '../../../playground/manifest.json'), path.join(fixture, 'manifest.json'));
  const original = await fs.readFile(path.join(__dirname, '../../../playground/traits.tellur'), 'utf8');
  const source = original.replace('control read(ref self input) exits read_result<item> {', 'control read(ref self owner) exits read_result<item> {').replace('borrow input.text', 'borrow owner.text');
  const uri = vscode.Uri.file(path.join(fixture, 'main.tellur'));
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
  async function hover() {
    const offset = document.getText().indexOf('reader<text_source>.read(');
    assert.ok(offset >= 0);
    const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, document.positionAt(offset));
    return (hovers ?? []).flatMap(h => h.contents.map(c => String(c.value))).join('\n');
  }
  await waitFor(async () => diagnostics().length === 0 && (await hover()).includes('Trait control contract'), 'checked trait contract');
  const initial = await hover();
  assert.ok(initial.includes('control reader<text_source>.read(ref text_source input) exits read_result<string>'));
  assert.ok(initial.includes('received { string value }') && initial.includes('Selected histories'));
  assert.ok(!initial.includes('owner') && !initial.includes('input.text'));
  await replace(source.replace('control read(ref self owner)', 'control read(self owner)'));
  await waitFor(() => diagnostics().some(d => (typeof d.code === 'object' ? d.code.value : d.code) === 'E_TRAIT_SIGNATURE'), 'implementation ownership mismatch');
  assert.ok(!(await hover()).includes('Trait control contract'));
  const fresh = source.replace('control read(ref self input) exits read_result<item>;', 'control read(ref self fresh) exits read_result<item>;').replace('item value', 'item selected');
  await replace(fresh);
  await waitFor(async () => diagnostics().length === 0 && (await hover()).includes('ref text_source fresh'), 'fresh trait parameter');
  assert.ok((await hover()).includes('received { string selected }'));
  await replace(source);
  await waitFor(async () => diagnostics().length === 0 && (await hover()).includes('ref text_source input'), 'restored trait contract');
  await document.save();
  const result = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'trait-hover')
  });
  assert.equal(result.code, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.equal(report.outcome, 'done');
  assert.deepEqual(report.events.filter(e => e.kind === 'emit').map(e => e.message), ['Hello through a trait']);
  await fs.writeFile(path.join(fixture, 'passed.txt'), 'declared trait names, concrete associated types, invalid-source suppression, fresh contract repair and native trait execution passed\n');
  console.log('TELLUR_TRAIT_HOVER_EDITOR_TESTS_PASSED: declared contracts, concrete associated types, fresh repairs and native trait execution');
};

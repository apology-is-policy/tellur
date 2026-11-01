const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');

exports.run = async function (root) {
  const fixture = path.join(root, 'constructor-hover');
  await fs.mkdir(fixture, {recursive:true});
  await fs.copyFile(path.join(__dirname, '../../../playground/manifest.json'), path.join(fixture, 'manifest.json'));
  const source = await fs.readFile(path.join(__dirname, '../../../playground/borrowed_construction.tellur'), 'utf8');
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
  async function hover(needle) {
    const offset = document.getText().indexOf(needle);
    assert.ok(offset >= 0, needle);
    const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, document.positionAt(offset));
    return (hovers ?? []).flatMap(h => h.contents.map(c => String(c.value))).join('\n');
  }
  await waitFor(async () => diagnostics().length === 0 && (await hover('text_pair(borrow')).includes('Constructor contract'), 'shared constructor contract');
  const shared = await hover('text_pair(borrow');
  assert.ok(shared.includes('constructor(ref string left, ref string right)[left, right]'));
  assert.ok(shared.includes("['left, 'right]") && shared.includes('Selected histories'));
  assert.ok((await hover('editable_text(borrow')).includes('constructor(ref mutable string input)[input]'));
  await replace(source.replace('self.text = input;', 'self.text = true;'));
  await waitFor(() => diagnostics().some(d => (typeof d.code === 'object' ? d.code.value : d.code) === 'E_BORROW'), 'invalid constructor assignment');
  assert.equal(document.getText(diagnostics()[0].range), 'true');
  assert.ok(!(await hover('editable_text(borrow')).includes('Constructor contract'));
  const fresh = source.replaceAll('input', 'fresh');
  await replace(fresh);
  await waitFor(async () => diagnostics().length === 0 && (await hover('editable_text(borrow')).includes('constructor(ref mutable string fresh)[fresh]'), 'fresh parameter and origin names');
  await replace(source);
  await waitFor(async () => diagnostics().length === 0 && (await hover('editable_text(borrow')).includes('[input]'), 'restored constructor contract');
  await document.save();
  const result = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'constructor-hover')
  });
  assert.equal(result.code, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.equal(report.outcome, 'done');
  assert.deepEqual(report.events.filter(e => e.kind === 'emit').map(e => e.message), ['first', 'second', 'edited through a constructed view', 'released again']);
  await fs.writeFile(path.join(fixture, 'passed.txt'), 'shared/exclusive constructor contracts, ordered origins, invalid-source suppression, fresh repair and native borrowed construction passed\n');
  console.log('TELLUR_CONSTRUCTOR_HOVER_EDITOR_TESTS_PASSED: checked constructor contracts, fresh repairs and native borrowed construction');
};

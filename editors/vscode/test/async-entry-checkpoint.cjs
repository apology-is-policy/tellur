const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');
exports.run = async function (root) {
  const fixture = path.join(root, 'async-entry');
  await fs.mkdir(fixture, {recursive:true});
  await fs.copyFile(path.join(__dirname, '../../../playground/manifest.json'), path.join(fixture, 'manifest.json'));
  const source = await fs.readFile(path.join(__dirname, '../../../playground/async_entry.tellur'), 'utf8');
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
  await waitFor(async () => diagnostics().length === 0 && (await hover('startup()')).includes('async control'), 'async contract hover');
  assert.ok((await hover('run startup')).includes('does not catch faults'));
  assert.ok((await hover('await greeting')).includes('caller storage and loans'));
  const broken = source.replace('await greeting(', 'greeting(');
  assert.notEqual(broken, source);
  await replace(broken);
  await waitFor(() => diagnostics().length === 1 && code(diagnostics()[0]) === 'E_ASYNC', 'missing await');
  assert.equal(document.getText(diagnostics()[0].range), 'greeting("Tellur")');
  assert.ok(!(await hover('startup()')).includes('async control'));
  await replace(source);
  await waitFor(async () => diagnostics().length === 0 && (await hover('startup()')).includes('async control'), 'repaired permission');
  await document.save();
  const result = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'async-entry')
  });
  assert.equal(result.code, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.equal(report.outcome, 'done');
  assert.deepEqual(report.events.filter(e => e.kind === 'emit').map(e => e.message), ['Hello, Tellur']);
  await fs.writeFile(path.join(fixture, 'passed.txt'), 'async hover, exact permission diagnostic, unsaved repair and native execution passed\n');
  console.log('TELLUR_ASYNC_ENTRY_EDITOR_TESTS_PASSED: async hover, diagnostic repair and native run');
};

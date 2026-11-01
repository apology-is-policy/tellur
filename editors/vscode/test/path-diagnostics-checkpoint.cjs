const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');

exports.run = async function (root) {
  const fixture = path.join(root, 'path-diagnostics-checkpoint');
  await fs.mkdir(fixture, {recursive:true});
  await fs.copyFile(path.join(__dirname, '../../../playground/manifest.json'), path.join(fixture, 'manifest.json'));
  const uri = vscode.Uri.file(path.join(fixture, 'path_diagnostics.tellur'));
  const source = await fs.readFile(path.join(__dirname, '../../../playground/path_diagnostics.tellur'), 'utf8');
  await fs.writeFile(uri.fsPath, source);
  const document = await vscode.workspace.openTextDocument(uri);
  await vscode.window.showTextDocument(document);
  await vscode.extensions.getExtension('tellur-language-local.tellur-language').activate();
  const diagnostics = () => vscode.languages.getDiagnostics(uri).filter(d => d.source === 'tellur');
  const code = d => typeof d.code === 'object' ? d.code.value : d.code;
  async function waitFor(predicate, description) {
    const end = Date.now() + 15000;
    while (Date.now() < end) {
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
  async function inferredTitle(type) {
    const position = document.positionAt(document.getText().indexOf('auto title'));
    const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, position);
    return hovers?.some(h => h.contents.some(c => String(c.value).includes(`${type} title`)));
  }
  await waitFor(async () => diagnostics().length === 0 && await inferredTitle('string'), 'initial inferred title');
  const broken = source.replace('count = 3', 'count = true').replace('summary = "Brief"', 'summary = 7');
  await replace(broken);
  await waitFor(() => diagnostics().length === 2 && diagnostics().every(d => code(d) === 'E_TYPE'), 'both disjoint path initializer errors');
  assert.deepEqual(diagnostics().map(d => document.getText(d.range)), ['true', '7']);
  assert.ok(!(await inferredTitle('string')), 'a failed replay must not publish inferred hover');
  await replace(broken.replace('count = true', 'count = 3'));
  await waitFor(() => diagnostics().length === 1 && document.getText(diagnostics()[0].range) === '7', 'one repaired initializer');
  const changed = source.replace('auto title = "Detailed";', 'auto title = 7;').replace('        emit(borrow title);\n', '');
  await replace(changed);
  await waitFor(async () => diagnostics().length === 0 && await inferredTitle('u32'), 'fresh inferred type after repair');
  await replace(source);
  await waitFor(async () => diagnostics().length === 0 && await inferredTitle('string'), 'restore runnable example');
  await document.save();
  const result = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'path-diagnostics')
  });
  assert.equal(result.code, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.equal(report.outcome, 'done');
  assert.deepEqual(report.events.filter(e => e.kind === 'emit').map(e => e.message), ['Detailed', 'Brief']);
  await fs.writeFile(path.join(fixture, 'passed.txt'), 'independent flat/nested path errors, expression ranges, dependent suppression, partial repair, fresh auto hover and native execution passed\n');
  console.log('TELLUR_PATH_DIAGNOSTICS_EDITOR_TESTS_PASSED: two errors, dependent suppression, exact ranges, partial repair, fresh auto hover and native execution');
};

const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');

exports.run = async function (root) {
  const fixture = path.join(root, 'constructor-access');
  await fs.mkdir(fixture, {recursive:true});
  await fs.copyFile(path.join(__dirname, '../../../playground/manifest.json'), path.join(fixture, 'manifest.json'));
  for (const name of ['main.tellur', 'messages.tellur']) {
    await fs.copyFile(path.join(__dirname, '../../../playground/constructor_access', name), path.join(fixture, name));
  }
  const uri = vscode.Uri.file(path.join(fixture, 'main.tellur'));
  const document = await vscode.workspace.openTextDocument(uri);
  const dependency = await vscode.workspace.openTextDocument(vscode.Uri.file(path.join(fixture, 'messages.tellur')));
  const original = dependency.getText();
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
    edit.replace(dependency.uri, new vscode.Range(dependency.positionAt(0), dependency.positionAt(dependency.getText().length)), text);
    assert.ok(await vscode.workspace.applyEdit(edit));
  }
  async function hover() {
    const at = document.getText().indexOf('messages.envelope("Second');
    assert.ok(at >= 0);
    const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, document.positionAt(at));
    return (hovers ?? []).flatMap(h => h.contents.map(c => String(c.value))).join('\n');
  }
  await waitFor(async () => diagnostics().length === 0 && (await hover()).includes('Constructor contract'), 'initial constructor contract');
  const initial = await hover();
  const privateConstructors = original.replaceAll('pub constructor', 'constructor');
  await replace(privateConstructors);
  await waitFor(() => diagnostics().length === 2 && diagnostics().every(d => code(d) === 'E_PRIVATE'), 'two inaccessible constructors');
  assert.ok(!(await hover()).includes('Constructor contract'));
  const locations = diagnostics().map(d => d.range.start.line).sort((a,b) => a-b);
  assert.ok(locations[0] < locations[1]);
  await replace(privateConstructors.replace('    constructor(', '    pub constructor('));
  await waitFor(() => diagnostics().length === 1 && code(diagnostics()[0]) === 'E_PRIVATE', 'one public constructor repaired');
  assert.equal(diagnostics()[0].range.start.line, locations[1]);
  assert.ok(!(await hover()).includes('Constructor contract'));
  await replace(original);
  await waitFor(async () => diagnostics().length === 0 && (await hover()) === initial, 'full repair and fresh contract');
  await dependency.save();
  const result = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'constructor-access')
  });
  assert.equal(result.code, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.equal(report.outcome, 'done');
  assert.deepEqual(report.events.filter(e => e.kind === 'emit').map(e => e.message), ['First constructor', 'Second constructor']);
  await fs.writeFile(path.join(fixture, 'passed.txt'), 'two privacy errors, independent unsaved dependency repair, no stale hover and native constructor access passed\n');
  console.log('TELLUR_CONSTRUCTOR_ACCESS_EDITOR_TESTS_PASSED: privacy batches, independent repair, fresh hover and native execution');
};

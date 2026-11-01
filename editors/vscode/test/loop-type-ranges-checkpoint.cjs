const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');

exports.run = async function (root) {
  const fixture = path.join(root, 'loop-type-ranges');
  await fs.mkdir(fixture, {recursive:true});
  await fs.copyFile(path.join(__dirname, '../../../playground/manifest.json'), path.join(fixture, 'manifest.json'));
  const program = await fs.readFile(path.join(__dirname, '../../../playground/count.tellur'), 'utf8');
  const countAt = program.indexOf('control count(');
  const statusAt = program.indexOf('// Process status policy');
  assert.ok(countAt >= 0 && statusAt > countAt);
  const main = 'import "counter.tellur" as counter;\n' +
    program.slice(0, countAt).replace('count(5)', 'counter.count(5)') + program.slice(statusAt);
  const counter = program.slice(countAt, statusAt)
    .replace('control count(', 'pub control count(')
    .replace('exits auto', 'exits done, arithmetic_fault');
  await fs.writeFile(path.join(fixture, 'main.tellur'), main);
  await fs.writeFile(path.join(fixture, 'counter.tellur'), counter);
  const uri = vscode.Uri.file(path.join(fixture, 'main.tellur'));
  const document = await vscode.workspace.openTextDocument(uri);
  const dependency = await vscode.workspace.openTextDocument(vscode.Uri.file(path.join(fixture, 'counter.tellur')));
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
    const at = document.getText().indexOf('counter.count(5)');
    assert.ok(at >= 0);
    const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, document.positionAt(at));
    return (hovers ?? []).flatMap(h => h.contents.map(c => String(c.value))).join('\n');
  }
  await waitFor(async () => diagnostics().length === 0 && (await hover()).includes('done'), 'initial count contract');
  const initial = await hover();
  for (const annotation of ['vector<\n        // Unicode: 😀\n        string,\n        u32\n    >', 'vector<string, u32>']) {
    const broken = original.replace('u32 number = 1', `${annotation} number = 1`);
    assert.notEqual(broken, original);
    await replace(broken);
    await waitFor(() => diagnostics().length === 1 && code(diagnostics()[0]) === 'E_MANIFEST', 'invalid loop parameter type');
    assert.ok(!(await hover()).includes('done'));
    const related = diagnostics()[0].relatedInformation.find(info => info.location.uri.toString() === dependency.uri.toString());
    assert.ok(related, 'original imported type location');
    const at = dependency.getText().indexOf(annotation);
    assert.ok(at >= 0);
    const expected = new vscode.Range(dependency.positionAt(at), dependency.positionAt(at + annotation.length));
    await waitFor(() => diagnostics()[0]?.relatedInformation.some(info => info.location.uri.toString() === dependency.uri.toString() && info.location.range.isEqual(expected)), 'fresh exact annotation range');
  }
  await replace(original);
  await waitFor(async () => diagnostics().length === 0 && (await hover()) === initial, 'full repair and fresh contract');
  await dependency.save();
  const result = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'loop-type-ranges')
  });
  assert.equal(result.code, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.equal(report.outcome, 'done');
  assert.deepEqual(report.events.filter(e => e.kind === 'emit').map(e => e.message), ['1', '2', '3', '4', '5']);
  await fs.writeFile(path.join(fixture, 'passed.txt'), 'multiline and compact type ranges, imported unsaved repair, no stale hover and native loop type annotation passed\n');
  console.log('TELLUR_LOOP_TYPE_RANGES_EDITOR_TESTS_PASSED: exact type ranges, unsaved repair, fresh hover and native execution');
};

const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');

exports.run = async function (root) {
  const fixture = path.join(root, 'trait-declaration-diagnostics');
  await fs.mkdir(fixture, {recursive:true});
  await fs.copyFile(path.join(__dirname, '../../../playground/manifest.json'), path.join(fixture, 'manifest.json'));
  // Keep the runnable trait example and add an independent unused interface.
  const example = await fs.readFile(path.join(__dirname, '../../../playground/traits.tellur'), 'utf8');
  const source = example + '\ntrait reporter {control report() exits done;}\n';
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
    const at = document.getText().indexOf('reader<text_source>.read(borrow source)');
    assert.ok(at >= 0);
    const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, document.positionAt(at));
    return (hovers ?? []).flatMap(h => h.contents.map(c => String(c.value))).join('\n');
  }
  await waitFor(async () => diagnostics().length === 0 && (await hover()).includes('Trait control contract'), 'initial trait contract');
  const initial = await hover();
  const broken = source.replace('control read(ref self input) exits read_result<item>;',
    'control read(ref self input) exits auto;').replace('control report() exits done;',
    'control report() exits auto;');
  assert.notEqual(broken, source);
  await replace(broken);
  await waitFor(() => diagnostics().length === 2 && diagnostics().every(d => code(d) === 'E_TRAIT_SIGNATURE'), 'two invalid trait declarations');
  assert.ok(!(await hover()).includes('Trait control contract'));
  const locations = diagnostics().map(d => d.range.start.line).sort((a,b) => a-b);
  assert.ok(locations[0] < locations[1]);
  await replace(broken.replace('control read(ref self input) exits auto;',
    'control read(ref self input) exits read_result<item>;'));
  await waitFor(() => diagnostics().length === 1 && code(diagnostics()[0]) === 'E_TRAIT_SIGNATURE', 'first trait declaration repaired');
  assert.equal(diagnostics()[0].range.start.line, locations[1]);
  assert.ok(!(await hover()).includes('Trait control contract'));
  await replace(source);
  await waitFor(async () => diagnostics().length === 0 && (await hover()) === initial, 'complete declaration repair and hover');
  await document.save();
  const result = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'trait-declaration-diagnostics')
  });
  assert.equal(result.code, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.equal(report.outcome, 'done');
  assert.deepEqual(report.events.filter(e => e.kind === 'emit').map(e => e.message), ['Hello through a trait']);
  await fs.writeFile(path.join(fixture, 'passed.txt'), 'two trait declaration errors, independent repairs, no stale hover and native trait declaration execution passed\n');
  console.log('TELLUR_TRAIT_DECLARATION_DIAGNOSTICS_EDITOR_TESTS_PASSED: trait declaration batches, independent repair, fresh hover and native execution');
};

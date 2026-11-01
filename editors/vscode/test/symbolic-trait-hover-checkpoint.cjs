const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');

exports.run = async function (root) {
  const fixture = path.join(root, 'symbolic-trait-hover');
  await fs.mkdir(fixture, {recursive:true});
  await fs.copyFile(path.join(__dirname, '../../../playground/manifest.json'), path.join(fixture, 'manifest.json'));
  const source = await fs.readFile(path.join(__dirname, '../../../playground/borrowed_traits.tellur'), 'utf8');
  const uri = vscode.Uri.file(path.join(fixture, 'borrowed_traits.tellur'));
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
    const at = document.getText().indexOf('reader<r>.peek(input)');
    assert.ok(at >= 0);
    const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, document.positionAt(at));
    return (hovers ?? []).flatMap(h => h.contents.map(c => String(c.value))).join('\n');
  }
  await waitFor(async () => diagnostics().length === 0 && (await hover()).includes('declared trait bound'), 'checked symbolic trait call');
  const initial = await hover();
  assert.ok(initial.includes('control reader<r>.peek(ref r input) exits found { ref reader<r>.t_item value from input }'), initial);
  assert.ok(initial.includes('Selected histories'), initial);
  assert.ok(!initial.includes('_path_') && !initial.includes('$0$') && !initial.includes('string'), initial);

  const declaration = 'control peek(ref self input) exits found { ref t_item value from input };';
  assert.ok(source.includes(declaration));
  await replace(source.replace(declaration, declaration.replace('from input', 'from missing')));
  await waitFor(() => diagnostics().length > 0, 'invalid trait origin');
  assert.ok(!(await hover()).includes('Trait control contract'));
  const renamed = source.replace(declaration, 'control peek(ref self owner) exits found { ref t_item value from owner };');
  await replace(renamed);
  await waitFor(async () => diagnostics().length === 0 && (await hover()).includes('peek(ref r owner)'), 'fresh trait binding names');
  assert.ok((await hover()).includes('value from owner'));
  await replace(source);
  await waitFor(async () => diagnostics().length === 0 && (await hover()) === initial, 'restored trait contract');
  await document.save();
  const result = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'symbolic-trait-hover')
  });
  assert.equal(result.code, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.equal(report.outcome, 'done');
  assert.deepEqual(report.events.filter(e => e.kind === 'emit').map(e => e.message), ['Hello through a borrowed trait']);
  await fs.writeFile(path.join(fixture, 'passed.txt'), 'symbolic associated types, declared origins, invalidation/repair, refreshed trait names and native borrowed trait execution passed\n');
  console.log('TELLUR_SYMBOLIC_TRAIT_HOVER_EDITOR_TESTS_PASSED: symbolic trait contracts, origins, edits and native execution');
};

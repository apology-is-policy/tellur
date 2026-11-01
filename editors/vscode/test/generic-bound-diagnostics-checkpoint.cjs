const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');

exports.run = async function (root) {
  const fixture = path.join(root, 'generic-bound-diagnostics');
  await fs.mkdir(fixture, {recursive:true});
  await fs.copyFile(path.join(__dirname, '../../../playground/manifest.json'), path.join(fixture, 'manifest.json'));
  // Use the existing native example with two explicit public generic interfaces.
  const example = await fs.readFile(path.join(__dirname, '../../../playground/trait_bounds.tellur'), 'utf8');
  const source = example.replace('outcome read_result', 'pub outcome read_result')
    .replace('trait reader {', 'pub trait reader {')
    .replace('control read_one<', 'pub control read_one<')
    + '\npub control read_another<r: reader>(ref r input) exits read_result<reader<r>.item> {\n    forward read_one<r>(input);\n}\n';
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
    const at = document.getText().indexOf('read_one<string>(borrow source)');
    assert.ok(at >= 0);
    const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, document.positionAt(at));
    return (hovers ?? []).flatMap(h => h.contents.map(c => String(c.value))).join('\n');
  }
  await waitFor(async () => diagnostics().length === 0 && (await hover()).includes('Control contract'), 'initial generic contract');
  const initial = await hover();
  const privateTrait = source.replace('pub trait reader', 'trait reader');
  await replace(privateTrait);
  await waitFor(() => diagnostics().length === 2 && diagnostics().every(d => code(d) === 'E_VISIBILITY'), 'two private-bound errors');
  assert.ok(!(await hover()).includes('Control contract'));
  const locations = diagnostics().map(d => d.range.start.line).sort((a,b) => a-b);
  assert.ok(locations[0] < locations[1]);
  await replace(privateTrait.replace('pub control read_one<', 'control read_one<'));
  await waitFor(() => diagnostics().length === 1 && code(diagnostics()[0]) === 'E_VISIBILITY', 'first bound boundary repaired');
  assert.equal(diagnostics()[0].range.start.line, locations[1]);
  assert.ok(!(await hover()).includes('Control contract'));
  await replace(source);
  await waitFor(async () => diagnostics().length === 0 && (await hover()) === initial, 'complete bound repair and hover');
  await document.save();
  const result = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'generic-bound-diagnostics')
  });
  assert.equal(result.code, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.equal(report.outcome, 'done');
  assert.deepEqual(report.events.filter(e => e.kind === 'emit').map(e => e.message), ['Hello through a generic trait']);
  await fs.writeFile(path.join(fixture, 'passed.txt'), 'two bound errors, independent repairs, no stale hover and native generic trait execution passed\n');
  console.log('TELLUR_GENERIC_BOUND_DIAGNOSTICS_EDITOR_TESTS_PASSED: bound batches, independent repair, fresh hover and native execution');
};

const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');

exports.run = async function (root) {
  const fixture = path.join(root, 'boundary-hover');
  await fs.mkdir(fixture, {recursive:true});
  await fs.copyFile(path.join(__dirname, '../../../playground/manifest.json'), path.join(fixture, 'manifest.json'));
  const source = await fs.readFile(path.join(__dirname, '../../../playground/contingency.tellur'), 'utf8');
  const uri = vscode.Uri.file(path.join(fixture, 'contingency.tellur'));
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
    const offset = document.getText().indexOf('request(true');
    assert.ok(offset >= 0);
    const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, document.positionAt(offset));
    return (hovers ?? []).flatMap(h => h.contents.map(c => String(c.value))).join('\n');
  }
  await waitFor(async () => diagnostics().length === 0 && (await hover()).includes('**Boundary contract**'), 'checked supervision contract');
  let text = await hover();
  assert.ok(text.includes('boundary request(bool broken, string context) retains context\n'));
  assert.ok(text.includes('no outcome, payload or fault value'));
  assert.ok(!text.includes('$contingency'));
  await replace(source.replace('string context', 'ref string context'));
  await waitFor(() => diagnostics().some(d => (typeof d.code === 'object' ? d.code.value : d.code) === 'E_BOUNDARY'), 'borrowed boundary input diagnostic');
  assert.ok(!(await hover()).includes('**Boundary contract**'));
  const renamed = source.replaceAll('context', 'request_context');
  await replace(renamed);
  await waitFor(async () => diagnostics().length === 0 && (await hover()).includes('retains request_context\n'), 'fresh retained name after repair');
  const escaping = renamed.replace('report(reason);\n    terminate;', 'report(reason);\n    escalate;');
  assert.notEqual(escaping, renamed);
  await replace(escaping);
  await waitFor(async () => diagnostics().length === 0 && (await hover()).includes('retains request_context faults\n'), 'inferred contingency escape');
  assert.ok((await hover()).includes("escape abandons the supervisor's continuation"));
  await replace(source);
  await waitFor(async () => diagnostics().length === 0 && (await hover()).includes('retains context\n'), 'restored settled boundary');
  await document.save();
  const result = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'boundary-hover')
  });
  assert.equal(result.code, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.equal(report.outcome, 'done');
  assert.deepEqual(report.events.filter(e => e.kind === 'emit').map(e => e.message), ['first request', 'request invariant failed', 'second request']);
  assert.equal(report.events.filter(e => e.kind === 'fault').length, 1);
  await fs.writeFile(path.join(fixture, 'passed.txt'), 'boundary ownership, retained names, inferred contingency escape, invalid-source suppression, fresh repair and native supervision passed\n');
  console.log('TELLUR_BOUNDARY_HOVER_EDITOR_TESTS_PASSED: owned supervision, fresh retention/escape contracts and native execution');
};

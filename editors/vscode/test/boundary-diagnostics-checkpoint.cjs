const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');

exports.run = async function (root) {
  const fixture = path.join(root, 'boundary-diagnostics');
  await fs.mkdir(fixture, {recursive:true});
  await fs.copyFile(path.join(__dirname, '../../../playground/manifest.json'), path.join(fixture, 'manifest.json'));
  const source = (await fs.readFile(path.join(__dirname, '../../../playground/contingency.tellur'), 'utf8'))
    .replace('retains context {', 'retains context {\n    u32 work_count = 1;\n    string work_text = "ready";')
    .replace('contingency(string reason) {', 'contingency(string reason) {\n    u32 report_count = 1;\n    string report_text = "handled";');
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
    const at = document.getText().indexOf('request(true');
    const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, document.positionAt(at));
    return (hovers ?? []).flatMap(h => h.contents.map(c => String(c.value))).join('\n');
  }
  async function expectErrors(tokens) {
    await waitFor(() => {
      const errors = diagnostics();
      return errors.length === tokens.length && errors.every((d, i) =>
        (typeof d.code === 'object' ? d.code.value : d.code) === 'E_TYPE' && document.getText(d.range) === tokens[i]);
    }, `diagnostic batch at ${tokens.join(', ')}`);
    assert.ok(!(await hover()).includes('Boundary contract'));
  }
  await waitFor(async () => diagnostics().length === 0 && (await hover()).includes('Boundary contract'), 'valid named supervision');
  const broken = source.replace('work_count = 1', 'work_count = true').replace('work_text = "ready"', 'work_text = 7')
    .replace('report_count = 1', 'report_count = false').replace('report_text = "handled"', 'report_text = 8');
  await replace(broken);
  await expectErrors(['false', '8']);
  const workBroken = broken.replace('report_count = false', 'report_count = 1').replace('report_text = 8', 'report_text = "handled"');
  await replace(workBroken);
  await expectErrors(['true', '7']);
  await replace(workBroken.replace('work_count = true', 'work_count = 1'));
  await expectErrors(['7']);
  await replace(source);
  await waitFor(async () => diagnostics().length === 0 && (await hover()).includes('Boundary contract'), 'fully repaired named boundary');
  await document.save();
  const result = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'boundary-diagnostics')
  });
  assert.equal(result.code, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.equal(report.outcome, 'done');
  assert.deepEqual(report.events.filter(e => e.kind === 'emit').map(e => e.message), ['first request', 'request invariant failed', 'second request']);
  assert.equal(report.events.filter(e => e.kind === 'fault').length, 1);
  await fs.writeFile(path.join(fixture, 'passed.txt'), 'handler/work batches, precise ranges, dependency suppression, partial/full repairs and native supervision passed\n');
  console.log('TELLUR_BOUNDARY_DIAGNOSTICS_EDITOR_TESTS_PASSED: independent errors, exact ranges, repairs and native execution');
};

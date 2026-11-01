const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');

exports.run = async function (root) {
  const fixture = path.join(root, 'lexical-diagnostics');
  await fs.mkdir(fixture, {recursive:true});
  await fs.copyFile(path.join(__dirname, '../../../playground/manifest.json'), path.join(fixture, 'manifest.json'));
  const source = (await fs.readFile(path.join(__dirname, '../../../playground/recovery.tellur'), 'utf8'))
    .replace('fault boundary {', 'fault boundary {\n    u32 work_count = 1;\n    string work_text = "ready";')
    .replace('contingency [context] {', 'contingency [context] {\n    u32 report_count = 1;\n    string report_text = "handled";');
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
    const at = document.getText().indexOf('auto view');
    const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, document.positionAt(at));
    return (hovers ?? []).flatMap(h => h.contents.map(c => String(c.value))).join('\n');
  }
  async function expectErrors(tokens) {
    await waitFor(() => {
      const errors = diagnostics();
      return errors.length === tokens.length && errors.every((d, i) =>
        (typeof d.code === 'object' ? d.code.value : d.code) === 'E_TYPE' && document.getText(d.range) === tokens[i]);
    }, `diagnostic batch at ${tokens.join(', ')}`);
    assert.ok(!(await hover()).includes('ref string'));
  }
  await waitFor(async () => diagnostics().length === 0 && (await hover()).includes('ref string'), 'valid lexical supervision');
  const broken = source.replace('work_count = 1', 'work_count = true').replace('work_text = "ready"', 'work_text = 7')
    .replace('report_count = 1', 'report_count = false').replace('report_text = "handled"', 'report_text = 8');
  await replace(broken);
  await expectErrors(['true', '7']);
  const handlerBroken = broken.replace('work_count = true', 'work_count = 1').replace('work_text = 7', 'work_text = "ready"');
  await replace(handlerBroken);
  await expectErrors(['false']);
  await replace(handlerBroken.replace('report_count = false', 'report_count = 1'));
  await expectErrors(['8']);
  await replace(source);
  await waitFor(async () => diagnostics().length === 0 && (await hover()).includes('ref string'), 'fully repaired lexical boundary');
  await document.save();
  const result = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'lexical-diagnostics')
  });
  assert.equal(result.code, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.equal(report.outcome, 'done');
  assert.deepEqual(report.events.filter(e => e.kind === 'emit').map(e => e.message), ['retired work', 'request failed', 'request context']);
  assert.equal(report.events.filter(e => e.kind === 'fault').length, 1);
  await fs.writeFile(path.join(fixture, 'passed.txt'), 'lexical work batches, conservative handler errors, precise ranges, fresh auto hover and native recovery passed\n');
  console.log('TELLUR_LEXICAL_DIAGNOSTICS_EDITOR_TESTS_PASSED: independent errors, exact ranges, repairs and native execution');
};

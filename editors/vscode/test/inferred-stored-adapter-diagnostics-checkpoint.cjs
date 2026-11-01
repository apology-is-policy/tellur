const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');

exports.run = async function (root) {
  const fixture = path.join(root, 'inferred-stored-adapter-diagnostics');
  await fs.mkdir(fixture, {recursive:true});
  await fs.copyFile(path.join(__dirname, '../../../playground/manifest.json'), path.join(fixture, 'manifest.json'));
  const source = await fs.readFile(path.join(__dirname, '../../../playground/stored_adapters.tellur'), 'utf8');
  const uri = vscode.Uri.file(path.join(fixture, 'stored_adapters.tellur'));
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
  async function hover(needle = 'display(message)') {
    const at = document.getText().indexOf(needle);
    assert.ok(at >= 0);
    const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, document.positionAt(at));
    return (hovers ?? []).flatMap(h => h.contents.map(c => String(c.value))).join('\n');
  }
  async function expectErrors(tokens) {
    await waitFor(() => {
      const errors = diagnostics();
      return errors.length === tokens.length && errors.every((d, i) =>
        (typeof d.code === 'object' ? d.code.value : d.code) === 'E_TYPE' && document.getText(d.range) === tokens[i]);
    }, `diagnostic batch at ${tokens.join(', ')}`);
    assert.ok(!(await hover()).includes('Control contract'));
  }
  await waitFor(async () => diagnostics().length === 0 && (await hover()).includes('Control contract'), 'valid stored adapter source');
  const initialAuto = await hover('auto saved');
  assert.ok(initialAuto.includes('ready'), initialAuto);
  const broken = source.replace('ready { clone(word) }', 'ready { concat(true, "") }')
    .replace('empty {}', 'empty { concat(false, "") }');
  assert.notEqual(broken, source);
  await replace(broken);
  await expectErrors(['true', 'false']);
  assert.equal(await hover('auto saved'), '');
  await replace(broken.replace('ready { concat(true, "") }', 'ready { clone(word) }'));
  await expectErrors(['false']);
  await replace(source);
  await waitFor(async () => diagnostics().length === 0 && (await hover()).includes('Control contract'), 'fully repaired stored adapter');
  assert.equal(await hover('auto saved'), initialAuto);
  await document.save();
  const result = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'inferred-stored-adapter-diagnostics')
  });
  assert.equal(result.code, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.equal(report.outcome, 'done');
  assert.deepEqual(report.fields, [{kind:'string',value:'hello from a stored adapter'}]);
  await fs.writeFile(path.join(fixture, 'passed.txt'), 'independent inferred labels, precise ranges, partial/full repairs, no partial result type and native inferred stored adapter passed\n');
  console.log('TELLUR_INFERRED_STORED_ADAPTER_DIAGNOSTICS_EDITOR_TESTS_PASSED: independent inferred labels, exact ranges, repairs and native execution');
};

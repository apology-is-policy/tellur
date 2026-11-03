const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');
exports.run = async function (root) {
  const fixture = path.join(root, 'boundary-handoff');
  await fs.mkdir(fixture, {recursive:true});
  await fs.copyFile(path.join(__dirname, '../../../playground/manifest.json'), path.join(fixture, 'manifest.json'));
  const example = await fs.readFile(path.join(__dirname, '../../../playground/boundary_handoff_tasks.tellur'), 'utf8');
  const split = example.indexOf('async control worker');
  assert.ok(split > 0);
  const helper = example.slice(0, split).replace('control helper<t>', 'pub control helper<t>').replace('ref t value from input } {', 'ref t value from input } faults {');
  const source = 'import "helpers.tellur" as helpers;\n' + example.slice(split).replace('delta helper<string>', 'delta helpers.helper<string>');
  const helperUri = vscode.Uri.file(path.join(fixture, 'helpers.tellur'));
  await fs.writeFile(helperUri.fsPath, helper);
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
    throw new Error(`Timed out: ${description}; ${JSON.stringify(diagnostics())}`);
  }
  async function replace(doc, text) {
    const edit = new vscode.WorkspaceEdit();
    edit.replace(doc.uri, new vscode.Range(doc.positionAt(0), doc.positionAt(doc.getText().length)), text);
    assert.ok(await vscode.workspace.applyEdit(edit));
  }
  async function hover(needle) {
    const at = document.getText().lastIndexOf(needle);
    assert.ok(at >= 0);
    const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, document.positionAt(at));
    return (hovers ?? []).flatMap(h => h.contents.map(c => String(c.value))).join('\n');
  }
  await waitFor(async () => diagnostics().length === 0 && (await hover('spawn worker')).includes('Synchronous helpers may enter named or lexical boundaries') && (await hover('helpers.helper<string>')).includes('from input'), 'helper handoff hover');
  const contract = await hover('helpers.helper<string>');
  assert.ok(contract.includes('from input') && contract.includes('string'), contract);
  await replace(document, source.replace('delta helpers.helper<string>', 'fault boundary {} contingency [] {escalate;} delta helpers.helper<string>'));
  await waitFor(() => diagnostics().some(d => code(d) === 'E_ASYNC'), 'direct boundary rejection');
  await replace(document, source.replace('[checked]: emit(value);', '[checked]: drop(parent);emit(value);'));
  await waitFor(() => diagnostics().some(d => code(d) === 'E_LOAN'), 'returned origin remains borrowed');
  await replace(document, source);
  await waitFor(() => diagnostics().length === 0, 'caller repair');
  const helperDocument = await vscode.workspace.openTextDocument(helperUri);
  await replace(helperDocument, helper.replace('ref t value from input', 'ref t value from missing'));
  await waitFor(() => diagnostics().some(d => code(d) === 'E_LIFETIME'), 'unsaved imported origin rejection');
  assert.ok(!(await hover('auto child')).includes('task<'));
  await replace(helperDocument, helper);
  await waitFor(async () => diagnostics().length === 0 && (await hover('helpers.helper<string>')) === contract, 'imported contract repair');
  await helperDocument.save();
  await document.save();
  await vscode.window.showTextDocument(document);
  const result = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'boundary-handoff')
  });
  assert.equal(result.code, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.equal(report.outcome, 'done');
  assert.deepEqual(report.events.filter(e => e.kind === 'emit').map(e => e.message), [
    'named handoff', 'child started', 'named recovery', 'lexical handoff',
    'child finished', 'parent result', 'child result'
  ]);
  await fs.writeFile(path.join(fixture, 'passed.txt'), 'helper handoffs, direct-boundary and loan rejection, unsaved imported origin repair and native execution passed\n');
  console.log('TELLUR_BOUNDARY_HANDOFF_EDITOR_TESTS_PASSED: hover, direct boundary and loan checks, imported repair and native order');
};

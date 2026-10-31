const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');

exports.run = async function (root) {
  const uri = vscode.Uri.file(path.join(root, 'owning-expressions.tellur'));
  const source = await fs.readFile(path.join(__dirname, '../../../playground/owning_expressions.tellur'), 'utf8');
  await fs.writeFile(uri.fsPath, source);
  const document = await vscode.workspace.openTextDocument(uri);
  await vscode.window.showTextDocument(document);
  await vscode.extensions.getExtension('tellur-language-local.tellur-language').activate();
  const diagnostics = () => vscode.languages.getDiagnostics(uri).filter(d => d.source === 'tellur');
  const code = d => typeof d.code === 'object' ? d.code.value : d.code;
  async function waitFor(predicate, description) {
    const end = Date.now() + 15000;
    while (Date.now() < end) {
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
  await waitFor(async () => {
    const at = document.positionAt(source.indexOf('control main') + 9);
    const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, at);
    return diagnostics().length === 0 && hovers?.length > 0;
  }, 'owning expression program checked');
  await replace(source.replace('emit(move value.caption)', 'emit(move caption)'));
  await waitFor(() => diagnostics().some(d => code(d) === 'E_MOVED'), 'use after explicit move');
  await replace(source);
  await waitFor(() => diagnostics().length === 0, 'ownership repair');
  await replace(source.replace('string caption =', 'string caption='));
  await waitFor(async () => {
    const edits = await vscode.commands.executeCommand('vscode.executeFormatDocumentProvider', uri, {tabSize:4,insertSpaces:true});
    return Array.isArray(edits) && edits.length > 0;
  }, 'formatter accepts move and temporary borrows');
  await replace(source);
  await document.save();
  const result = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'owning-expressions')
  });
  assert.equal(result.code, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.equal(report.outcome, 'done');
  assert.deepEqual(report.events.filter(e => e.kind === 'emit').map(e => e.message), ['temporary argument', 'owned image']);
  console.log('TELLUR_OWNING_EXPRESSIONS_EDITOR_TESTS_PASSED: unsaved move error/repair, formatter and native temporary/owning arguments');
};

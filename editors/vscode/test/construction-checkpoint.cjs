const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');

exports.run = async function (root) {
  const fixture = path.join(root, 'construction-checkpoint');
  await fs.mkdir(fixture, {recursive:true});
  await fs.copyFile(path.join(__dirname, '../../../playground/manifest.json'), path.join(fixture, 'manifest.json'));
  const uri = vscode.Uri.file(path.join(fixture, 'construction.tellur'));
  const source = await fs.readFile(path.join(__dirname, '../../../playground/construction.tellur'), 'utf8');
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
    const at = document.positionAt(source.indexOf('auto note'));
    const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, at);
    return diagnostics().length === 0 && hovers?.some(h => h.contents.some(c => String(c.value).includes('letter note')));
  }, 'inferred constructed value hover');
  const symbols = await vscode.commands.executeCommand('vscode.executeDocumentSymbolProvider', uri);
  assert.ok(symbols.some(s => s.name === 'greeting' && s.children.some(c => c.name === 'constructor')));
  const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, document.positionAt(source.indexOf('constructor(')));
  assert.ok(hovers.some(h => h.contents.some(c => String(c.value).includes('Raw brace construction'))));
  await replace(source.replace('[casual]: self.text = concat("Hello, ", move name);', '[casual]: emit(move name);'));
  await waitFor(() => diagnostics().some(d => code(d) === 'E_UNAVAILABLE'), 'missing constructor field on one path');
  await replace(source);
  await waitFor(() => diagnostics().length === 0, 'constructor repair');
  await replace(source.replace('self.body = move body;', 'self.body=move body;'));
  const edits = await vscode.commands.executeCommand('vscode.executeFormatDocumentProvider', uri, {tabSize:4,insertSpaces:true});
  assert.ok(Array.isArray(edits) && edits.length > 0);
  await replace(source);
  await document.save();
  const result = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'construction')
  });
  assert.equal(result.code, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.equal(report.outcome, 'done');
  assert.deepEqual(report.events.filter(e => e.kind === 'emit').map(e => e.message), [
    'Hello, Ada', 'Built from nested owning values.', 'Good day, Grace',
    'A temporary lives through this call.', 'Welcome, Tellur!'
  ]);
  await fs.writeFile(path.join(fixture, 'passed.txt'), 'constructor outline, hover, auto type, unsaved field error/repair, formatting and native nested construction passed\n');
  console.log('TELLUR_CONSTRUCTION_EDITOR_TESTS_PASSED: constructor outline/hover, auto type, unsaved field error/repair, formatting and native nested construction');
};

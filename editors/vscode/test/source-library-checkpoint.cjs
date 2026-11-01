const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');

exports.run = async function (root) {
  const fixture = path.join(root, 'source-library-checkpoint');
  for (const file of [
    'library/json_file.tellur', 'library/host/text_file.rs', 'library/host/json_string_map.rs',
    'examples/json_file/main.tellur', 'examples/json_file/manifest.json',
    'examples/json_file/host.rs', 'examples/json_file/Cargo.lock'
  ]) {
    const destination = path.join(fixture, file);
    await fs.mkdir(path.dirname(destination), {recursive:true});
    await fs.copyFile(path.join(__dirname, '../../..', file), destination);
  }
  const uri = vscode.Uri.file(path.join(fixture, 'examples/json_file/main.tellur'));
  const source = await fs.readFile(uri.fsPath, 'utf8');
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
    const position = document.positionAt(source.indexOf('json_file.load') + 'json_file.'.length);
    const hover = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, position);
    return diagnostics().length === 0 && hover?.some(h => h.contents.some(c => String(c.value).includes('Selected histories')));
  }, 'library call selection hover');
  await replace(source.replace('1048576', 'false'));
  await waitFor(() => diagnostics().some(d => code(d) === 'E_TYPE'), 'unsaved library argument error');
  await replace(source);
  await waitFor(() => diagnostics().length === 0, 'library argument repair');
  await document.save();
  const data = path.join(fixture, 'data.json');
  await fs.rm(data, {force:true});
  const output = path.join(root, 'native output', 'source-library');
  const saved = await vscode.commands.executeCommand('tellur.buildAndRun', {
    entry:'save', arguments:['--json', '--', data, ' padded ', '🙂'], output
  });
  assert.equal(saved.code, 0, saved.stderr);
  assert.equal(JSON.parse(saved.stdout).outcome, 'saved');
  assert.deepEqual(JSON.parse(await fs.readFile(data, 'utf8')), {' padded ':'🙂'});
  const loaded = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json', '--', data], output
  });
  assert.equal(loaded.code, 0, loaded.stderr);
  const report = JSON.parse(loaded.stdout);
  assert.equal(report.outcome, 'done');
  const message = report.events.find(e => e.kind === 'emit').message;
  assert.deepEqual(JSON.parse(message), {' padded ':'🙂'});
  await fs.writeFile(path.join(fixture, 'passed.txt'), 'library call selection hover, unsaved argument error and repair, shared native modules, independent save/reload and Unicode payload passed\n');
  console.log('TELLUR_SOURCE_LIBRARY_EDITOR_TESTS_PASSED: selected-history hover, unsaved repair, shared modules and native Unicode save/reload');
};

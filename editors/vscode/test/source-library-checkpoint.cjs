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
  async function hoverText() {
    const position = document.positionAt(document.getText().indexOf('json_file.load') + 'json_file.'.length);
    const hover = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, position);
    return (hover ?? []).flatMap(h => h.contents.map(c => String(c.value))).join('\n');
  }
  await waitFor(async () => {
    const text = await hoverText();
    return diagnostics().length === 0 && text.includes('Selected histories') &&
      text.includes('control json_file.load(ref string path, u32 byte_limit) exits json_file.load_result');
  }, 'library call contract and selection hover');
  await replace(source.replace('1048576', 'false'));
  await waitFor(() => diagnostics().some(d => code(d) === 'E_TYPE'), 'unsaved library argument error');
  assert.ok(!(await hoverText()).includes('Control contract'), 'broken arguments must suppress contract hover');
  await replace(source);
  await waitFor(() => diagnostics().length === 0, 'library argument repair');
  await document.save();
  const libraryUri = vscode.Uri.file(path.join(fixture, 'library/json_file.tellur'));
  const library = await vscode.workspace.openTextDocument(libraryUri);
  const librarySource = library.getText();
  async function replaceLibrary(text) {
    const edit = new vscode.WorkspaceEdit();
    edit.replace(libraryUri, new vscode.Range(library.positionAt(0), library.positionAt(library.getText().length)), text);
    assert.ok(await vscode.workspace.applyEdit(edit));
  }
  await replaceLibrary(librarySource.replaceAll('byte_limit', 'read_limit'));
  await waitFor(async () => (await hoverText()).includes('u32 read_limit'), 'unsaved imported contract refresh');
  await replaceLibrary(librarySource);
  await waitFor(async () => (await hoverText()).includes('u32 byte_limit'), 'restored imported contract');
  await library.save();
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
  await fs.writeFile(path.join(fixture, 'passed.txt'), 'library call contract and selection hover, unsaved argument error and repair, fresh imported contract edits, shared native modules, independent save/reload and Unicode payload passed\n');
  console.log('TELLUR_CALL_CONTRACT_EDITOR_TESTS_PASSED: contract and selection hover, unsaved repair and imported signature refresh, shared modules and native Unicode save/reload');
};

const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');

exports.run = async function (root) {
  const fixture = path.join(root, 'host-operations');
  await fs.mkdir(fixture, {recursive:true});
  await fs.copyFile(path.join(__dirname, '../../../playground/manifest.json'), path.join(fixture, 'manifest.json'));
  const original = 'async control next(ref handle input) exits got {bytes data}, ended {} {\n    delta await receive(input) {\n        data {bytes data} => received,\n        closed => closed,\n        failed {read_error error} => failed\n    }\n    [received]: exit got {data};\n    [closed | failed]: exit ended; // 😀\n}\ncontrol main() exits process_result {\n    delta open("input") {\n        opened {handle input} => ready,\n        not_found => absent,\n        denied => blocked\n    }\n    [absent | blocked]: exit done;\n    [ready]: delta run next(borrow input) {got {bytes data} => got, ended => ended}\n    [ready]: delta close(move input) {closed => closed, failed {close_error error} => failed}\n    [ready]: exit done;\n}\noutcome process_result {@exit 0 done}\n';
  const uri = vscode.Uri.file(path.join(fixture, 'main.tellur'));
  await fs.writeFile(uri.fsPath, original);
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
    const at = document.getText().indexOf('receive(input)');
    const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, document.positionAt(at));
    return (hovers ?? []).flatMap(h => h.contents.map(c => String(c.value))).join('\n');
  }
  await waitFor(async () => diagnostics().length === 0 && (await hover()).includes('Async host operation'), 'async host operation hover');
  const initial = await hover();
  assert.ok(initial.includes('receive(ref handle handle)') && !initial.includes('$host'), initial);
  for (const [form, message] of [
    ['delta receive(input)', 'async host operation `receive` must be invoked with await'],
    ['delta run receive(input)', 'run of async host operation `receive` is not yet admitted'],
  ]) {
    const text = original.replace('delta await receive(input)', form);
    await replace(text);
    const at = text.indexOf(form.replace('delta ', ''));
    await waitFor(() => diagnostics().length === 1 && code(diagnostics()[0]) === 'E_ASYNC' && diagnostics()[0].message.includes(message) && !diagnostics()[0].message.includes('$host') && diagnostics()[0].range.start.isEqual(document.positionAt(at)), `fresh ${form} rejection`);
    assert.ok(!(await hover()).includes('Async host operation'), 'rejected source must not expose stale hover');
  }
  await replace(original);
  await waitFor(async () => diagnostics().length === 0 && (await hover()) === initial, 'repair restores hover');
  await document.save();
  const result = await vscode.commands.executeCommand('tellur.buildAndRun', {arguments:['--json'], output:path.join(root, 'native output', 'host-operations')});
  assert.notEqual(result.code, 0);
  assert.ok(`${result.stderr}${result.stdout}`.includes('native builds do not yet support async host operation `receive`'), `${result.stderr}${result.stdout}`);
  console.log('TELLUR_HOST_OPERATIONS_EDITOR_TESTS_PASSED: async host operation hover, unsaved E_ASYNC repair, UTF-16 ranges and explicit native rejection');
};

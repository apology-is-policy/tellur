const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');

exports.run = async function () {
  const repo = path.resolve(__dirname, '../../..');
  const workspace = vscode.workspace.workspaceFolders[0].uri.fsPath;
  const example = 'examples/collection_discovery';
  for (const file of [example + '/main.vn', example + '/sequences.vn',
    example + '/manifest.json', 'library/membership.vn']) {
    const destination = path.join(workspace, file);
    await fs.mkdir(path.dirname(destination), { recursive: true });
    await fs.copyFile(path.join(repo, file), destination);
  }

  const uri = vscode.Uri.file(path.join(workspace, example, 'main.vn'));
  const document = await vscode.workspace.openTextDocument(uri);
  assert.equal(document.languageId, 'villanelle');
  const editor = await vscode.window.showTextDocument(document);
  const original = document.getText();
  await vscode.extensions.getExtension('tellur-language-local.tellur-language').activate();

  for (const [typed, label, ownership, callee, argument] of [
    ['headers.', 'contains_key', 'borrows headers', 'membership.contains_key<string>(borrow headers', 'borrow "origin"'],
    ['values.at', 'at', 'borrows values', 'sequences.at<u32>(borrow values', '0'],
    ['values.pa', 'partition', 'moves values', 'sequences.partition(values', '2'],
  ]) {
    const call = callee + ', ' + argument + ')';
    const start = original.indexOf(call);
    assert.ok(start >= 0);
    const at = document.positionAt(start);
    assert.ok(await editor.edit(edit => edit.replace(new vscode.Range(at, document.positionAt(start + call.length)), typed)));
    const cursor = document.positionAt(start + typed.length);
    editor.selection = new vscode.Selection(cursor, cursor);
    let item;
    const deadline = Date.now() + 20000;
    do {
      const list = await vscode.commands.executeCommand('vscode.executeCompletionItemProvider', uri, cursor);
      item = list?.items.find(candidate => (candidate.label.label ?? candidate.label) === label && candidate.detail?.includes(ownership));
      if (item) break;
      await new Promise(resolve => setTimeout(resolve, 100));
    } while (Date.now() < deadline);
    assert.ok(item, 'missing completion for ' + typed);
    const range = item.range.replacing ?? item.range;
    assert.equal(document.getText(range), typed);
    assert.ok(item.insertText instanceof vscode.SnippetString);
    assert.ok(await editor.insertSnippet(item.insertText, range));
    assert.ok(!editor.selection.isEmpty, 'remaining argument should be selected');
    assert.ok(await editor.edit(edit => edit.replace(editor.selection, argument)));
    await vscode.commands.executeCommand('leaveSnippet');
    assert.equal(document.getText(), original);
  }
  const output = path.join(workspace, 'build', 'collection-example');
  const result = await vscode.commands.executeCommand('villanelle.buildAndRun', { output, arguments: ['--json'] });
  assert.equal(result.code, 0, result.stderr);
  assert.equal(JSON.parse(result.stdout).outcome, 'done');
  console.log('Three collection completions applied in VS Code with explicit ownership and editable arguments.');
};

const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');

exports.run = async function (root) {
  const fixture = path.join(root, 'declaration-ranges');
  await fs.mkdir(fixture, {recursive:true});
  await fs.copyFile(path.join(__dirname, '../../../playground/manifest.json'), path.join(fixture, 'manifest.json'));
  const original = "control leaf(bool flag) exits first {delta flag {true=>exit first,false=>exit first}}\r\ncontrol middle(bool flag) exits auto {forward leaf(flag);}\r\npub control work(bool flag) exits first {forward middle(flag);}";
  const main = 'import "worker.tellur" as worker;\ncontrol main() exits process_result {string text="declaration ranges";emit(text);worker.work(true);exit done;}\noutcome process_result {@exit 0 done}\n';
  await fs.writeFile(path.join(fixture, 'main.tellur'), main);
  await fs.writeFile(path.join(fixture, 'worker.tellur'), original);
  const uri = vscode.Uri.file(path.join(fixture, 'main.tellur'));
  const document = await vscode.workspace.openTextDocument(uri);
  const dependency = await vscode.workspace.openTextDocument(vscode.Uri.file(path.join(fixture, 'worker.tellur')));
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
    edit.replace(dependency.uri, new vscode.Range(dependency.positionAt(0), dependency.positionAt(dependency.getText().length)), text);
    assert.ok(await vscode.workspace.applyEdit(edit));
  }
  async function hover() {
    const at = document.getText().indexOf('worker.work(true)');
    const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, document.positionAt(at));
    return (hovers ?? []).flatMap(h => h.contents.map(c => String(c.value))).join('\n');
  }
  await waitFor(async () => diagnostics().length === 0 && (await hover()).includes('first'), 'initial contract');
  const initial = await hover();
  for (const declared of ["exits first, // 😀\r\n second", "exits {first, second}"]) {
    const forwarded = "forward // 😀\r\n leaf(flag);";
    const text = original.replace("exits first", declared).replace("forward leaf(flag);", forwarded);
    await replace(text);
    const expected = [declared, forwarded].map(selected => {
      const at = text.indexOf(selected);
      return new vscode.Range(dependency.positionAt(at), dependency.positionAt(at + selected.length));
    });
    await waitFor(() => diagnostics().length === 1 && code(diagnostics()[0]) === 'E_OUTCOME' && ['declared','forwarded'].every((kind,i) => diagnostics()[0].relatedInformation?.some(info => info.message.includes(kind) && info.location.uri.toString() === dependency.uri.toString() && info.location.range.isEqual(expected[i]))), 'fresh authored declaration ranges');
    assert.ok(!(await hover()).includes('first'), 'failed check must not expose stale hover');
  }
  await replace(original);
  await waitFor(async () => diagnostics().length === 0 && (await hover()) === initial, 'repair restores contract');
  await dependency.save();
  const result = await vscode.commands.executeCommand('tellur.buildAndRun', {arguments:['--json'], output:path.join(root, 'native output', 'declaration-ranges')});
  assert.equal(result.code, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.equal(report.outcome, 'done');
  assert.deepEqual(report.events.filter(e => e.kind === 'emit').map(e => e.message), ['declaration ranges']);
  console.log('TELLUR_DECLARATION_RANGES_EDITOR_TESTS_PASSED: imported declaration/forwarding notes, exact unsaved UTF-16 ranges, hover recovery and native execution');
};

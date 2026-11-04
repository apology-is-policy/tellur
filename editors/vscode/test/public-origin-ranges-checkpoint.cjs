const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');

exports.run = async function (root) {
  const fixture = path.join(root, 'public-origin-ranges');
  await fs.mkdir(fixture, {recursive:true});
  await fs.copyFile(path.join(__dirname, '../../../playground/manifest.json'), path.join(fixture, 'manifest.json'));
  const original = "pub outcome reply['a] {ready {ref string value <- 'a}}\r\npub control first(ref string input) exits ready {ref string value from input} {exit ready {input};}\r\npub control second(ref string input) exits reply[input] {exit ready {input};}\r\npub control work() exits done {exit done;}";
  const main = 'import "worker.tellur" as worker;\ncontrol main() exits process_result {string text="public origin ranges";emit(text);worker.work();exit done;}\noutcome process_result {@exit 0 done}\n';
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
    const at = document.getText().indexOf('worker.work()');
    const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, document.positionAt(at));
    return (hovers ?? []).flatMap(h => h.contents.map(c => String(c.value))).join('\n');
  }
  await waitFor(async () => diagnostics().length === 0 && (await hover()).includes('done'), 'initial contract');
  const initial = await hover();
  const partial = original.replace("exits reply[input]", "exits reply // 😀\r\n ");
  const broken = partial.replace("ref string value from input", "ref // 😀\r\n string value");
  for (const [text, selections] of [
    [broken, ["ref // 😀\r\n string", "reply"]],
    [partial, ["reply"]],
  ]) {
    await replace(text);
    const expected = selections.map(selected => {
      const at = selected === "reply" ? text.indexOf("exits reply") + 6 : text.indexOf(selected);
      return new vscode.Range(dependency.positionAt(at), dependency.positionAt(at + selected.length));
    });
    await waitFor(() => diagnostics().length === expected.length && diagnostics().every((d, i) => code(d) === 'E_PUBLIC_CONTRACT' && d.relatedInformation?.some(info => info.location.uri.toString() === dependency.uri.toString() && info.location.range.isEqual(expected[i]))), 'fresh missing-origin contract ranges after partial repair');
    assert.ok(!(await hover()).includes('done'), 'partial contract repair must not expose stale hover');
  }
  await replace(original);
  await waitFor(async () => diagnostics().length === 0 && (await hover()) === initial, 'repair restores contract');
  await dependency.save();
  const result = await vscode.commands.executeCommand('tellur.buildAndRun', {arguments:['--json'], output:path.join(root, 'native output', 'public-origin-ranges')});
  assert.equal(result.code, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.equal(report.outcome, 'done');
  assert.deepEqual(report.events.filter(e => e.kind === 'emit').map(e => e.message), ['public origin ranges']);
  console.log('TELLUR_PUBLIC_ORIGIN_RANGES_EDITOR_TESTS_PASSED: imported inline and named missing-origin contracts, unsaved partial repair, UTF-16 ranges, hover recovery and native execution');
};

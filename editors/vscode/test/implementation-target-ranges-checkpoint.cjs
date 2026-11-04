const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');

exports.run = async function (root) {
  const fixture = path.join(root, 'implementation-target-ranges');
  await fs.mkdir(fixture, {recursive:true});
  await fs.copyFile(path.join(__dirname, '../../../playground/manifest.json'), path.join(fixture, 'manifest.json'));
  const original = "pub struct box<t> {t value;} pub trait reader {type item:copy;type mirror:copy;control read(self input) exits done;}\r\nimplement reader for u32 {type item=u32;type mirror=item;control read(self first) exits done {string note=\"😀\";drop(note);exit done;}} implement reader for string {type item=u32;type mirror=item;control read(self second) exits done {exit done;}}\r\npub control work() exits done {reader<u32>.read(1);reader<string>.read(\"x\");exit done;}";
  const main = 'import "worker.tellur" as worker;\ncontrol main() exits process_result {string text="implementation target ranges";emit(text);worker.work();worker.reader<u32>.read(1);exit done;}\noutcome process_result {@exit 0 done}\n';
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
    const at = document.getText().indexOf('worker.reader<u32>.read');
    const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, document.positionAt(at));
    return (hovers ?? []).flatMap(h => h.contents.map(c => String(c.value))).join('\n');
  }
  await waitFor(async () => diagnostics().length === 0 && (await hover()).includes('done'), 'initial contract');
  const initial = await hover();
  assert.ok(initial.includes('input') && !initial.includes('first'), 'hover retains trait-side parameter names');
  const partial = original.replace("for string {", "for box< // 😀\r\n u32,bool> {");
  const broken = partial.replace("for u32 {", "for ref // 😀\r\n mutable u32 {");
  for (const [text, selections] of [
    [broken, ["ref // 😀\r\n mutable u32"]],
    [partial, ["box< // 😀\r\n u32,bool>"]],
  ]) {
    await replace(text);
    const expected = selections.map(selected => {
      const at = text.indexOf(selected);
      return new vscode.Range(dependency.positionAt(at), dependency.positionAt(at + selected.length));
    });
    await waitFor(() => diagnostics().length === expected.length && diagnostics().every((d, i) => code(d) === 'E_TRAIT' && d.relatedInformation?.some(info => info.location.uri.toString() === dependency.uri.toString() && info.location.range.isEqual(expected[i]))), 'fresh implementation target ranges after partial repair');
    assert.ok(!(await hover()).includes('done'), 'partial contract repair must not expose stale hover');
  }
  await replace(original);
  await waitFor(async () => diagnostics().length === 0 && (await hover()) === initial, 'repair restores contract');
  await dependency.save();
  const result = await vscode.commands.executeCommand('tellur.buildAndRun', {arguments:['--json'], output:path.join(root, 'native output', 'implementation-target-ranges')});
  assert.equal(result.code, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.equal(report.outcome, 'done');
  assert.deepEqual(report.events.filter(e => e.kind === 'emit').map(e => e.message), ['implementation target ranges']);
  console.log('TELLUR_IMPLEMENTATION_TARGET_RANGES_EDITOR_TESTS_PASSED: independent imported implementation conformance errors, unsaved partial repair, UTF-16 ranges, hover recovery and native execution');
};

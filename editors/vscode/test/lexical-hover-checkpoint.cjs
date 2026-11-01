const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');

exports.run = async function (root) {
  const fixture = path.join(root, 'lexical-hover');
  await fs.mkdir(fixture, {recursive:true});
  await fs.copyFile(path.join(__dirname, '../../../playground/manifest.json'), path.join(fixture, 'manifest.json'));
  const source = 'struct item { u32 shadow; }\n' + await fs.readFile(path.join(__dirname, '../../../playground/trait_aliases.tellur'), 'utf8');
  const uri = vscode.Uri.file(path.join(fixture, 'aliases.tellur'));
  await fs.writeFile(uri.fsPath, source);
  const document = await vscode.workspace.openTextDocument(uri);
  await vscode.window.showTextDocument(document);
  await vscode.extensions.getExtension('tellur-language-local.tellur-language').activate();
  const diagnostics = () => vscode.languages.getDiagnostics(uri).filter(d => d.source === 'tellur');
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
  async function hover(doc, needle) {
    const offset = doc.getText().indexOf(needle);
    assert.ok(offset >= 0, needle);
    const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', doc.uri, doc.positionAt(offset));
    return (hovers ?? []).flatMap(h => h.contents.map(c => String(c.value))).join('\n');
  }
  await waitFor(async () => diagnostics().length === 0 && (await hover(document, 'reader<self>.read(owner)')).includes('ref message input'), 'lexical self contract');
  assert.ok((await hover(document, 'box<item>(move text)')).includes('constructor(string input)'));
  await replace(source.replace('type item = string;', 'type item = bytes;'));
  await waitFor(() => diagnostics().some(d => (typeof d.code === 'object' ? d.code.value : d.code) === 'E_TYPE'), 'invalid associated type edit');
  assert.ok(!(await hover(document, 'reader<self>.read(owner)')).includes('Trait control contract'));
  assert.ok(!(await hover(document, 'box<item>(move text)')).includes('Constructor contract'));
  await replace(source.replace('control read(ref self input)', 'control read(ref self fresh)'));
  await waitFor(async () => diagnostics().length === 0 && (await hover(document, 'reader<self>.read(owner)')).includes('ref message fresh'), 'fresh trait declaration after repair');
  await replace(source);
  await waitFor(async () => diagnostics().length === 0 && (await hover(document, 'reader<self>.read(owner)')).includes('ref message input'), 'restored contract');
  await document.save();
  const result = await vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--json'], output:path.join(root, 'native output', 'lexical-hover')
  });
  assert.equal(result.code, 0, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.equal(report.outcome, 'done');
  assert.deepEqual(report.events.filter(e => e.kind === 'emit').map(e => e.message), ['Hello through lexical trait aliases']);
  const aggregateUri = vscode.Uri.file(path.join(fixture, 'aggregate.tellur'));
  await fs.copyFile(path.join(__dirname, '../../../playground/aggregate_traits.tellur'), aggregateUri.fsPath);
  const aggregate = await vscode.workspace.openTextDocument(aggregateUri);
  await vscode.window.showTextDocument(aggregate);
  await waitFor(async () => (await hover(aggregate, 'prepare<text_editor>(borrow')).includes("exits preparation<string>[input, input['text]]"), 'normalized named helper contract');
  assert.ok((await hover(aggregate, 'prepare<text_editor>(borrow')).includes("ready { edit_job<string>[input, input['text]] job }"));
  await fs.writeFile(path.join(fixture, 'passed.txt'), 'lexical self/associated aliases, nominal shadowing, invalid-source suppression, fresh repair, named nested contracts and native alias example passed\n');
  console.log('TELLUR_LEXICAL_HOVER_EDITOR_TESTS_PASSED: lexical aliases, fresh repairs, named origin contracts and native execution');
};

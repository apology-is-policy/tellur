const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const vscode = require('vscode');

exports.run = async function (root) {
  const fixture = path.join(root, 'loopback-library-checkpoint');
  for (const file of [
    'library/loopback_http.tellur', 'library/host/loopback_http.rs',
    'examples/http_hello/main.tellur', 'examples/http_hello/manifest.json',
    'examples/http_hello/host.rs', 'examples/http_hello/Cargo.lock'
  ]) {
    const destination = path.join(fixture, file);
    await fs.mkdir(path.dirname(destination), {recursive:true});
    await fs.copyFile(path.join(__dirname, '../../..', file), destination);
  }
  const uri = vscode.Uri.file(path.join(fixture, 'examples/http_hello/main.tellur'));
  const document = await vscode.workspace.openTextDocument(uri);
  const source = document.getText();
  await vscode.window.showTextDocument(document);
  await vscode.extensions.getExtension('tellur-language-local.tellur-language').activate();
  const diagnostics = () => vscode.languages.getDiagnostics(uri).filter(d => d.source === 'tellur');
  const code = d => typeof d.code === 'object' ? d.code.value : d.code;
  async function waitFor(predicate, description, milliseconds = 15000) {
    const until = Date.now() + milliseconds;
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
  await waitFor(async () => {
    const position = document.positionAt(source.indexOf('http.reply_failure') + 5);
    const hover = await vscode.commands.executeCommand('vscode.executeHoverProvider', uri, position);
    const text = (hover ?? []).flatMap(h => h.contents.map(c => String(c.value))).join('\n');
    return diagnostics().length === 0 && text.includes('@contingent') &&
      text.includes('http.failure_reply_result') && text.includes('Selected histories');
  }, 'imported contingency call contract');
  const changed = source.replace('http.release(listener);', '');
  assert.notEqual(changed, source);
  await replace(changed);
  await waitFor(() => diagnostics().some(d => code(d) === 'E_RESOURCE'), 'uncompleted imported listener');
  await replace(source);
  await waitFor(() => diagnostics().length === 0, 'listener completion repair');
  await document.save();

  const reservation = require('node:net').createServer();
  await new Promise(resolve => reservation.listen(0, '127.0.0.1', resolve));
  const port = reservation.address().port;
  await new Promise(resolve => reservation.close(resolve));
  let completion;
  const run = vscode.commands.executeCommand('tellur.buildAndRun', {
    arguments:['--', String(port), '1', '3000'], output:path.join(root, 'native output', 'http-hello')
  }).then(result => { completion = result; return result; });
  await waitFor(() => {
    if (completion) throw new Error(`Service exited before response: ${JSON.stringify(completion)}`);
    return new Promise((resolve, reject) => {
      const request = require('node:http').get({host:'127.0.0.1', port, path:'/hello', agent:false}, response => {
        let body = '';
        response.setEncoding('utf8'); response.on('data', part => body += part);
        response.on('end', () => {
          try { assert.equal(response.statusCode, 200); assert.equal(body, 'Hello from Tellur 🙂'); resolve(true); }
          catch (error) { reject(error); }
        });
        response.on('error', reject);
      });
      request.setTimeout(1000, () => request.destroy());
      request.on('error', () => resolve(false));
    });
  }, 'standalone native HTTP greeting', 45000);
  const result = await run;
  assert.equal(result.code, 0, result.stderr);
  assert.ok(result.stdout.includes('completed'), result.stdout);
  await fs.writeFile(path.join(fixture, 'passed.txt'), 'imported contingent contract, missing listener release and repair, shared transport native build/run, Unicode HTTP greeting and bounded shutdown passed\n');
  console.log('TELLUR_LOOPBACK_LIBRARY_EDITOR_TESTS_PASSED: imported contingency contract, ownership repair, native HTTP greeting and bounded shutdown');
};

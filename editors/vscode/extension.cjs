const vscode = require('vscode');
const fs = require('node:fs');
const path = require('node:path');
const { LanguageClient } = require('vscode-languageclient/node');
const { configuration } = require('./configuration.cjs');
const { registerBuildCommands } = require('./build.cjs');
let client;
let fileWatcher;
let startQueue = Promise.resolve();

function serverPath(context) {
  const configured = configuration(vscode).get('serverPath', '');
  const root = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  if (configured) return path.isAbsolute(configured) ? configured : path.resolve(root || context.extensionPath, configured);
  const binary = process.platform === 'win32' ? 'tellur-lsp.exe' : 'tellur-lsp';
  const candidates = [path.join(context.extensionPath, 'server', binary), path.resolve(context.extensionPath, '../../target/debug', binary), root && path.join(root, 'target', 'debug', binary)];
  return candidates.find(p => p && fs.existsSync(p)) || binary;
}

async function start(context) {
  if (client) { const previous = client; client = undefined; await previous.dispose(); }
  fileWatcher?.dispose();
  fileWatcher = undefined;
  if (!vscode.workspace.isTrusted) return;
  const command = serverPath(context);
  const watcher = vscode.workspace.createFileSystemWatcher('**/*.{json,tellur,path}');
  fileWatcher = watcher;
  const next = new LanguageClient('tellurLanguage', 'Tellur Language', { command, args: [], options: { env: process.env } }, {
    documentSelector: ['tellur', 'path'].flatMap(language => ['file', 'untitled'].map(scheme => ({ language, scheme }))),
    initializationOptions: { manifestPath: configuration(vscode).get('manifestPath', '') },
    synchronize: { fileEvents: watcher },
  });
  client = next;
  try { await next.start(); }
  catch (error) {
    watcher.dispose();
    if (client === next) client = undefined;
    await next.dispose();
    void vscode.window.showErrorMessage(`Tellur language server could not start (${command}). Build tellur-lsp or set tellurLanguage.serverPath. ${error.message}`);
    throw error;
  }

}
function scheduleStart(context) {
  startQueue = startQueue.then(() => start(context), () => start(context));
  return startQueue;
}
async function activate(context) {
  registerBuildCommands(context);
  context.subscriptions.push(vscode.commands.registerCommand('tellur.showContractSnapshot', async () => {
    const source = vscode.window.activeTextEditor?.document;
    if (!source || !['tellur', 'path'].includes(source.languageId)) {
      void vscode.window.showWarningMessage('Open a Tellur source file to inspect its public contract.');
      return null;
    }
    await startQueue;
    if (!client) {
      void vscode.window.showWarningMessage('The Tellur language server must be running in a trusted workspace.');
      return null;
    }
    try {
      const snapshot = await client.sendRequest('tellur/contractSnapshot', { textDocument: { uri: source.uri.toString() } });
      const document = await vscode.workspace.openTextDocument({ language: 'json', content: JSON.stringify(snapshot, null, 2) + '\n' });
      await vscode.window.showTextDocument(document, { preview: false });
      return snapshot;
    } catch (error) {
      void vscode.window.showErrorMessage(`Tellur contract snapshot failed: ${error.message}`);
      return null;
    }
  }));
  for (const name of ['tellur.restartServer', 'path.restartServer']) {
    context.subscriptions.push(vscode.commands.registerCommand(name, () => scheduleStart(context)));
  }
  context.subscriptions.push(vscode.workspace.onDidGrantWorkspaceTrust(() => { void scheduleStart(context).catch(() => {}); }));
  context.subscriptions.push(vscode.workspace.onDidChangeConfiguration(event => {
    if (event.affectsConfiguration('tellurLanguage.serverPath') || event.affectsConfiguration('pathLanguage.serverPath')) {
      void scheduleStart(context).catch(() => {});
    } else if (event.affectsConfiguration('tellurLanguage') || event.affectsConfiguration('pathLanguage')) {
      void startQueue.then(() => client?.sendNotification('workspace/didChangeConfiguration', {
        settings: { tellurLanguage: { manifestPath: configuration(vscode).get('manifestPath', '') } },
      })).catch(() => {});
    }
  }));
  await scheduleStart(context);
}
async function deactivate() { await startQueue.catch(() => {}); if (client) await client.dispose(); fileWatcher?.dispose(); }
module.exports = { activate, deactivate };

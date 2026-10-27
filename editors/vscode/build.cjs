const vscode = require('vscode');
const { configuration } = require('./configuration.cjs');
const fs = require('node:fs');
const path = require('node:path');
const { spawn } = require('node:child_process');
function registerBuildCommands(context) {
  const output = vscode.window.createOutputChannel('Tellur Build');
  context.subscriptions.push(output);
  let busy = false;
  let active;
  context.subscriptions.push({ dispose: () => active?.kill() });
  function compiler(root, config) {
    const configured = config.get('compilerPath', '');
    if (configured) return path.isAbsolute(configured) ? configured : path.resolve(root, configured);
    const name = process.platform === 'win32' ? 'tellurc.exe' : 'tellurc';
    return [path.join(context.extensionPath, 'server', name), path.join(root, 'target', 'debug', name)].find(p => fs.existsSync(p)) || name;
  }
  function manifest(source, root, config) {
    const configured = config.get('manifestPath', '');
    if (configured) return path.resolve(root, configured);
    let dir = path.dirname(source);
    while (true) {
      const candidate = path.join(dir, 'manifest.json');
      if (fs.existsSync(candidate)) return candidate;
      if (dir === root || path.dirname(dir) === dir) break;
      dir = path.dirname(dir);
    }
    throw new Error('No manifest.json found. Set tellurLanguage.manifestPath or add one beside your source.');
  }
  async function execute(run, overrides = {}) {
    if (!vscode.workspace.isTrusted) throw new Error('Trust the workspace before building or running Tellur programs.');
    if (busy) throw new Error('A Tellur build is already running.');
    const document = vscode.window.activeTextEditor?.document;
    if (!document || !['tellur', 'path'].includes(document.languageId) || document.isUntitled) throw new Error('Open a saved .tellur or .path file first.');
    if (!(await document.save())) throw new Error('The source file could not be saved.');
    const root = vscode.workspace.getWorkspaceFolder(document.uri)?.uri.fsPath || path.dirname(document.uri.fsPath);
    const config = configuration(vscode, document.uri);
    const entry = overrides.entry || config.get('entryRegion', 'main');
    const programArgs = overrides.arguments || config.get('programArguments', []);
    if (!Array.isArray(programArgs) || programArgs.some(a => typeof a !== 'string')) throw new Error('programArguments must be an array of strings.');
    const buildFile = overrides.output || path.join(path.dirname(document.uri.fsPath), 'build', path.basename(document.uri.fsPath, path.extname(document.uri.fsPath)) + (process.platform === 'win32' ? '.exe' : ''));
    const args = [run ? 'exec' : 'build', document.uri.fsPath, manifest(document.uri.fsPath, root, config), '--entry', entry, '--out', buildFile];
    if (config.get('releaseBuild', false)) args.push('--release');
    if (run) args.push('--', ...programArgs);
    output.clear(); output.show(true);
    output.appendLine(`${run ? 'Building and running' : 'Building'} ${path.basename(document.uri.fsPath)} · entry ${entry}`);
    const command = compiler(root, config);
    busy = true;
    try {
      return await new Promise((resolve, reject) => {
        let stdout = '', stderr = '';
        const child = spawn(command, args, { cwd: root, env: process.env, shell: false });
        active = child;
        child.stdout.on('data', data => { stdout += data; output.append(data.toString()); });
        child.stderr.on('data', data => { stderr += data; output.append(data.toString()); });
        child.on('error', error => reject(new Error(`Could not start tellurc: ${error.message}. Set tellurLanguage.compilerPath.`)));
        child.on('close', code => {
          output.appendLine(`\n${code === 0 ? 'Finished' : 'Failed'} (exit ${code})`);
          resolve({ code, stdout, stderr, executable: buildFile });
        });
      });
    } finally { busy = false; active = undefined; }
  }
  for (const [name, run] of [['tellur.build', false], ['tellur.buildAndRun', true], ['path.build', false], ['path.buildAndRun', true]]) {
    context.subscriptions.push(vscode.commands.registerCommand(name, async options => {
      try { return await execute(run, options); }
      catch (error) { output.appendLine(error.message); output.show(true); void vscode.window.showErrorMessage(error.message); throw error; }
    }));
  }
}
module.exports = { registerBuildCommands };

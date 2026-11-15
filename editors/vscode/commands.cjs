const fs = require('node:fs');
const path = require('node:path');

function resolveCommand(extensionPath, root, configured, server) {
  if (configured) return { command: path.resolve(root || extensionPath, configured), args: [] };

  const suffix = process.platform === 'win32' ? '.exe' : '';
  const unified = { name: 'tellur' + suffix, args: server ? ['lsp'] : [] };
  const legacy = { name: (server ? 'tellur-lsp' : 'tellurc') + suffix, args: [] };
  const directories = [path.join(extensionPath, 'server')];
  if (server) directories.push(path.resolve(extensionPath, '../../target/debug'));
  if (root) directories.push(path.join(root, 'target', 'debug'));
  directories.push(...(process.env.PATH || '').split(path.delimiter).filter(Boolean));

  for (const directory of directories) {
    for (const { name, args } of [unified, legacy]) {
      const command = path.resolve(directory, name);
      try {
        fs.accessSync(command, fs.constants.X_OK);
        if (fs.statSync(command).isFile()) return { command, args };
      } catch { /* Continue looking for an executable. */ }
    }
  }
  return { command: unified.name, args: unified.args };
}

module.exports = { resolveCommand };

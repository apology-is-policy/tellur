const { test } = require('node:test');
const assert = require('node:assert/strict');
const { configuration } = require('../configuration.cjs');
function config(current, legacy) {
  return configuration({ workspace: { getConfiguration: name => ({
    inspect: key => (name === 'villanelleLanguage' ? current : legacy)[key],
    get: (_, fallback) => fallback,
  }) } });
}
test('legacy workspace settings survive defaults and canonical explicit values win', () => {
  const legacy = { compilerPath: { workspaceValue: '/old/pathc' }, releaseBuild: { globalValue: true } };
  assert.equal(config({}, legacy).get('compilerPath', ''), '/old/pathc');
  assert.equal(config({}, legacy).get('releaseBuild', false), true);
  assert.equal(config({ compilerPath: { globalValue: '/new/tellurc' } }, legacy).get('compilerPath', ''), '/new/tellurc');
  assert.equal(config({ compilerPath: { workspaceValue: '' } }, legacy).get('compilerPath', 'fallback'), '');
  assert.equal(config({ releaseBuild: { workspaceValue: false } }, legacy).get('releaseBuild', true), false);
  assert.equal(config({}, {}).get('entryRegion', 'main'), 'main');
});
test('more specific configuration levels take precedence within a namespace', () => {
  const configValue = { compilerPath: { globalValue: '/global', workspaceValue: '/workspace', workspaceFolderValue: '/folder', workspaceFolderLanguageValue: '/language' } };
  assert.equal(config(configValue, {}).get('compilerPath', ''), '/language');
});

test('one bundled compiler serves both roles and configured standalone commands keep their arguments', t => {
  const fs = require('node:fs');
  const path = require('node:path');
  const os = require('node:os');
  const { resolveCommand } = require('../commands.cjs');
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'tellur-commands-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const server = path.join(root, 'server');
  fs.mkdirSync(server);
  const suffix = process.platform === 'win32' ? '.exe' : '';
  const legacy = path.join(server, 'tellur-lsp' + suffix);
  fs.writeFileSync(legacy, '', { mode: 0o755 });
  assert.deepEqual(resolveCommand(root, root, '', true), { command: legacy, args: [] });
  const command = path.join(server, 'vnc' + suffix);
  fs.writeFileSync(command, '', { mode: 0o755 });
  assert.deepEqual(resolveCommand(root, root, '', true), { command, args: ['lsp'] });
  assert.deepEqual(resolveCommand(root, root, '', false), { command, args: [] });
  assert.deepEqual(resolveCommand(root, root, 'my-server', true), { command: path.join(root, 'my-server'), args: [] });
});

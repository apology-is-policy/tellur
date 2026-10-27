const { test } = require('node:test');
const assert = require('node:assert/strict');
const { configuration } = require('../configuration.cjs');
function config(current, legacy) {
  return configuration({ workspace: { getConfiguration: name => ({
    inspect: key => (name === 'tellurLanguage' ? current : legacy)[key],
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

// A canonical setting wins when explicitly set; otherwise honor existing workspaces.
function configuration(vscode, resource) {
  const current = vscode.workspace.getConfiguration('tellurLanguage', resource);
  const legacy = vscode.workspace.getConfiguration('pathLanguage', resource);
  const levels = ['workspaceFolderLanguageValue', 'workspaceFolderValue', 'workspaceLanguageValue', 'workspaceValue', 'globalLanguageValue', 'globalValue'];
  return {
    get(key, fallback) {
      for (const config of [current, legacy]) {
        const inspected = config.inspect(key);
        for (const level of levels) {
          if (inspected?.[level] !== undefined) return inspected[level];
        }
      }
      return current.get(key, fallback);
    },
  };
}
module.exports = { configuration };

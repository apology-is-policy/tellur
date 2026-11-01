const vscode = require('vscode');
exports.run = async function () {
  try {
    await require('./constructor-access-checkpoint.cjs').run(vscode.workspace.workspaceFolders[0].uri.fsPath);
  } finally {
    await vscode.commands.executeCommand('workbench.action.closeAllEditors');
  }
};

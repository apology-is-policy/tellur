const vscode = require('vscode');
exports.run = async function () {
  try {
    await require('./implementation-trait-ranges-checkpoint.cjs').run(vscode.workspace.workspaceFolders[0].uri.fsPath);
    await require('./trait-member-duplicate-ranges-checkpoint.cjs').run(vscode.workspace.workspaceFolders[0].uri.fsPath);
  } finally {
    await vscode.commands.executeCommand('workbench.action.closeAllEditors');
  }
};

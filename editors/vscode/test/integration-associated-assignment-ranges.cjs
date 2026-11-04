const vscode = require('vscode');
exports.run = async function () {
  try {
    await require('./associated-normalization-batches-checkpoint.cjs').run(vscode.workspace.workspaceFolders[0].uri.fsPath);
    await require('./implementation-conformance-batches-checkpoint.cjs').run(vscode.workspace.workspaceFolders[0].uri.fsPath);
    await require('./associated-assignment-ranges-checkpoint.cjs').run(vscode.workspace.workspaceFolders[0].uri.fsPath);
  } finally {
    await vscode.commands.executeCommand('workbench.action.closeAllEditors');
  }
};

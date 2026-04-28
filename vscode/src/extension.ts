import * as vscode from 'vscode';

export function activate(context: vscode.ExtensionContext) {
  console.log('Congratulations, your extension "agentenv" is now active!');

  let disposable = vscode.commands.registerCommand('agentenv.sync', () => {
    vscode.window.showInformationMessage('Syncing agentenv plugins...');
    // TODO: Implement sync functionality
  });

  context.subscriptions.push(disposable);

  let doctorDisposable = vscode.commands.registerCommand('agentenv.doctor', () => {
    vscode.window.showInformationMessage('Running agentenv doctor...');
    // TODO: Implement doctor functionality
  });

  context.subscriptions.push(doctorDisposable);
}

export function deactivate() {}

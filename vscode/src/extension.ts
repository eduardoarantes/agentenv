import * as vscode from 'vscode';
import {
  CONFIG_FILENAME,
  configFileExists,
  disposeOutputChannel,
  getOutputChannel,
  getPrimaryWorkspaceFolder,
  handleSpawnError,
  runAgentenv,
} from './agentenv';

export function activate(context: vscode.ExtensionContext) {
  context.subscriptions.push({ dispose: disposeOutputChannel });

  context.subscriptions.push(
    vscode.commands.registerCommand('agentenv.sync', () => runSyncCommand()),
    vscode.commands.registerCommand('agentenv.doctor', () => runDoctorCommand()),
    vscode.commands.registerCommand('agentenv.openConfig', () => runOpenConfigCommand()),
    vscode.commands.registerCommand('agentenv.listPlugins', () => runListPluginsCommand()),
    vscode.commands.registerCommand('agentenv.clean', () => runCleanCommand())
  );

  setupConfigWatcher(context);
  void maybeRunStartupSync();
}

export function deactivate() {}

const DEFAULT_CONFIG_CHANGE_DEBOUNCE_MS = 1500;

function getConfigChangeDebounceMs(): number {
  const raw = vscode.workspace
    .getConfiguration('agentenv')
    .get<number>('configChangeDebounceMs', DEFAULT_CONFIG_CHANGE_DEBOUNCE_MS);
  if (!Number.isFinite(raw) || raw < 0) {
    return DEFAULT_CONFIG_CHANGE_DEBOUNCE_MS;
  }
  return raw;
}

function setupConfigWatcher(context: vscode.ExtensionContext): void {
  const folder = getPrimaryWorkspaceFolder();
  if (!folder) {
    return;
  }
  if (!vscode.workspace.getConfiguration('agentenv').get<boolean>('syncOnConfigChange', true)) {
    return;
  }

  const pattern = new vscode.RelativePattern(folder, CONFIG_FILENAME);
  const watcher = vscode.workspace.createFileSystemWatcher(pattern);

  let debounceTimer: NodeJS.Timeout | undefined;
  let syncing = false;
  let pending = false;

  const runIfQuiet = async () => {
    if (syncing) {
      pending = true;
      return;
    }
    syncing = true;
    try {
      do {
        pending = false;
        await runSyncCommand({ silentSuccess: true });
      } while (pending);
    } finally {
      syncing = false;
    }
  };

  const trigger = () => {
    if (debounceTimer) {
      clearTimeout(debounceTimer);
    }
    debounceTimer = setTimeout(() => {
      debounceTimer = undefined;
      void runIfQuiet();
    }, getConfigChangeDebounceMs());
  };

  context.subscriptions.push(
    watcher,
    watcher.onDidChange(trigger),
    watcher.onDidCreate(trigger),
    {
      dispose: () => {
        if (debounceTimer) {
          clearTimeout(debounceTimer);
        }
      },
    }
  );
}

async function maybeRunStartupSync(): Promise<void> {
  const folder = getPrimaryWorkspaceFolder();
  if (!folder) {
    return;
  }
  if (!(await configFileExists(folder))) {
    // No `.agentrc.yaml` here — the user hasn't expressed intent to use
    // agentenv in this workspace, so stay silent (don't probe, don't sync).
    return;
  }

  const syncOnOpen = vscode.workspace
    .getConfiguration('agentenv')
    .get<boolean>('syncOnOpen', true);
  if (syncOnOpen) {
    // A startup sync surfaces missing-CLI errors itself via handleSpawnError.
    await runSyncCommand({ silentSuccess: true });
    return;
  }

  // `syncOnOpen` is off but the workspace clearly intends to use agentenv
  // (`.agentrc.yaml` is present). Probe the CLI once so the user finds out
  // up front rather than the first time they invoke a command.
  await probeAgentenvCli();
}

/**
 * One-shot probe: spawn `agentenv --version` and route ENOENT through the
 * shared `handleSpawnError` dialog. Any non-ENOENT failure (e.g. a broken
 * install that exits non-zero) is intentionally ignored — we only want to
 * alert when the binary itself is missing.
 */
async function probeAgentenvCli(): Promise<void> {
  try {
    await runAgentenv(['--version']);
  } catch (err) {
    await handleSpawnError(err);
  }
}

interface SyncCommandOptions {
  silentSuccess?: boolean;
}

async function runSyncCommand(opts: SyncCommandOptions = {}): Promise<void> {
  const config = vscode.workspace.getConfiguration('agentenv');
  const args = ['sync'];
  if (config.get<boolean>('refetchOnSync', false)) {
    args.push('--refetch');
  }

  await vscode.window.withProgress(
    { location: vscode.ProgressLocation.Window, title: 'agentenv: syncing…' },
    async () => {
      try {
        const result = await runAgentenv(args);
        if (result.code === 0) {
          if (!opts.silentSuccess) {
            vscode.window.showInformationMessage('agentenv: sync complete');
          }
        } else {
          const choice = await vscode.window.showWarningMessage(
            'agentenv: sync finished with errors',
            'Show Output'
          );
          if (choice === 'Show Output') {
            getOutputChannel().show(true);
          }
        }
      } catch (err) {
        await handleSpawnError(err);
      }
    }
  );
}

async function runDoctorCommand(): Promise<void> {
  try {
    const result = await runAgentenv(['doctor'], { revealOutput: true });
    if (result.code === 0) {
      vscode.window.showInformationMessage('agentenv: doctor reports no issues');
    } else {
      vscode.window.showWarningMessage('agentenv: doctor reported issues — see output');
    }
  } catch (err) {
    await handleSpawnError(err);
  }
}

async function runOpenConfigCommand(): Promise<void> {
  const folder = getPrimaryWorkspaceFolder();
  if (!folder) {
    vscode.window.showWarningMessage('agentenv: open a workspace folder first');
    return;
  }
  const uri = vscode.Uri.joinPath(folder.uri, CONFIG_FILENAME);
  if (await configFileExists(folder)) {
    const doc = await vscode.workspace.openTextDocument(uri);
    await vscode.window.showTextDocument(doc);
    return;
  }

  const choice = await vscode.window.showInformationMessage(
    `No ${CONFIG_FILENAME} found. Initialize it now?`,
    'Run agentenv init',
    'Cancel'
  );
  if (choice !== 'Run agentenv init') {
    return;
  }
  try {
    const result = await runAgentenv(['init']);
    if (result.code === 0 && (await configFileExists(folder))) {
      const doc = await vscode.workspace.openTextDocument(uri);
      await vscode.window.showTextDocument(doc);
    } else {
      vscode.window.showWarningMessage('agentenv: init did not produce a config — see output');
      getOutputChannel().show(true);
    }
  } catch (err) {
    await handleSpawnError(err);
  }
}

async function runListPluginsCommand(): Promise<void> {
  try {
    await runAgentenv(['list'], { revealOutput: true });
  } catch (err) {
    await handleSpawnError(err);
  }
}

async function runCleanCommand(): Promise<void> {
  const confirm = await vscode.window.showWarningMessage(
    'Remove all agentenv-managed links recorded in .agentenv/state.json?',
    { modal: true, detail: 'Only links agentenv created will be removed. Unmanaged files are left alone.' },
    'Clean'
  );
  if (confirm !== 'Clean') {
    return;
  }
  try {
    const result = await runAgentenv(['clean'], { revealOutput: true });
    if (result.code === 0) {
      vscode.window.showInformationMessage('agentenv: clean complete');
    } else {
      vscode.window.showWarningMessage('agentenv: clean reported issues — see output');
    }
  } catch (err) {
    await handleSpawnError(err);
  }
}

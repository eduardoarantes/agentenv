import * as vscode from 'vscode';
import { execFile } from 'child_process';

export const CONFIG_FILENAME = '.agentrc.yaml';
export const OUTPUT_CHANNEL_NAME = 'Agentenv';

export interface RunResult {
  code: number;
  stdout: string;
  stderr: string;
}

export interface RunOptions {
  /** Project root passed via `--project`. Defaults to first workspace folder. */
  cwd?: string;
  /** When true, reveal the output channel before streaming. */
  revealOutput?: boolean;
}

let outputChannel: vscode.OutputChannel | undefined;

export function getOutputChannel(): vscode.OutputChannel {
  if (!outputChannel) {
    outputChannel = vscode.window.createOutputChannel(OUTPUT_CHANNEL_NAME);
  }
  return outputChannel;
}

export function disposeOutputChannel(): void {
  outputChannel?.dispose();
  outputChannel = undefined;
}

export function getConfiguredBinary(): string {
  return vscode.workspace.getConfiguration('agentenv').get<string>('path', 'agentenv');
}

export function getPrimaryWorkspaceFolder(): vscode.WorkspaceFolder | undefined {
  const folders = vscode.workspace.workspaceFolders;
  if (!folders || folders.length === 0) {
    return undefined;
  }
  if (folders.length > 1) {
    getOutputChannel().appendLine(
      `[agentenv] multi-root workspace detected; using first folder: ${folders[0].uri.fsPath}`
    );
  }
  return folders[0];
}

// eslint-disable-next-line no-control-regex -- intentionally matches ESC byte to strip ANSI color codes from CLI output
const ANSI_PATTERN = /\x1b\[[0-9;]*m/g;

function stripAnsi(text: string): string {
  return text.replace(ANSI_PATTERN, '');
}

/**
 * Spawn the agentenv CLI with the given subcommand args. Streams stdout/stderr
 * (ANSI-stripped) to the shared "Agentenv" output channel.
 *
 * Throws when the binary is missing — caller should catch and present the
 * "Open settings" recovery action via `handleSpawnError`.
 */
export function runAgentenv(args: string[], opts: RunOptions = {}): Promise<RunResult> {
  const channel = getOutputChannel();
  const folder = opts.cwd ? undefined : getPrimaryWorkspaceFolder();
  const cwd = opts.cwd ?? folder?.uri.fsPath;
  const binary = getConfiguredBinary();
  const fullArgs = cwd ? ['--project', cwd, ...args] : args;

  if (opts.revealOutput) {
    channel.show(true);
  }
  channel.appendLine(`$ ${binary} ${fullArgs.join(' ')}`);

  return new Promise((resolve, reject) => {
    const child = execFile(
      binary,
      fullArgs,
      { cwd, maxBuffer: 10 * 1024 * 1024 },
      (error, stdout, stderr) => {
        const cleanStdout = stripAnsi(stdout);
        const cleanStderr = stripAnsi(stderr);
        if (cleanStdout) {
          channel.append(cleanStdout);
          if (!cleanStdout.endsWith('\n')) {
            channel.append('\n');
          }
        }
        if (cleanStderr) {
          channel.append(cleanStderr);
          if (!cleanStderr.endsWith('\n')) {
            channel.append('\n');
          }
        }

        if (error && (error as NodeJS.ErrnoException).code === 'ENOENT') {
          channel.appendLine(`[agentenv] binary not found: ${binary}`);
          reject(error);
          return;
        }

        const code = child.exitCode ?? (error ? 1 : 0);
        channel.appendLine(`[agentenv] exit ${code}`);
        resolve({ code, stdout: cleanStdout, stderr: cleanStderr });
      }
    );
  });
}

/**
 * Present a user-facing error for spawn failures. Returns true if the error was
 * an ENOENT (binary missing), so callers can short-circuit further messaging.
 */
export async function handleSpawnError(err: unknown): Promise<boolean> {
  const errno = err as NodeJS.ErrnoException | undefined;
  if (errno?.code === 'ENOENT') {
    const binary = getConfiguredBinary();
    const choice = await vscode.window.showErrorMessage(
      `Could not find the agentenv binary "${binary}". Install it or set "agentenv.path".`,
      'Open Settings',
      'Show Output'
    );
    if (choice === 'Open Settings') {
      await vscode.commands.executeCommand('workbench.action.openSettings', 'agentenv.path');
    } else if (choice === 'Show Output') {
      getOutputChannel().show(true);
    }
    return true;
  }
  const message = err instanceof Error ? err.message : String(err);
  await vscode.window.showErrorMessage(`agentenv failed to run: ${message}`);
  return false;
}

export async function configFileExists(folder: vscode.WorkspaceFolder): Promise<boolean> {
  const uri = vscode.Uri.joinPath(folder.uri, CONFIG_FILENAME);
  try {
    await vscode.workspace.fs.stat(uri);
    return true;
  } catch {
    return false;
  }
}

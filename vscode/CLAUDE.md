# VS Code Extension Development Guide

## Overview

This is the VS Code extension for agentenv. It provides UI integration, command palettes, and IDE-level controls for managing agentenv in VS Code.

## Architecture

### Extension Structure
- `src/extension.ts` — Main entry point; handles activation, command registration, and file watching
- Commands implemented:
  - `agentenv.sync` — Run marketplace sync
  - `agentenv.doctor` — Run diagnostics
  - `agentenv.openConfig` — Open .agentrc.yaml in editor
  - `agentenv.listPlugins` — Show installed plugins
  - `agentenv.clean` — Clean managed symlinks

### Configuration
Settings are defined in `package.json` under `contributes.configuration`:
- `agentenv.path` — Path to the agentenv binary (defaults to looking in $PATH)
- `agentenv.syncOnOpen` — Auto-sync when workspace opens (default: true)
- `agentenv.syncOnConfigChange` — Auto-sync when .agentrc.yaml changes (default: true)
- `agentenv.configChangeDebounceMs` — Debounce delay for file watch (default: 1500ms)
- `agentenv.refetchOnSync` — Force marketplace refetch (default: false)

### File Watching
The extension watches `.agentrc.yaml` changes and debounces sync invocations:
- Debounce logic prevents thrashing when users rapidly edit config
- Configurable via `agentenv.configChangeDebounceMs`

## Development Workflow

### Build & Test
```bash
npm install            # Install dependencies
npm run compile        # Build TypeScript
npm run lint          # ESLint checks
npm run format        # Prettier formatting
npm run esbuild-watch # Watch mode during development
```

### Debugging
Press F5 to launch the debug instance. Console logs appear in the Debug Console.

### Release
```bash
npm run vscode:prepublish  # Minify for production
```

## Key Dependencies
- `@vscode/*` — VS Code API (types and runtime)
- `esbuild` — Fast bundler
- `eslint` + `prettier` — Code quality and formatting
- `typescript` — Language and type checking

## Extension Lifecycle

1. **Activation** — Triggered on `onStartupFinished` event (after all extensions load)
2. **Command Registration** — All 5 commands registered in `activate()`
3. **File Watching** — If `.agentrc.yaml` exists, watch it for changes
4. **Auto-sync** — On config change (debounced) or workspace open (if enabled)
5. **Deactivation** — Cleanup happens automatically (watchers, etc.)

## Common Tasks

### Add a new command
1. Add entry to `contributes.commands` in `package.json`
2. Register handler in `activate()` with `vscode.commands.registerCommand()`
3. Implement the handler function

### Add a new setting
1. Add property to `contributes.configuration.properties` in `package.json`
2. Read via `vscode.workspace.getConfiguration('agentenv').get('setting')`

### Improve diagnostics
The `agentenv doctor` command should report:
- Whether the agentenv binary is found
- Whether .agentrc.yaml is valid YAML
- Current marketplace status
- Plugin count and status

## Testing in VS Code

1. Run `npm run compile` to build
2. Press F5 to launch a debug instance
3. Open a folder with `.agentrc.yaml`
4. Test commands via Command Palette (Ctrl+Shift+P)
5. Check output in the Debug Console

## Notes for AI Assistants

- TypeScript strict mode enabled
- Prefer async/await over callbacks
- Always handle errors gracefully (show user-facing errors)
- Use workspace settings API, not user settings, for project-scoped config
- The agentenv CLI is the source of truth; the extension is thin wrapper

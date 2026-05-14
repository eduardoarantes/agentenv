#!/usr/bin/env bash
# PostToolUse(Write|Edit) hook: format + lint files edited by Claude Code.
#   - *.rs                 -> rustfmt (via rustup)
#   - vscode/**/*.ts       -> prettier --write && eslint --fix
# Silent on failure: the agent's turn is never blocked.

set +e
f=$(/usr/bin/jq -r '.tool_response.filePath // .tool_input.file_path // empty')
[ -z "$f" ] && exit 0
[ ! -f "$f" ] && exit 0

case "$f" in
  *.rs)
    if command -v rustup >/dev/null 2>&1; then
      rustup run stable rustfmt "$f" >/dev/null 2>&1
    elif command -v rustfmt >/dev/null 2>&1; then
      rustfmt "$f" >/dev/null 2>&1
    fi
    ;;
  *.ts)
    case "$f" in
      "$CLAUDE_PROJECT_DIR"/vscode/*)
        cd "$CLAUDE_PROJECT_DIR/vscode" || exit 0
        ./node_modules/.bin/prettier --write "$f" >/dev/null 2>&1
        ./node_modules/.bin/eslint --fix "$f" >/dev/null 2>&1
        ;;
    esac
    ;;
esac

exit 0

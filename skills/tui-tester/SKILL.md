---
name: tui-tester
description: Use this skill when a user asks to test a terminal UI (TUI) through tmux, including rendering/layout checks, keyboard interactions, navigation flows, and collecting reproducible evidence from pane captures.
---

# TUI Tester

Use this skill to exercise TUIs in a real terminal session via `tmux` and validate behavior end-to-end.

## Scope

This skill is for:
- Launching and controlling a TUI in tmux.
- Testing rendering and layout at runtime.
- Exercising keyboard navigation and interactions.
- Verifying command hints/help text shown in the UI.
- Capturing evidence (pane snapshots) for what happened.

This skill is not for:
- Pixel-perfect visual QA.
- Screenshot/image diff pipelines.
- Editing the app as the primary task.

## Required Approach

1. Always run the TUI in `tmux`, never by describing expected behavior only.
2. Drive interactions with `tmux send-keys` and verify each step with `tmux capture-pane`.
3. Keep sessions deterministic:
- Use a named tmux session.
- Use one window/pane unless the user asks for more.
- Capture output after every meaningful action.
4. Clean up at the end by killing the tmux session unless the user asks to keep it alive.

## Preflight Checklist

Before sending keys:
1. Confirm the launch command (for example `just run`, `cargo run`, `npm run dev`, binary path).
2. Start a new detached tmux session:
```bash
tmux new-session -d -s <session_name> 'cd <repo> && <launch_command>'
```
3. Record pane PID and process group early, before cleanup:
```bash
PANE_PID="$(tmux list-panes -t <session_name> -F '#{pane_pid}')"
PANE_PGID="$(ps -o pgid= -p \"$PANE_PID\" | tr -d ' ')"
```
4. Poll until the UI is actually visible:
```bash
tmux capture-pane -pt <session_name> | tail -n <N>
```
5. If startup failed, capture stderr/output and report the exact failure.

## Interaction Loop

Repeat this loop for each test objective:

1. State the next action briefly.
2. Send one interaction step (or a short, explicit key sequence):
```bash
tmux send-keys -t <session_name> <keys...>
```
3. Wait briefly when needed (`sleep 0.2` to `sleep 1` depending on app latency).
4. Capture pane and inspect for expected state changes.
5. Record what changed and whether it matched expectation.

Do not chain long blind key sequences without intermediate captures.

## What to Validate

### Rendering
- Main frame appears (title/header/body/footer).
- No obvious garbled output or missing redraws.
- Loading and error states render intelligibly.

### Layout
- Panels split as intended when toggles/modals/previews are opened.
- Footer/help area remains readable and wrapped correctly.
- Long lines/large tables behave with clipping/scrolling instead of corrupting layout.

### Interaction
- Core navigation keys work (`Up/Down`, `Left/Right`, `Enter`, `Esc`, app-specific keys).
- Mode transitions are correct (browse -> modal -> close, preview toggle, search mode, etc.).
- Key hints shown to users match actual behavior.

### Stability
- No crashes/panics during interactive use.
- Repeated open/close/toggle actions remain consistent.

## Evidence Collection

For each major checkpoint, keep a capture snippet showing:
- Current location/screen title.
- Selected item/active panel.
- Footer command hints.

Useful commands:
```bash
tmux capture-pane -pt <session_name> | tail -n 80
tmux capture-pane -pt <session_name> -S -200
```

When reporting results, include concrete observations from captures, not just conclusions.

## Recommended Test Script (Generic)

Run this flow unless user gives a different scenario:
1. App launch and initial render check.
2. Basic navigation up/down and enter/back.
3. Open primary feature view (preview/details/modal/etc.).
4. Exercise in-view interactions (scroll, toggle, tabs/modes).
5. Confirm footer hints include relevant commands.
6. Exit view and return to baseline screen.
7. Repeat one action sequence to confirm consistency.

## Failure Handling

If behavior diverges:
1. Capture current pane immediately.
2. Try one minimal recovery action (`Esc`, refresh key, back key).
3. Capture again.
4. Report: expected vs actual, exact key sequence, and capture context.

If tmux command fails due to permissions:
- Retry with escalated permissions where allowed.
- Continue only after access is granted.

## Cleanup

Unless asked to keep it running:
1. Attempt graceful app exit first (prefer app quit key, then `Ctrl-C`):
```bash
tmux send-keys -t <session_name> q
sleep 0.5
tmux send-keys -t <session_name> C-c
sleep 0.5
```
2. Kill tmux session:
```bash
tmux kill-session -t <session_name>
```
3. Confirm tmux session removal:
```bash
tmux ls
```
4. Verify the app process did not survive as an orphan:
```bash
pgrep -af '<app_binary_name_or_pattern>' || true
```
5. If process remains and matches the recorded process group, terminate the group:
```bash
kill -TERM -<PANE_PGID>
sleep 1
pgrep -af '<app_binary_name_or_pattern>' || true
```
6. If still running, force kill as last resort:
```bash
kill -KILL -<PANE_PGID>
```

Notes:
- `tmux kill-session` may remove tmux cleanly while the launched command chain remains alive.
- Always verify process cleanup explicitly; do not assume session deletion means app exit.
- Report exactly which cleanup stage stopped the process.

## Output Style

When summarizing test execution:
1. List the steps performed.
2. For each step, state pass/fail with a short reason.
3. Include any residual risks or untested paths.
4. Mention whether tmux session was cleaned up.

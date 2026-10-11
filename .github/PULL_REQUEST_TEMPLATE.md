<!--
PR title should follow Conventional Commits as it becomes the squash commit message.
Examples: feat(tabs): add tab grouping / fix(keybinds): fixed keybinds not working on Windows
-->

## What
<!-- One or two sentences describing the change. -->

## Why
<!-- The problem you're solving. Please link to the issue if it exists (e.g. "Closes #42"). -->

## How
<!-- Brief notes on the approach, only if non-obvious. -->

## Testing
<!-- How did you verify this works? "Ran tsc clean" is not enough on its own, please
     describe the actual flows you exercised. -->

- [ ] `cd crates/shell && pnpm exec tsc --noEmit` clean
- [ ] Manual smoke-test of the affected feature
- [ ] `make fmt-check`, `make lint` and `make test` clean (these cover the Rust and the frontend)
- [ ] (If you changed a `#[tauri::command]` signature) called out below so the FE caller can be updated in lockstep
- [ ] (If UI) tested in `make dev`
- [ ] Platforms tested: <!-- macOS / Linux / Windows -->

## Screenshots / GIFs
<!-- Required for any UI change. Before / after if applicable. -->

## Notes for reviewer
<!-- Anything risky, anything you want a second opinion on, follow-ups for later. -->

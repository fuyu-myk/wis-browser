# Contributing to Wis

Wis is a solo-maintained project with a strong focus on privacy and security. Contributions are welcome, but **alignment matters more than volume**. Please read the following guidelines before contributing.

## How this project is maintained

- Wis has one maintainer [@fuyu-myk](https://github.com/fuyu-myk). All contributions are reviewed and merged by the maintainer.
- Review bandwidth is limited.
- Not every contribution will be accepted, hope to seek your understanding.

## Quick start

```bash
make install
make dev
```

Prereqs: Rust via [rustup](https://rustup.rs) (it installs the version `rust-toolchain.toml` pins), Node 22.13+, pnpm 11+, plus your platform's [Tauri prerequisites](https://tauri.app/start/prerequisites/) (on macOS, Xcode or its Command Line Tools 26.4 or later, for the Swift translation bridge).

## Quality bar

Wis positions itself as **secure, fast, production-grade**. Every PR is reviewed against:

- `pnpm exec tsc --noEmit` clean
- `cargo clippy` clean, `cargo fmt` applied
- `pnpm test` and `cargo test` pass
- No new heavy dependencies (>50KB gzip in client bundle, >5MB compiled on Rust side) without justification
- Platform parity preserved (macOS / Linux / Windows / WSL still work)
- Security review for changes to sensitive code like password storage and sandboxing

If you're unsure about any of these criteria, please open an issue to discuss and confirm before starting.

## Branches

Branch off `main`. Use these prefixes (kebab-case):

| Prefix       | Use for                                  |
| ------------ | ---------------------------------------- |
| `feat/`      | New feature                              |
| `fix/`       | Bug fix                                  |
| `chore/`     | Tooling, config, dependencies            |
| `refactor/`  | Refactor without changing behavior       |
| `docs/`      | Docs-only changes                        |
| `perf/`      | Performance work                         |
| `security/`  | Security fix or hardening                |

Examples: `feat/todo-list`, `fix/tabs`, `security/peek-sandbox`.

Don't open PRs from your fork's `main` branch. Work on a feature branch.

## Commits & PRs

The **PR title becomes the squash commit** for most PRs. Multi-commit PRs with well-crafted atomic commits may be merged with a merge commit at the maintainer's discretion (security audits, multi-step refactors). Title must follow [Conventional Commits](https://www.conventionalcommits.org/):

```text
<type>(<scope>): <subject>

# examples
feat(utils): add todo list
fix(tabs): fix tab resizing bug
chore(deps): bump tauri to 2.x
security(peek): tighten sandbox
```

Types: `feat`, `fix`, `chore`, `docs`, `perf`, `refactor`, `test`, `build`, `ci`, `security`.

Common scopes: `panes`, `utils`, `ai`, `settings`, `tabs`, `shortcuts`, `ui`, `windows`, `linux`, `macos`.

Within a PR, individual commit messages can be free-form (they get squashed or grouped).

**Fill out the PR template.** Include: what changed, why, how you tested. Screenshots/GIFs for UI changes. "Tested manually by ..." is the bare minimum.

**Open a draft PR early** if you want feedback mid-flight. Mark "Ready for review" when done.

## Code style

- Follow existing patterns. Read 2-3 adjacent files before adding new ones.
- TypeScript: no `any` unless you really mean it. Strict mode is on.
- Rust: `cargo fmt` + `clippy` clean.
- Comments: only for *why*, not *what*. Code should explain itself. No multi-paragraph docstrings.
- No emojis in code or commit messages.

## Project layout

Cargo workspace with five crates, plus a Tauri + React frontend. Dependencies flow strictly downward (`shell/src-tauri` -> `network` -> `storage` -> `core`/`secrets`); never add an edge pointing the other way.

```text

crates/
├── core/                  wis-core: pure data model (BrowserState, Workspace, Tab, settings, ids)
├── secrets/               wis-secrets: vault crypto (ChaCha20-Poly1305) + OS key store
├── storage/               wis-storage: `Storage` trait + SqliteStorage (rusqlite, WAL, migrations)
├── network/               wis-network: tracker blocker, DoH resolver, HTTPS-First, proxy, Tor
└── shell/                 Tauri app + frontend (the only crate that renders UI)
    │
    ├── src/               React/TypeScript frontend
    │   ├── components/    UI components (chrome, settings, tabs, overlays)
    │   ├── hooks/         React hooks (browser state, webviews, overlays, keybinds)
    │   ├── lib/           Framework-agnostic helpers (url, theme, keybindings, ...)
    │   ├── overlay/       Overlay surface router + individual overlay surfaces
    │   ├── bindings/      Typed IPC wrappers (commands.ts, types.ts) mirroring wis-core
    │   └── styles/        CSS tokens, fonts, window chrome styles
    │
    └── src-tauri/         Rust backend
        └── src/
            ├── commands/  IPC command handlers
            └── *.rs       One module per native feature (peek, vault, backup, windows, ...)
```

For the detailed architecture (crate responsibilities, IPC command pattern, webview compositing/overlay model, multi-window design, frontend structure), see [WIS.md](../../WIS.md), especially if you touched `wis-core`, `wis-storage`, or the webview/overlay/window internals.

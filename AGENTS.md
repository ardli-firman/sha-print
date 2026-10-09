# ShaPrint — agent notes

ShaPrint shares printers over LAN and cross-VLAN using standard IPPS and multicast DNS discovery.
The desktop application is located in `apps/desktop` (React + TypeScript over a modular Rust crate).
See `apps/desktop/README.md` and `docs/adr`.

The legacy .NET 8 WPF application has been retired to the `legacy/wpf-lts` branch (ADR 0011).

Domain vocabulary is in `CONTEXT.md` (server, client, shared printer, print job, Network Channel) — use those terms exactly.

## Verification before you hand off

Run the suite that owns the files you touched; do not claim a result you did not see.

```bash
# Frontend & workspace tests (from repository root or apps/desktop)
bun run typecheck && bun run test

# Rust backend & integration tests (from apps/desktop/src-tauri)
cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test
```

A feature or bug fix is not done until the changed surface was actually exercised (launched app, real command, reproduced-then-fixed bug), not just compiled.

## Conventions

- Commits and PR titles use Conventional Commits: `<type>(<scope>): <imperative summary>`. Types in use: `feat`, `fix`, `refactor`, `chore`, `docs`, `test`, `build`, `style`.
- Branch names: `feat/<name>`, `fix/<name>`, `docs/<name>`, `review/<name>`.
- Base branch: `main`.
- Architecture decisions are recorded as ADRs in `docs/adr`; add one when a decision outlives the PR.
- Never put credentials, Network Channel values, IPP URLs with keys, or print job content in logs, status payloads, IPC DTOs, test fixtures, or commits. The Tauri app's multicast DNS discovery is unsigned by design, because identity there is pinned by the TLS certificate fingerprint (ADR 0004).

## Pull requests

1. Read `.github/pull_request_template.md` and fill every section — that file is the contract for what a PR must state.
2. Title: Conventional Commit.
3. Link issues in the template body: `Closes #N` auto-closes on merge into the default branch (`main`).
4. Verification section: exact commands plus the observed output, and a runtime check of the changed surface. An unevidenced acceptance criterion is an open criterion — say so instead of asserting success.
5. Mentions: request the review agent with `@ebra-reviewer review`, or @ a human reviewer.

```bash
# 1. Write the filled template (comments removed) to a scratch file.
# 2. Create the PR with that file as the body.
gh pr create --base main --title "feat(scope): summary" --body-file /tmp/pr-body.md
```

## Agent skills

### Issue tracker

Issues and specs live in GitHub Issues for `ardli-firman/sha-print`; use the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Domain docs

This is a single-context repo. Read root `CONTEXT.md` and relevant ADRs in `docs/adr/`. See `docs/agents/domain.md`.

### Triage labels

Use the default canonical labels: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, and `wontfix`. See `docs/agents/triage-labels.md`.

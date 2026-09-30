# ShaPrint — agent notes

ShaPrint shares printers over LAN and cross-VLAN. Two code paths live side by side:

- .NET 8 WPF application (current release): `ShaPrint.WpfApp`, `ShaPrint.Core`, `ShaPrint.Updater`, `ShaPrint.Tests`.
- Windows-only Tauri desktop app (the product path, `apps/desktop`): React + TypeScript over one modular Rust crate. See `apps/desktop/README.md` and `docs/adr`.

Domain vocabulary is in `CONTEXT.md` (server, client, shared printer, print job, Network Channel) — use those terms exactly.

## Verification before you hand off

Run the suite that owns the files you touched; do not claim a result you did not see.

```bash
# .NET, repository root
dotnet build ShaPrint.sln -c Release
dotnet test -c Release
```

```bash
# Desktop, apps/desktop
bun run typecheck && bun run test
cd src-tauri && cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test
```

A feature or bug fix is not done until the changed surface was actually exercised (launched app, real command, reproduced-then-fixed bug), not just compiled.

## Conventions

- Commits and PR titles use Conventional Commits: `<type>(<scope>): <imperative summary>`. Types in use: `feat`, `fix`, `refactor`, `chore`, `docs`, `test`, `build`, `style`.
- Branch names: `feat/<name>`, `fix/<name>`, `docs/<name>`, `review/<name>`.
- Base branch: `dev-tauri` for Tauri/IPP migration work (spec #29), `main` for .NET work. Release PRs go `develop` → `main` and are titled `release: ...`.
- Architecture decisions are recorded as ADRs in `docs/adr`; add one when a decision outlives the PR.
- Never put credentials, Network Channel values, IPP URLs with keys, or print/scan job content in logs, status payloads, IPC DTOs, test fixtures, or commits. TCP payloads stay AES-256-GCM; discovery stays HMAC-SHA256.

## Pull requests

1. Read `.github/pull_request_template.md` and fill every section — that file is the contract for what a PR must state.
2. Title: Conventional Commit. `stable-release.yml` derives the release version from commit subjects, so `feat:` → minor, `fix:` → patch, `<type>!:` or `BREAKING CHANGE` → major. A label that does not match the change ships a wrong version.
3. Link issues in the template body: `Closes #N` auto-closes on merge **into the default branch (`main`)**. A PR based on `dev-tauri` does not auto-close anything — use `Relates to #N` and close the ticket from the release PR or by hand.
4. Verification section: exact commands plus the observed output, and a runtime check of the changed surface. An unevidenced acceptance criterion is an open criterion — say so instead of asserting success.
5. Mentions: request the review agent with `@ebra-reviewer review`, or @ a human reviewer.

```bash
# 1. Write the filled template (comments removed) to a scratch file.
# 2. Create the PR with that file as the body.
gh pr create --base dev-tauri --title "feat(scope): summary" --body-file /tmp/pr-body.md
```

`gh` never expands the template on its own: `--template <file>` only opens an editor and is rejected
next to `--body`/`--body-file`. Read the template, fill it, pass `--body-file`. If a human opens the
PR in the browser instead, GitHub applies the template automatically.

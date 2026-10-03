<!--
Title: <type>(<scope>): <imperative summary>

The title is release input, not decoration: .github/workflows/stable-release.yml computes the next
version from commit subjects on `main`, so a wrong prefix ships a wrong version.

  feat:      → minor bump
  fix:       → patch bump
  <type>!:   → major bump ("BREAKING CHANGE" in the body also forces major)
  chore/docs/refactor/test/build → patch bump (default)

Scopes in use: core, wpf, client, server, scanner, updater, desktop, ipp, discovery, monitor, ui, ci, docs.

Base branch: `dev-tauri` for the Tauri/IPP migration (apps/desktop, spec #29), `main` for .NET work
and hotfixes. Release PRs are `develop` → `main`, titled `release: ...`.

Agents: fill this file into a body file, then `gh pr create --base <base> --title "<title>" --body-file <body.md>`.
(`gh pr create --template <file>` only opens an editor and cannot be combined with `--body`/`--body-file`.)
Delete the HTML comments before submitting; keep every section.
Do not write a result you did not observe. Every claim needs a command or a click path.
-->

## Linked issues

Closes #
Relates to #

<!--
`Closes #N` / `Fixes #N` / `Resolves #N` closes the issue automatically when this PR merges into the
default branch (`main`). PRs merged into `dev-tauri` do NOT trigger auto-close: keep `Relates to #N`
there and close the ticket from the release PR, or by hand.
-->

## Summary

<!-- 3-5 sentences: what changed, why, and which issue/spec it delivers. Name the user-visible outcome. -->

## What changed

<!-- Bullets grouped by area. Name modules and files. State decisions and their reason, not just actions. -->

-

## Acceptance criteria

<!-- If the linked issue has acceptance criteria, copy the checklist and map each one to where it is met. -->

| Criterion | Where |
|---|---|
| | |

## Type of change

- [ ] `feat` — new capability
- [ ] `fix` — bug fix
- [ ] `refactor` / `chore` / `build` / `test` — no behavior change
- [ ] `docs` — docs, ADR, or README only
- [ ] **Breaking change** — `!` in the title, and the section below is filled

## Area

- [ ] .NET — `ShaPrint.Core`
- [ ] .NET — `ShaPrint.WpfApp`
- [ ] .NET — `ShaPrint.Updater`
- [ ] .NET — `ShaPrint.Tests`
- [ ] Desktop — `apps/desktop/src-tauri` (Rust)
- [ ] Desktop — `apps/desktop/src` (React + TypeScript)
- [ ] CI / release — `.github/workflows`
- [ ] Docs / ADR — `docs/adr`, `docs/RELEASE.md`, `README.md`

## Verification

<!-- Exact commands, then the observed result. Paste real output, no paraphrase, no "should work". -->

```bash
# .NET (repository root)
dotnet build ShaPrint.sln -c Release
dotnet test -c Release
```

```bash
# Desktop (apps/desktop)
bun install --frozen-lockfile
bun run typecheck
bun run test

cd src-tauri
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

- Result: <!-- e.g. "dotnet test → 214 passed, 0 failed; cargo test → 27 unit + 4 integration passed" -->
- Runtime check: <!-- Launch the changed surface and describe what you saw, or state the environment limit -->
- CI run: <!-- workflow name + run id once green -->

## Breaking changes and migration

<!-- Required when "Breaking change" is checked: what breaks, who is affected, and the migration steps. -->
None.

## Risk and rollback

<!-- Blast radius, failure modes, and how to revert. `git revert` of the merge commit is the default. -->

## Security

- [ ] No credentials, Network Channel values, or print/scan job content in code, logs, status payloads, IPC DTOs, or commits
- [ ] New network input is validated and size-bounded; TCP payloads stay AES-256-GCM, discovery stays HMAC-SHA256
- [ ] No capability, permission, or CSP relaxation beyond what the feature needs

## Checklist

- [ ] PR title is a Conventional Commit and matches the actual change
- [ ] Every section above is filled; no placeholder left behind
- [ ] New behavior is covered by a test that fails without the change (or the issue explains why not)
- [ ] Existing tests, docs, and callers are updated; obsolete code from the change is removed
- [ ] No secrets committed; new build artifacts and generated files are ignored
- [ ] Linked issue uses the right keyword for the base branch

## Reviewer / agent mentions

<!-- Optional: request the review agent with `@ebra-reviewer review`, or @ the human reviewer. -->

## Notes for reviewers

<!-- Files to read first, tradeoffs taken, follow-ups that are deliberately out of scope. -->

# Contributing to kieMonoBench

[日本語](CONTRIBUTING.md) · English

Bug reports, feature requests and pull requests are welcome. We cannot promise a quick reply; the project is maintained by a very small team.

## Before you start

- **Please open an issue to discuss a change before you build it.** Small fixes (typos, obvious bugs) can go straight to a pull request.
- The philosophy, structure, input/output contracts and working rules are in [AGENTS.md](AGENTS.md) (Japanese). Area-specific rules are in [.claude/rules/](.claude/rules/), and [docs/地図.md](docs/地図.md) maps where things live. People and AI agents follow the same rules.
- Do not report security issues in public issues. Follow [SECURITY.md](SECURITY.md) instead.

## Building and checking

See [README.en.md](README.en.md#building) for what you need to build. Before opening a pull request, run the checks for the crates you changed:

```sh
cargo xtask verify                # tests and checks for the crates you changed
cargo xtask verify --dependents   # also the crates that depend on them
```

Report the result as counts (passed, failed, skipped) in the pull request. [docs/verify.md](docs/verify.md) explains the checks (layer direction, argument declarations, wording and so on). If a check fails, fix the code rather than loosening the check.

## Pull request rules

- One pull request, one purpose. Put unrelated formatting or refactoring in a separate pull request.
- If you change the user interface, attach before and after screenshots side by side.
- When you add a feature, record it as one file in `docs/機能/` (see [docs/機能/README.md](docs/機能/README.md)).
- New dependencies must use permissive licences such as MIT, Apache-2.0 or BSD (`deny.toml`). **GPL, LGPL and AGPL dependencies and code cannot be used.**
- Do not copy or translate code from other software, whatever its licence. Reading it to understand how something works is fine, but write your own code.
- Do not name other CAD/CAM products or cite research sources in code, comments or documents (names of machines and file formats needed for compatibility are fine). **Pull request descriptions, commit messages and branch names are public too.**
- We do not build interaction designs that are covered by patents. The list of shapes we avoid is in [docs/避ける形.md](docs/避ける形.md).

## How changes are merged

A maintainer reads the pull request, runs the full check (`cargo xtask verify --all`) locally and then merges it into main. The merge button on GitHub is not used. How incoming issues and pull requests are triaged and answered is described in [docs/受け方.md](docs/受け方.md) (Japanese).

## Licence

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 licence, shall be dual licensed under MIT and Apache-2.0, without any additional terms or conditions. No contributor licence agreement (CLA) is required. The product name and the icon are covered by [TRADEMARKS.md](TRADEMARKS.md).

## Conduct

Please follow [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

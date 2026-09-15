# PR Pal

PR Pal is a fork of [PR Bro](https://github.com/toniperic/pr-bro) by [@toniperic](https://github.com/toniperic). The `PR_BRO_GH_TOKEN` environment variable keeps its upstream name.

[![CI](https://github.com/nate-double-u/pr-pal/actions/workflows/ci.yml/badge.svg)](https://github.com/nate-double-u/pr-pal/actions/workflows/ci.yml) [![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

[![Demo](https://asciinema.org/a/780716.svg)](https://asciinema.org/a/780716)

Know which PR to review next. PR Pal ranks pull requests by weighted scoring across your GitHub queries, so you always start with the most important review.

## Requirements

- **GitHub Personal Access Token**: 
  - `repo` scope for private repos
  - `public_repo` for public only
- **Platforms**: 
  - macOS (Intel + Apple Silicon)
  - Linux (x64)

## Installation

Run from source with the Rust toolchain (install from [rustup.rs](https://rustup.rs)):

```bash
git clone https://github.com/nate-double-u/pr-pal.git
cd pr-pal
cargo run --release
```

Installing upstream `pr-bro` via Homebrew, Cargo, or binary download gets you the original, without this fork's changes; see the [upstream README](https://github.com/toniperic/pr-bro#installation).

## Quick Usage

```bash
cargo run --release
```

On first run, PR Pal will prompt you with a series of questions to set up your configuration. It will also ask for your GitHub token interactively. To skip the token prompt, set the `PR_BRO_GH_TOKEN` environment variable.

For the full list of configuration options, see the [Configuration Reference](docs/configuration.md).

Use `cargo run -- --help` for all command-line options. Press `?` in the TUI for keyboard shortcuts.

## Features

**Weighted scoring** calculates a single priority number for each PR based on age, approval count, size, labels, draft status, and whether you've reviewed it before, all based on your preferences/configuration. Each parameter can be used to boost or penalize PRs score in any way you see fit.

**Interactive TUI** shows all PRs sorted by score. Navigate with arrow keys or vim bindings. Press `b` to see the score breakdown for any PR. Press `r` to refresh.

**Multiple queries** let you track different PR sets. Each query can override global scoring rules. First-match-wins when a PR appears in multiple queries.

**Snooze PRs** to hide them temporarily. Press `s` to snooze for a custom duration or indefinitely. Snoozed PRs live in a separate tab and don't clutter your main list.

**Review-cycle awareness** (optional) hides PRs you've already reviewed while the ball is in the author's court, then resurfaces them, tagged and optionally score-boosted, when the author pushes, you're mentioned, or your review is re-requested. A safety valve resurfaces anything quiet for too long. See [docs/configuration.md](docs/configuration.md#awaiting-author-suppression).

**Score breakdown** shows exactly how a PR's score was calculated. See which factors contributed most. Press `b` on any PR to open the detail view.

**Light and dark themes** adapt to your terminal. PR Pal auto-detects your terminal background and picks the right color palette.

**ETag-based HTTP caching** reduces GitHub API calls. Auto-refresh only fetches if data changed on the server. Manual refresh bypasses in-memory cache.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and commit message format.

## License

[MIT](LICENSE)

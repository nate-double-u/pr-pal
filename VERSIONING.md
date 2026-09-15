# Versioning

PR Pal uses [Semantic Versioning 2.0](https://semver.org/).

## Commit Conventions

Commit messages follow the conventional commit format:

| Commit Type | Example | Version Impact |
|-------------|---------|----------------|
| `feat` | `feat(tui): add score breakdown view` | MINOR |
| `fix` | `fix(cache): handle network timeout` | PATCH |
| `perf` | `perf(scoring): optimize label matching` | PATCH |
| `feat!` / `BREAKING CHANGE:` | `feat(config)!: change YAML schema` | MAJOR |
| `docs`, `style`, `refactor`, `test`, `build`, `ci`, `chore` | Any | No bump |

### Breaking Changes for PR Pal

These changes warrant a major version bump:

- CLI flag renames or removals
- Config schema changes requiring user edits to existing configs
- Output format changes that break scripting or automation
- Removal of features

### Non-Breaking Changes

These are safe additive changes:

- Adding new features (as long as existing features work unchanged)
- Bug fixes that change behavior to match documentation
- Internal refactoring with no user-visible changes
- Performance improvements

## Releases

PR Pal publishes no artifacts: no crates.io releases, no binaries, no
Homebrew formula, and the release workflows are disabled. Run it from
source (see the [README](README.md)), or install straight from git:

```bash
cargo install --git https://github.com/nate-double-u/pr-pal
```

Version bumps are manual, marked by a git tag and a GitHub release.
Upstream PR Bro's automated release process is documented in
[their VERSIONING.md](https://github.com/toniperic/pr-bro/blob/master/VERSIONING.md).

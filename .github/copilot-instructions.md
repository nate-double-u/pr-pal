# Copilot instructions

Guidance for AI coding agents working in this repository. Read alongside
`CONTRIBUTING.md`.

## Locked regression tests

This repo uses "locked" regression tests to keep fixed bugs from silently
coming back. Treat a locked test as immutable.

A locked test is tagged with a greppable comment naming the fix it guards:

```rust
// LOCKED: regression for <issue/PR>
```

List them with `rg "LOCKED:"`.

Rules:

- **Do not** edit, weaken, skip, rename, or delete a locked test, or change the
  behavior it asserts, without the maintainer's explicit approval first. This
  applies in automated/agent modes too.
- If a change appears to require touching a locked test, stop and ask. Add new
  tests alongside; leave the locked one intact.

When you fix a bug, lock it (fix-first TDD):

1. Write a test that reproduces the bug and confirm it fails (red) *before*
   fixing. A test that passes before the fix proves nothing.
2. Make the minimum change to turn it green.
3. Tag it `// LOCKED: regression for <issue/PR>` (name the issue or PR, or a
   short description if none is filed yet).

Current example: `src/github/search.rs` locks `search_prs` pagination, which
must follow the `Link` header past the first page rather than stopping at
GitHub's default page size.

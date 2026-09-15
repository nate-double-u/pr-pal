# Configuration Reference

This document covers the full configuration options for PR Pal. For a quick-start guide, see the [README](../README.md).

Configuration file location: `~/.config/pr-pal/config.yaml`

Upgrading from PR Bro? Move your existing configuration once: `mv ~/.config/pr-bro ~/.config/pr-pal`. If you set `PR_BRO_GH_TOKEN` in your shell profile, rename it to `PR_PAL_GH_TOKEN`.

## Full Configuration Example

```yaml
# Theme: "auto" (default, detects terminal), "dark", or "light"
theme: auto

# Auto-refresh interval in seconds (default: 300 = 5 minutes)
auto_refresh_interval: 300

# Global scoring configuration (applies to all queries unless overridden)
scoring:
  base_score: 100
  age: "+1 per 1h"       # Adds 1 point per hour of age
  approvals: "+10 per 1"  # Adds 10 points per approval
  size:
    exclude: ["*.lock", "package-lock.json"]
    buckets:
      - range: "0-9"
        effect: "x10"     # Extremely small PRs get a huge boost
      - range: "10-99"
        effect: "x5"      # Small PRS get a decent boost
      - range: "100-249"
        effect: "x1"      # Medium PRs: no change
      - range: "250-499"
        effect: "x0.5"    # Large PRs get a decent penalty
      - range: ">=500"
        effect: "x0.25"   # Extremely large PRs get a huge penalty
  labels:
    - name: "highest priority"
      effect: "x10"
    - name: "wip"
      effect: "x0.5"
  previously_reviewed: "x2.5"  # Previously reviewed PRs get a boost
  draft: "x0.1"               # Deprioritize draft PRs
  since_my_review:            # React to what happened after your last review
    pushed: "x5"              # Author pushed new commits
    mentioned: "x5"           # You were @-mentioned
    review_requested: "x3"    # Your review was re-requested

# Hide reviewed PRs while the ball is in the author's court (optional)
suppress:
  awaiting_author: true
  wake_on: [push, mention, review_request]
  resurface_after: 21d        # Safety valve; "never" disables it

# Queries to execute (at least one required)
queries:
  - name: "foo/bar PRs needing my attention"
    query: "is:pr is:open review-requested:@me repository:foo/bar"
    scoring:               # Per-query scoring (merges with global)
      base_score: 50
      age: "x1.5 per 1d"  # Gets a x1.5 boost per day of age
      approvals: "+5 per 1"
```

## Scoring Factors

Each scoring factor is optional and can use addition (`+N`) or multiplication (`xN`) effects.

### Age

Format: `"+N per DURATION"` or `"xN per DURATION"`

Duration uses humantime format: `1h`, `30m`, `1d`, `1w`

Examples:
- `"+1 per 1h"` — adds 1 point per hour of age
- `"x1.1 per 1d"` — multiplies score by 1.1 per day of age

### Approvals

Format: `"+N per 1"`, `"xN per 1"`, `"+N"`, or `"xN"`

This is NOT bucket-based — the effect applies per approval count.

Examples:
- `"+10 per 1"` — adds 10 points per approval
- `"x2 per 1"` — doubles score per approval
- `"+50"` — adds 50 points if any approvals exist

### Size

Bucket-based configuration with optional file exclusions.

**Range formats:**
- `"<N"` — less than N lines
- `"<=N"` — less than or equal to N lines
- `">N"` — greater than N lines
- `">=N"` — greater than or equal to N lines
- `"N-M"` — inclusive range from N to M lines

**Effect formats:**
- `"+N"` — add N points
- `"xN"` — multiply score by N

**Important:** Size bucket ranges must NOT overlap. The validator will reject configurations with overlapping ranges at startup.

Example:

```yaml
size:
  exclude:
    - "*.lock"
    - "package-lock.json"
    - "yarn.lock"
  buckets:
    - range: "<100"
      effect: "x5"      # Small PRs: 5x multiplier
    - range: "100-500"
      effect: "x1"      # Medium PRs: no change
    - range: ">500"
      effect: "x0.5"    # Large PRs: 0.5x penalty
```

**Exclude pattern behavior:**
- Patterns match against the **filename only** (basename), not the full file path. For example, `*.lock` will match `Cargo.lock` and `subdir/package-lock.json`.
- When exclude patterns are configured, PR Pal fetches per-file diff data from the GitHub API to determine which files to exclude. This adds 1-2 API calls per PR (paginated at 100 files per page).
- If the per-file data fetch fails (e.g., rate limit), PR Pal falls back to the aggregate size from the PR summary (no exclusions applied).
- Without exclude patterns, no extra API calls are made.
- Invalid glob patterns are caught at startup during config validation.

### Labels

Optional. Applies score effects based on GitHub labels on the PR. Multiple matching labels compound their effects sequentially (not first-match). Label matching is **case-insensitive**.

```yaml
labels:
  - name: "urgent"
    effect: "+10"     # Add 10 points for urgent PRs
  - name: "wip"
    effect: "x0.5"    # Halve score for work-in-progress
  - name: "critical"
    effect: "x2"      # Double score for critical PRs
```

A PR with both "urgent" and "critical" labels gets both effects: score + 10, then x2.

Each matching label appears as a separate entry in the score breakdown detail view (press `b`).

### Previously Reviewed

Optional. Applies a score effect when the authenticated user (the user whose token is configured) has previously submitted a review on the PR.

If your team is using review requests via GitHub to ask for reviews when a PR is ready to review, this configuration plays well with it, as you can then use `review-requested:@me` filter in the GitHub query to fetch PRs needing your review.

That workflow, paired with this configuration, means that when they need you to re-review such a PR, you could prioritize it using

```yaml
previously_reviewed: "x2.5"   # Prioritize already-reviewed PRs
```

### Draft

Optional. Applies a score effect when a PR is marked as a draft. Useful for deprioritizing PRs that aren't ready for review yet.

```yaml
draft: "x0.1"   # Heavily deprioritize draft PRs
```

### Since My Review

Optional. Applies a score effect based on what has happened since your last review of a PR. Complements [awaiting-author suppression](#awaiting-author-suppression): suppression controls *whether* a reviewed PR is shown, this factor controls *how high* it ranks once it resurfaces.

```yaml
since_my_review:
  pushed: "x5"             # Author pushed commits after your review
  mentioned: "x5"          # You were @-mentioned after your review
  review_requested: "x3"   # Your review was re-requested
  awaiting_author: "x0.2"  # Nothing happened yet (rarely needed with suppression on)
```

All fields are optional flat effects (`+N` or `xN`). At most one applies per PR, checked in this order: pushed, mentioned, review_requested, awaiting_author. PRs you have not reviewed are unaffected.

The signals come from each PR's timeline, fetched only for PRs you have reviewed and only when this factor or `suppress` is configured.

## Effect Syntax Summary

| Syntax | Meaning |
|--------|---------|
| `+N` | Add N points |
| `xN` | Multiply score by N |
| `+N per DURATION` | Add N points per time unit (age only) |
| `xN per DURATION` | Multiply by N per time unit (age only) |
| `+N per M` | Add N points per M units (approvals only) |
| `xN per M` | Multiply by N per M units (approvals only) |

Labels, previously_reviewed, draft, and since_my_review use flat effects (`+N` or `xN`), not per-unit effects.

## Per-Query Scoring

Queries can override individual fields of the global scoring configuration. When a PR appears in multiple queries, the **first query's scoring is used** (first-match-wins). Per-query scoring merges with global scoring at the **leaf level** — only the exact sub-fields you specify in a query override the global values; everything else is inherited. This means setting `scoring.size.exclude` in a query does **not** replace the entire `size` block; global `size.buckets` are preserved (and vice versa).

Example:

```yaml
scoring:
  base_score: 100
  age: "+1 per 1h"
  approvals: "+10 per 1"
  size:
    buckets:
      - range: "<100"
        effect: "x5"
      - range: "100-500"
        effect: "x1"
      - range: ">500"
        effect: "x0.5"
  labels:
    - name: "urgent"
      effect: "+20"
    - name: "wip"
      effect: "x0.5"

queries:
  - name: urgent
    query: "is:pr label:urgent"
    scoring:
      age: "+10 per 1h"       # Override: urgent PRs age faster
      size:
        exclude: ["*.lock"]   # Add exclude — inherits global buckets
      labels:
        - name: "urgent"
          effect: "+50"       # Override: stronger urgent boost for this query
      # base_score, approvals — inherited from global
      # size.buckets — inherited from global (not overridden)
      # label "wip" — inherited from global (not mentioned here)

  - name: other
    query: "is:pr org:myorg"
    # No scoring block — uses global scoring entirely
```

In this example, the "urgent" query:
- **Overrides** `age` to `"+10 per 1h"` (urgent PRs age faster).
- **Adds** `size.exclude` with `["*.lock"]`. Because merging is leaf-level, global `size.buckets` are inherited — setting `size.exclude` does NOT replace the entire `size` block.
- **Overrides** the "urgent" label effect from `"+20"` to `"+50"`. Labels merge by name (case-insensitive): the query's "urgent" label wins over the global one. The global "wip" label is preserved because the query does not mention it.
- **Inherits** `base_score`, `approvals`, `previously_reviewed`, and `draft` from the global config (not specified in the query, so global values apply).

### YAML Merge Keys

YAML merge keys (`<<:`) are supported by the YAML parser for reducing duplication within your config file. This is a YAML feature processed when reading the file, independent of the runtime merge that combines global and per-query scoring. Note that because PR Pal validates config structure strictly (`deny_unknown_fields`), YAML anchors must be placed inside fields that expect the anchored structure, not at the top level. For advanced YAML anchor/merge-key usage, refer to the [YAML specification](https://yaml.org/type/merge.html).

## Awaiting-Author Suppression

Optional top-level `suppress` block. After you review a PR, it usually can't move until the author acts; suppression hides it from the Active list so your queue only shows PRs you can act on. Suppressed PRs appear in the Snoozed tab marked "awaiting author".

```yaml
suppress:
  awaiting_author: true                        # Feature switch (default: true when block present)
  wake_on: [push, mention, review_request]     # Events that resurface a PR (default: all three)
  resurface_after: 21d                         # Safety valve; "never" disables (default: 21d)
```

A suppressed PR returns to the Active list when, after your last activity on it (review or comment):

- **push**: the author pushes new commits (or force-pushes)
- **mention**: you are @-mentioned
- **review_request**: your review is re-requested
- the **safety valve** expires: nothing happened for `resurface_after`, so it resurfaces tagged "(stalled)" rather than staying invisible forever

Resurfaced rows are tagged with the wake reason: `(updated)`, `(mentioned)`, `(re-requested)`, or `(stalled)`.

Notes:

- Suppression is derived from GitHub state on each refresh; nothing is written to your snooze file. Commenting on a PR (a nudge) re-arms the safety valve.
- Manual snooze always wins: snoozing a suppressed PR converts it into a regular snooze.
- Omitting the `suppress` block (or `awaiting_author: false`) disables the feature.
- `unsnooze` does not apply to suppressed PRs; they come back via wake events.

## Theme

PR Pal supports light and dark color themes. The default is `auto`, which detects your terminal's background color at startup and selects the appropriate palette.

```yaml
theme: auto    # Detect terminal background (default)
theme: dark    # Always use dark theme
theme: light   # Always use light theme
```

If auto-detection fails (e.g., over SSH or in tmux), it falls back to the dark theme.

## Config Validation

PR Pal validates your configuration at startup with clear error messages:

- **Unknown YAML keys** are rejected (catches typos like `approvalls` instead of `approvals`)
- **Overlapping size bucket ranges** are rejected (prevents ambiguous scoring)
- **Invalid effect syntax** is caught with helpful messages
- **Empty label names** are rejected
- **Invalid glob patterns** in `size.exclude` are caught (e.g., unclosed character classes like `[invalid`)
- **Invalid label effects**, **invalid previously_reviewed effects**, and **invalid draft effects** are caught at startup
- **Invalid since_my_review effects** and **invalid suppress.resurface_after durations** are caught at startup

Validation errors will show exactly what's wrong and where, so you can fix configuration issues quickly.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Label-based scoring effect.
///
/// Maps label names to score effects. Multiple matching labels compound.
///
/// Example YAML:
/// ```yaml
/// labels:
///   - name: "urgent"
///     effect: "+10"
///   - name: "wip"
///     effect: "x0.5"
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LabelEffect {
    pub name: String,
    pub effect: String,
}

/// Main scoring configuration.
///
/// Defines how PR scores are calculated. Each factor is optional and can use
/// either addition (`+N`) or multiplication (`xN`) operations.
///
/// Example YAML:
/// ```yaml
/// scoring:
///   base_score: 100
///   age: "+1 per 1h"
///   approvals: "x2 per 1"
///   size:
///     exclude: ["*.lock"]
///     buckets:
///       - { range: "<100", effect: "x5" }
///       - { range: ">=500", effect: "x0.5" }
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ScoringConfig {
    /// Base score before factors are applied (default: 100.0)
    #[serde(default)]
    pub base_score: Option<f64>,

    /// Age factor: format is "+N per <duration>" or "xN per <duration>"
    /// Example: "+1 per 1h" adds 1 point per hour of age
    #[serde(default)]
    pub age: Option<String>,

    /// Approval factor: effect string applied based on approval count
    /// Format: "+N per 1", "xN per 1", "+N", or "xN"
    /// Example: "+10 per 1" adds 10 points per approval
    #[serde(default)]
    pub approvals: Option<String>,

    /// Size factor: bucket-based with optional file exclusions
    #[serde(default)]
    pub size: Option<SizeConfig>,

    /// Label-based scoring effects (case-insensitive, multiple labels compound)
    /// Example: [{ name: "urgent", effect: "+10" }]
    #[serde(default)]
    pub labels: Option<Vec<LabelEffect>>,

    /// Previously reviewed factor: effect applied when user has reviewed PR
    /// Example: "x0.5" to deprioritize previously-reviewed PRs
    #[serde(default)]
    pub previously_reviewed: Option<String>,

    /// Draft factor: effect applied when PR is a draft
    /// Example: "x0.1" to deprioritize draft PRs
    #[serde(default)]
    pub draft: Option<String>,

    /// Since-my-review factor: effect applied based on what happened after
    /// the user's last activity (review or comment) on a PR they reviewed
    #[serde(default)]
    pub since_my_review: Option<SinceMyReviewScoring>,
}

/// Since-my-review scoring effects.
///
/// Applied only to PRs the user has already reviewed. Exactly one effect
/// fires, chosen by what happened after the user's last activity:
/// `pushed` > `mentioned` > `review_requested` > `awaiting_author`.
///
/// Example YAML:
/// ```yaml
/// since_my_review:
///   pushed: "x5"
///   mentioned: "x5"
///   review_requested: "x3"
///   awaiting_author: "x0.2"
/// ```
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SinceMyReviewScoring {
    /// New commits landed after the user's last activity
    #[serde(default)]
    pub pushed: Option<String>,

    /// User was @-mentioned after their last activity
    #[serde(default)]
    pub mentioned: Option<String>,

    /// Review was re-requested from the user after their last activity
    #[serde(default)]
    pub review_requested: Option<String>,

    /// Nothing notable since the user's last activity (damp alternative for
    /// setups that don't hide awaiting-author PRs via `suppress`)
    #[serde(default)]
    pub awaiting_author: Option<String>,
}

impl Default for ScoringConfig {
    fn default() -> Self {
        Self {
            base_score: Some(100.0),
            age: Some("+1 per 1h".to_string()),
            approvals: Some("+10 per 1".to_string()),
            size: Some(SizeConfig {
                exclude: None,
                buckets: Some(vec![
                    SizeBucket {
                        range: "<100".to_string(),
                        effect: "x5".to_string(),
                    },
                    SizeBucket {
                        range: "100-500".to_string(),
                        effect: "x1".to_string(),
                    },
                    SizeBucket {
                        range: ">500".to_string(),
                        effect: "x0.5".to_string(),
                    },
                ]),
            }),
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        }
    }
}

/// Merge per-query scoring config with global config at field level.
/// Per-query `Some` values override global values.
/// Per-query `None` values fall through to global values.
/// This allows setting only `scoring.age` in a query while preserving global size/approvals/etc.
pub fn merge_scoring_configs(
    global: &ScoringConfig,
    query: Option<&ScoringConfig>,
) -> ScoringConfig {
    let Some(query) = query else {
        return global.clone();
    };

    ScoringConfig {
        base_score: query.base_score.or(global.base_score),
        age: query.age.clone().or_else(|| global.age.clone()),
        approvals: query.approvals.clone().or_else(|| global.approvals.clone()),
        size: merge_size_configs(global.size.as_ref(), query.size.as_ref()),
        labels: merge_label_configs(global.labels.as_ref(), query.labels.as_ref()),
        previously_reviewed: query
            .previously_reviewed
            .clone()
            .or_else(|| global.previously_reviewed.clone()),
        draft: query.draft.clone().or_else(|| global.draft.clone()),
        since_my_review: merge_since_my_review(
            global.since_my_review.as_ref(),
            query.since_my_review.as_ref(),
        ),
    }
}

/// Merge SinceMyReviewScoring at field level, mirroring merge_size_configs:
/// per-query set fields override, unset fields inherit from global.
fn merge_since_my_review(
    global: Option<&SinceMyReviewScoring>,
    query: Option<&SinceMyReviewScoring>,
) -> Option<SinceMyReviewScoring> {
    match (query, global) {
        (Some(q), Some(g)) => Some(SinceMyReviewScoring {
            pushed: q.pushed.clone().or_else(|| g.pushed.clone()),
            mentioned: q.mentioned.clone().or_else(|| g.mentioned.clone()),
            review_requested: q
                .review_requested
                .clone()
                .or_else(|| g.review_requested.clone()),
            awaiting_author: q
                .awaiting_author
                .clone()
                .or_else(|| g.awaiting_author.clone()),
        }),
        (Some(q), None) => Some(q.clone()),
        (None, g) => g.cloned(),
    }
}

/// Merge SizeConfig with leaf-level field handling.
/// When both global and query have SizeConfig:
/// - exclude: per-query overrides global (or falls through if None)
/// - buckets: per-query overrides global (or falls through if None)
///
/// Absent field (None) means inherit from global; explicitly set field means override.
fn merge_size_configs(
    global: Option<&SizeConfig>,
    query: Option<&SizeConfig>,
) -> Option<SizeConfig> {
    match (query, global) {
        (Some(q), Some(g)) => Some(SizeConfig {
            exclude: q.exclude.clone().or_else(|| g.exclude.clone()),
            buckets: q.buckets.clone().or_else(|| g.buckets.clone()),
        }),
        (Some(q), None) => Some(q.clone()),
        (None, g) => g.cloned(),
    }
}

/// Merge label configs by name (case-insensitive).
/// Query labels override global labels with same name.
/// Global labels not in query are preserved.
fn merge_label_configs(
    global: Option<&Vec<LabelEffect>>,
    query: Option<&Vec<LabelEffect>>,
) -> Option<Vec<LabelEffect>> {
    match (query, global) {
        (None, g) => g.cloned(),
        (Some(q), None) => Some(q.clone()),
        (Some(q), Some(g)) => {
            let mut merged: HashMap<String, LabelEffect> = HashMap::new();
            // Add global labels first (lowercase keys for case-insensitive dedup)
            for label in g {
                merged.insert(label.name.to_lowercase(), label.clone());
            }
            // Override with query labels (query wins on collision)
            for label in q {
                merged.insert(label.name.to_lowercase(), label.clone());
            }
            Some(merged.into_values().collect())
        }
    }
}

/// Size factor configuration.
///
/// Supports file exclusion patterns and size-based buckets.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SizeConfig {
    /// Glob patterns for files to exclude from size calculation
    /// Example: ["*.lock", "package-lock.json"]
    #[serde(default)]
    pub exclude: Option<Vec<String>>,

    /// Size buckets mapping line count ranges to effects
    #[serde(default)]
    pub buckets: Option<Vec<SizeBucket>>,
}

/// Size factor bucket.
///
/// Maps line count ranges to score effects.
/// Range format: "<N", "<=N", ">N", ">=N", "N-M" (inclusive range)
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SizeBucket {
    /// Range expression (e.g., "<100", ">=500", "100-500")
    pub range: String,

    /// Effect on score (e.g., "x5", "x0.5")
    pub effect: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_scoring_config() {
        let config = ScoringConfig::default();

        assert_eq!(config.base_score, Some(100.0));
        assert_eq!(config.age, Some("+1 per 1h".to_string()));
        assert_eq!(config.approvals, Some("+10 per 1".to_string()));
        assert!(config.size.is_some());
        assert!(config.labels.is_none());
        assert!(config.previously_reviewed.is_none());
    }

    #[test]
    fn test_scoring_config_serde_roundtrip() {
        let config = ScoringConfig::default();
        let yaml = serde_saphyr::to_string(&config).unwrap();
        let parsed: ScoringConfig = serde_saphyr::from_str(&yaml).unwrap();
        assert_eq!(config, parsed);
    }

    #[test]
    fn test_partial_scoring_config_parse() {
        let yaml = r#"
base_score: 200
age: "+5 per 1h"
"#;
        let config: ScoringConfig = serde_saphyr::from_str(yaml).unwrap();
        assert_eq!(config.base_score, Some(200.0));
        assert_eq!(config.age, Some("+5 per 1h".to_string()));
        assert!(config.approvals.is_none());
        assert!(config.size.is_none());
        assert!(config.labels.is_none());
        assert!(config.previously_reviewed.is_none());
    }

    #[test]
    fn test_full_scoring_config_parse() {
        let yaml = r#"
base_score: 100
age: "+1 per 1h"
approvals: "x2 per 1"
size:
  exclude:
    - "*.lock"
    - "package-lock.json"
  buckets:
    - range: "<100"
      effect: "x5"
    - range: ">=500"
      effect: "x0.5"
"#;
        let config: ScoringConfig = serde_saphyr::from_str(yaml).unwrap();
        assert_eq!(config.base_score, Some(100.0));
        assert_eq!(config.age, Some("+1 per 1h".to_string()));
        assert_eq!(config.approvals, Some("x2 per 1".to_string()));

        let size = config.size.unwrap();
        assert_eq!(size.exclude.unwrap().len(), 2);
        assert_eq!(size.buckets.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn test_empty_scoring_config_parse() {
        let yaml = "{}";
        let config: ScoringConfig = serde_saphyr::from_str(yaml).unwrap();
        assert!(config.base_score.is_none());
        assert!(config.age.is_none());
        assert!(config.approvals.is_none());
        assert!(config.size.is_none());
        assert!(config.labels.is_none());
        assert!(config.previously_reviewed.is_none());
    }

    #[test]
    fn test_size_config_without_exclude() {
        let yaml = r#"
buckets:
  - range: "<100"
    effect: "x5"
"#;
        let config: SizeConfig = serde_saphyr::from_str(yaml).unwrap();
        assert!(config.exclude.is_none());
        assert_eq!(config.buckets.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_size_config_exclude_only() {
        let yaml = r#"
exclude:
  - "*.lock"
  - "package-lock.json"
"#;
        let config: SizeConfig = serde_saphyr::from_str(yaml).unwrap();
        assert_eq!(config.exclude.as_ref().unwrap().len(), 2);
        assert!(config.buckets.is_none());
    }

    #[test]
    fn test_labels_config_parse() {
        let yaml = r#"
labels:
  - name: "urgent"
    effect: "+10"
  - name: "wip"
    effect: "x0.5"
"#;
        let config: ScoringConfig = serde_saphyr::from_str(yaml).unwrap();
        let labels = config.labels.unwrap();
        assert_eq!(labels.len(), 2);
        assert_eq!(labels[0].name, "urgent");
        assert_eq!(labels[0].effect, "+10");
        assert_eq!(labels[1].name, "wip");
        assert_eq!(labels[1].effect, "x0.5");
    }

    #[test]
    fn test_previously_reviewed_config_parse() {
        let yaml = r#"
previously_reviewed: "x0.5"
"#;
        let config: ScoringConfig = serde_saphyr::from_str(yaml).unwrap();
        assert_eq!(config.previously_reviewed, Some("x0.5".to_string()));
    }

    #[test]
    fn test_draft_config_parse() {
        let yaml = r#"
draft: "x0.1"
"#;
        let config: ScoringConfig = serde_saphyr::from_str(yaml).unwrap();
        assert_eq!(config.draft, Some("x0.1".to_string()));
    }

    #[test]
    fn test_full_config_with_all_factors() {
        let yaml = r#"
base_score: 100
age: "+1 per 1h"
approvals: "x2 per 1"
size:
  buckets:
    - range: "<100"
      effect: "x5"
labels:
  - name: "urgent"
    effect: "+20"
previously_reviewed: "x0.5"
draft: "x0.1"
"#;
        let config: ScoringConfig = serde_saphyr::from_str(yaml).unwrap();
        assert_eq!(config.base_score, Some(100.0));
        assert_eq!(config.age, Some("+1 per 1h".to_string()));
        assert_eq!(config.approvals, Some("x2 per 1".to_string()));
        assert!(config.size.is_some());
        assert_eq!(config.labels.as_ref().unwrap().len(), 1);
        assert_eq!(config.previously_reviewed, Some("x0.5".to_string()));
        assert_eq!(config.draft, Some("x0.1".to_string()));
    }

    // --- Merge function tests ---

    #[test]
    fn test_merge_no_query_returns_global() {
        let global = ScoringConfig::default();
        let result = merge_scoring_configs(&global, None);
        assert_eq!(result, global);
    }

    #[test]
    fn test_merge_partial_query_preserves_global_fields() {
        let global = ScoringConfig {
            base_score: Some(100.0),
            age: Some("+1 per 1h".to_string()),
            approvals: Some("+10 per 1".to_string()),
            size: Some(SizeConfig {
                exclude: Some(vec!["*.lock".to_string()]),
                buckets: Some(vec![SizeBucket {
                    range: "<100".to_string(),
                    effect: "x5".to_string(),
                }]),
            }),
            labels: Some(vec![LabelEffect {
                name: "urgent".to_string(),
                effect: "+10".to_string(),
            }]),
            previously_reviewed: Some("x0.5".to_string()),
            draft: None,
            since_my_review: None,
        };

        // Query only sets age — everything else should come from global
        let query = ScoringConfig {
            base_score: None,
            age: Some("+5 per 1h".to_string()),
            approvals: None,
            size: None,
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };

        let result = merge_scoring_configs(&global, Some(&query));
        assert_eq!(result.base_score, Some(100.0)); // from global
        assert_eq!(result.age, Some("+5 per 1h".to_string())); // from query
        assert_eq!(result.approvals, Some("+10 per 1".to_string())); // from global
        assert!(result.size.is_some()); // from global
        assert_eq!(
            result.size.as_ref().unwrap().exclude,
            Some(vec!["*.lock".to_string()])
        );
        assert_eq!(result.labels.as_ref().unwrap().len(), 1); // from global
        assert_eq!(result.previously_reviewed, Some("x0.5".to_string())); // from global
    }

    #[test]
    fn test_merge_query_overrides_global() {
        let global = ScoringConfig {
            base_score: Some(100.0),
            age: Some("+1 per 1h".to_string()),
            approvals: None,
            size: None,
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };

        let query = ScoringConfig {
            base_score: Some(200.0),
            age: Some("+5 per 1h".to_string()),
            approvals: None,
            size: None,
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };

        let result = merge_scoring_configs(&global, Some(&query));
        assert_eq!(result.base_score, Some(200.0)); // query override
        assert_eq!(result.age, Some("+5 per 1h".to_string())); // query override
    }

    #[test]
    fn test_merge_size_config_preserves_global_exclude() {
        let global = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: Some(SizeConfig {
                exclude: Some(vec!["*.lock".to_string()]),
                buckets: Some(vec![SizeBucket {
                    range: "<100".to_string(),
                    effect: "x5".to_string(),
                }]),
            }),
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };

        // Query has size with new buckets but no exclude
        let query = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: Some(SizeConfig {
                exclude: None,
                buckets: Some(vec![SizeBucket {
                    range: "<50".to_string(),
                    effect: "x10".to_string(),
                }]),
            }),
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };

        let result = merge_scoring_configs(&global, Some(&query));
        let size = result.size.unwrap();
        // exclude falls through from global
        assert_eq!(size.exclude, Some(vec!["*.lock".to_string()]));
        // buckets from query (explicitly set)
        let buckets = size.buckets.unwrap();
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].range, "<50");
    }

    #[test]
    fn test_merge_size_config_absent_buckets_falls_through() {
        let global = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: Some(SizeConfig {
                exclude: None,
                buckets: Some(vec![SizeBucket {
                    range: "<100".to_string(),
                    effect: "x5".to_string(),
                }]),
            }),
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };

        // Query has size with absent buckets (None = inherit)
        let query = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: Some(SizeConfig {
                exclude: None,
                buckets: None,
            }),
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };

        let result = merge_scoring_configs(&global, Some(&query));
        let size = result.size.unwrap();
        // Absent buckets (None) fall through to global
        let buckets = size.buckets.unwrap();
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].range, "<100");
    }

    #[test]
    fn test_merge_size_config_query_exclude_overrides_global() {
        let global = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: Some(SizeConfig {
                exclude: Some(vec!["*.lock".to_string()]),
                buckets: None,
            }),
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };

        let query = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: Some(SizeConfig {
                exclude: Some(vec!["*.json".to_string()]),
                buckets: None,
            }),
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };

        let result = merge_scoring_configs(&global, Some(&query));
        let size = result.size.unwrap();
        // Query exclude overrides global
        assert_eq!(size.exclude, Some(vec!["*.json".to_string()]));
    }

    #[test]
    fn test_merge_all_none_query() {
        let global = ScoringConfig::default();

        // Query with all fields None
        let query = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: None,
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };

        let result = merge_scoring_configs(&global, Some(&query));
        // Should behave same as no query — returns global values
        assert_eq!(result, global);
    }

    // --- Leaf-level size merge tests ---

    #[test]
    fn test_merge_size_exclude_inherits_global_buckets() {
        // Query has only size.exclude, global has size.buckets
        let global = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: Some(SizeConfig {
                exclude: None,
                buckets: Some(vec![SizeBucket {
                    range: "<100".to_string(),
                    effect: "x5".to_string(),
                }]),
            }),
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };

        let query = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: Some(SizeConfig {
                exclude: Some(vec!["*.lock".to_string()]),
                buckets: None, // absent = inherit
            }),
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };

        let result = merge_scoring_configs(&global, Some(&query));
        let size = result.size.unwrap();
        // exclude from query
        assert_eq!(size.exclude, Some(vec!["*.lock".to_string()]));
        // buckets inherited from global
        let buckets = size.buckets.unwrap();
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].range, "<100");
    }

    #[test]
    fn test_merge_size_buckets_inherits_global_exclude() {
        // Query has only size.buckets, global has size.exclude
        let global = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: Some(SizeConfig {
                exclude: Some(vec!["*.lock".to_string()]),
                buckets: None,
            }),
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };

        let query = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: Some(SizeConfig {
                exclude: None, // absent = inherit
                buckets: Some(vec![SizeBucket {
                    range: "<200".to_string(),
                    effect: "x3".to_string(),
                }]),
            }),
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };

        let result = merge_scoring_configs(&global, Some(&query));
        let size = result.size.unwrap();
        // exclude inherited from global
        assert_eq!(size.exclude, Some(vec!["*.lock".to_string()]));
        // buckets from query
        let buckets = size.buckets.unwrap();
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].range, "<200");
    }

    // --- Label merge tests ---

    #[test]
    fn test_merge_labels_by_name_query_wins() {
        let global = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: None,
            labels: Some(vec![LabelEffect {
                name: "foo".to_string(),
                effect: "x3".to_string(),
            }]),
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };

        let query = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: None,
            labels: Some(vec![LabelEffect {
                name: "foo".to_string(),
                effect: "x2".to_string(),
            }]),
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };

        let result = merge_scoring_configs(&global, Some(&query));
        let labels = result.labels.unwrap();
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].name, "foo");
        assert_eq!(labels[0].effect, "x2"); // query wins
    }

    #[test]
    fn test_merge_labels_preserves_unmentioned_global() {
        let global = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: None,
            labels: Some(vec![
                LabelEffect {
                    name: "foo".to_string(),
                    effect: "+5".to_string(),
                },
                LabelEffect {
                    name: "bar".to_string(),
                    effect: "+10".to_string(),
                },
            ]),
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };

        let query = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: None,
            labels: Some(vec![LabelEffect {
                name: "foo".to_string(),
                effect: "+20".to_string(),
            }]),
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };

        let result = merge_scoring_configs(&global, Some(&query));
        let labels = result.labels.unwrap();
        assert_eq!(labels.len(), 2);
        // Use find to avoid order dependence (HashMap)
        let foo = labels.iter().find(|l| l.name == "foo").unwrap();
        assert_eq!(foo.effect, "+20"); // from query
        let bar = labels.iter().find(|l| l.name == "bar").unwrap();
        assert_eq!(bar.effect, "+10"); // preserved from global
    }

    #[test]
    fn test_merge_labels_case_insensitive() {
        let global = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: None,
            labels: Some(vec![LabelEffect {
                name: "Urgent".to_string(),
                effect: "+10".to_string(),
            }]),
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };

        let query = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: None,
            labels: Some(vec![LabelEffect {
                name: "urgent".to_string(),
                effect: "+20".to_string(),
            }]),
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };

        let result = merge_scoring_configs(&global, Some(&query));
        let labels = result.labels.unwrap();
        // Only one entry — case-insensitive dedup
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].name, "urgent"); // query case preserved
        assert_eq!(labels[0].effect, "+20"); // query value wins
    }

    #[test]
    fn test_merge_labels_no_global() {
        let global = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: None,
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };

        let query = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: None,
            labels: Some(vec![LabelEffect {
                name: "foo".to_string(),
                effect: "+5".to_string(),
            }]),
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };

        let result = merge_scoring_configs(&global, Some(&query));
        let labels = result.labels.unwrap();
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].name, "foo");
    }

    #[test]
    fn test_merge_labels_no_query() {
        let global = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: None,
            labels: Some(vec![LabelEffect {
                name: "bar".to_string(),
                effect: "+10".to_string(),
            }]),
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };

        let query = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: None,
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };

        let result = merge_scoring_configs(&global, Some(&query));
        let labels = result.labels.unwrap();
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].name, "bar");
    }
}

use super::config::{ScoringConfig, SizeBucket};
use super::factors::{Effect, RangeOp};
use anyhow::Result;

/// Validate scoring configuration at startup.
/// Returns all validation errors at once (not just the first).
pub fn validate_scoring(config: &ScoringConfig) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();

    // Validate base_score
    if let Some(base) = config.base_score {
        if base < 0.0 {
            errors.push("scoring.base_score: must be non-negative".to_string());
        }
    }

    // Validate age factor syntax
    if let Some(ref age) = config.age {
        if let Err(e) = Effect::parse(age) {
            errors.push(format!("scoring.age: invalid format '{}' - {}", age, e));
        }
    }

    // Validate approvals effect string
    if let Some(ref approvals) = config.approvals {
        // For approvals, "per N" means "per N approvals", not per time unit
        // Convert formats like "+10 per 1" to "+10 per 1sec" for parsing validation
        let parseable_str = if let Some((effect_part, per_part)) = approvals.split_once(" per ") {
            // Check if per_part is just a number (no time unit)
            if per_part.trim().chars().all(|c| c.is_numeric() || c == '.') {
                format!("{} per 1sec", effect_part)
            } else {
                approvals.clone()
            }
        } else {
            approvals.clone()
        };

        if let Err(e) = Effect::parse(&parseable_str) {
            errors.push(format!(
                "scoring.approvals: invalid format '{}' - {}",
                approvals, e
            ));
        }
    }

    // Validate size buckets
    if let Some(ref size_config) = config.size {
        if let Some(ref buckets) = size_config.buckets {
            for (i, bucket) in buckets.iter().enumerate() {
                if let Err(e) = RangeOp::parse(&bucket.range) {
                    errors.push(format!(
                        "scoring.size.buckets[{}].range: invalid '{}' - {}",
                        i, bucket.range, e
                    ));
                }
                if let Err(e) = Effect::parse(&bucket.effect) {
                    errors.push(format!(
                        "scoring.size.buckets[{}].effect: invalid '{}' - {}",
                        i, bucket.effect, e
                    ));
                }
            }

            // Check for overlapping ranges
            if let Err(e) = check_bucket_overlaps(buckets) {
                errors.push(e);
            }
        }

        // Validate size exclude patterns
        if let Some(ref excludes) = size_config.exclude {
            for (i, pattern) in excludes.iter().enumerate() {
                if let Err(e) = glob::Pattern::new(pattern) {
                    errors.push(format!(
                        "scoring.size.exclude[{}]: invalid glob pattern '{}' - {}",
                        i, pattern, e
                    ));
                }
            }
        }
    }

    // Validate label effects
    if let Some(ref labels) = config.labels {
        for (i, label_effect) in labels.iter().enumerate() {
            if label_effect.name.trim().is_empty() {
                errors.push(format!("scoring.labels[{}].name: must not be empty", i));
            }
            if let Err(e) = Effect::parse(&label_effect.effect) {
                errors.push(format!(
                    "scoring.labels[{}].effect: invalid '{}' - {}",
                    i, label_effect.effect, e
                ));
            }
        }
    }

    // Validate previously_reviewed effect
    if let Some(ref reviewed) = config.previously_reviewed {
        if let Err(e) = Effect::parse(reviewed) {
            errors.push(format!(
                "scoring.previously_reviewed: invalid '{}' - {}",
                reviewed, e
            ));
        }
    }

    // Validate draft effect
    if let Some(ref draft) = config.draft {
        if let Err(e) = Effect::parse(draft) {
            errors.push(format!("scoring.draft: invalid '{}' - {}", draft, e));
        }
    }

    // Validate since_my_review effects
    if let Some(ref smr) = config.since_my_review {
        let fields = [
            ("pushed", &smr.pushed),
            ("mentioned", &smr.mentioned),
            ("review_requested", &smr.review_requested),
            ("awaiting_author", &smr.awaiting_author),
        ];
        for (name, value) in fields {
            if let Some(effect) = value {
                if let Err(e) = Effect::parse(effect) {
                    errors.push(format!(
                        "scoring.since_my_review.{}: invalid '{}' - {}",
                        name, effect, e
                    ));
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn check_bucket_overlaps(buckets: &[SizeBucket]) -> Result<(), String> {
    for i in 0..buckets.len() {
        for j in (i + 1)..buckets.len() {
            let r1 =
                RangeOp::parse(&buckets[i].range).map_err(|e| format!("bucket[{}]: {}", i, e))?;
            let r2 =
                RangeOp::parse(&buckets[j].range).map_err(|e| format!("bucket[{}]: {}", j, e))?;

            if do_ranges_overlap(&r1, &r2) {
                return Err(format!(
                    "scoring.size.buckets: overlapping ranges at buckets[{}] ('{}') and buckets[{}] ('{}')",
                    i, buckets[i].range, j, buckets[j].range
                ));
            }
        }
    }
    Ok(())
}

fn do_ranges_overlap(r1: &RangeOp, r2: &RangeOp) -> bool {
    // Check if any value could match both ranges
    // Need exhaustive case matching for all RangeOp variants
    use RangeOp::*;

    match (r1, r2) {
        (Equal(a), Equal(b)) => *a == *b,
        (Equal(n), Between(low, high)) | (Between(low, high), Equal(n)) => {
            *n >= *low && *n <= *high
        }
        (Equal(n), LessThan(max)) | (LessThan(max), Equal(n)) => *n < *max,
        (Equal(n), LessEqual(max)) | (LessEqual(max), Equal(n)) => *n <= *max,
        (Equal(n), GreaterThan(min)) | (GreaterThan(min), Equal(n)) => *n > *min,
        (Equal(n), GreaterEqual(min)) | (GreaterEqual(min), Equal(n)) => *n >= *min,

        (Between(l1, h1), Between(l2, h2)) => {
            // Ranges overlap if one's low is in other's range, or vice versa
            (*l1 >= *l2 && *l1 <= *h2) || (*l2 >= *l1 && *l2 <= *h1)
        }

        (LessThan(_), LessThan(_)) | (LessEqual(_), LessEqual(_)) => true,
        (LessThan(_), LessEqual(_)) | (LessEqual(_), LessThan(_)) => true,

        (GreaterThan(_), GreaterThan(_)) | (GreaterEqual(_), GreaterEqual(_)) => true,
        (GreaterThan(_), GreaterEqual(_)) | (GreaterEqual(_), GreaterThan(_)) => true,

        (LessThan(max), Between(low, _)) | (Between(low, _), LessThan(max)) => *low < *max,
        (LessEqual(max), Between(low, _)) | (Between(low, _), LessEqual(max)) => *low <= *max,
        (GreaterThan(min), Between(_, high)) | (Between(_, high), GreaterThan(min)) => *high > *min,
        (GreaterEqual(min), Between(_, high)) | (Between(_, high), GreaterEqual(min)) => {
            *high >= *min
        }

        (LessThan(max), GreaterThan(min)) | (GreaterThan(min), LessThan(max)) => *max > *min + 1,
        (LessThan(max), GreaterEqual(min)) | (GreaterEqual(min), LessThan(max)) => *max > *min,
        (LessEqual(max), GreaterThan(min)) | (GreaterThan(min), LessEqual(max)) => *max > *min,
        (LessEqual(max), GreaterEqual(min)) | (GreaterEqual(min), LessEqual(max)) => *max >= *min,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scoring::{LabelEffect, SizeBucket, SizeConfig};

    #[test]
    fn test_valid_config() {
        let config = ScoringConfig {
            base_score: Some(100.0),
            age: Some("+1 per 1h".to_string()),
            approvals: Some("x0.5".to_string()),
            size: None,
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };
        assert!(validate_scoring(&config).is_ok());
    }

    #[test]
    fn test_empty_config() {
        let config = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: None,
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };
        assert!(validate_scoring(&config).is_ok());
    }

    #[test]
    fn test_invalid_age_format() {
        let config = ScoringConfig {
            base_score: None,
            age: Some("invalid".to_string()),
            approvals: None,
            size: None,
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };
        let result = validate_scoring(&config);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors[0].contains("scoring.age"));
    }

    #[test]
    fn test_negative_base_score() {
        let config = ScoringConfig {
            base_score: Some(-10.0),
            age: None,
            approvals: None,
            size: None,
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };
        let result = validate_scoring(&config);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors[0].contains("base_score"));
    }

    #[test]
    fn test_invalid_approval_effect() {
        let config = ScoringConfig {
            base_score: None,
            age: None,
            approvals: Some("invalid".to_string()),
            size: None,
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };
        let result = validate_scoring(&config);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors[0].contains("scoring.approvals"));
    }

    #[test]
    fn test_invalid_size_bucket() {
        let config = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: Some(SizeConfig {
                exclude: None,
                buckets: Some(vec![SizeBucket {
                    range: "<100".to_string(),
                    effect: "bad".to_string(),
                }]),
            }),
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };
        let result = validate_scoring(&config);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors[0].contains("scoring.size.buckets[0].effect"));
    }

    #[test]
    fn test_collects_all_errors() {
        let config = ScoringConfig {
            base_score: Some(-10.0),      // Error 1
            age: Some("bad".to_string()), // Error 2
            approvals: None,
            size: None,
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };
        let result = validate_scoring(&config);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn test_default_config_passes_validation() {
        let config = ScoringConfig::default();
        match validate_scoring(&config) {
            Ok(_) => {}
            Err(errors) => {
                eprintln!("Default config validation failed: {:?}", errors);
                panic!("Default config should pass validation");
            }
        }
    }

    #[test]
    fn test_no_overlap_exclusive_boundary() {
        let config = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: Some(SizeConfig {
                exclude: None,
                buckets: Some(vec![
                    SizeBucket {
                        range: "<100".to_string(),
                        effect: "x5".to_string(),
                    },
                    SizeBucket {
                        range: ">=100".to_string(),
                        effect: "x1".to_string(),
                    },
                ]),
            }),
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };
        assert!(validate_scoring(&config).is_ok());
    }

    #[test]
    fn test_overlap_at_boundary() {
        let config = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: Some(SizeConfig {
                exclude: None,
                buckets: Some(vec![
                    SizeBucket {
                        range: "<=100".to_string(),
                        effect: "x5".to_string(),
                    },
                    SizeBucket {
                        range: ">=100".to_string(),
                        effect: "x1".to_string(),
                    },
                ]),
            }),
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };
        let result = validate_scoring(&config);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors[0].contains("overlapping ranges"));
        assert!(errors[0].contains("<=100"));
        assert!(errors[0].contains(">=100"));
    }

    #[test]
    fn test_overlap_between_ranges() {
        let config = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: Some(SizeConfig {
                exclude: None,
                buckets: Some(vec![
                    SizeBucket {
                        range: "100-500".to_string(),
                        effect: "x2".to_string(),
                    },
                    SizeBucket {
                        range: "300-700".to_string(),
                        effect: "x1".to_string(),
                    },
                ]),
            }),
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };
        let result = validate_scoring(&config);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors[0].contains("overlapping ranges"));
    }

    #[test]
    fn test_no_overlap_between_ranges() {
        let config = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: Some(SizeConfig {
                exclude: None,
                buckets: Some(vec![
                    SizeBucket {
                        range: "100-200".to_string(),
                        effect: "x2".to_string(),
                    },
                    SizeBucket {
                        range: "300-400".to_string(),
                        effect: "x1".to_string(),
                    },
                ]),
            }),
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };
        assert!(validate_scoring(&config).is_ok());
    }

    #[test]
    fn test_equal_in_between_overlaps() {
        let config = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: Some(SizeConfig {
                exclude: None,
                buckets: Some(vec![
                    SizeBucket {
                        range: "150".to_string(),
                        effect: "x2".to_string(),
                    },
                    SizeBucket {
                        range: "100-200".to_string(),
                        effect: "x1".to_string(),
                    },
                ]),
            }),
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };
        let result = validate_scoring(&config);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors[0].contains("overlapping ranges"));
    }

    #[test]
    fn test_same_direction_always_overlaps() {
        let config = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: Some(SizeConfig {
                exclude: None,
                buckets: Some(vec![
                    SizeBucket {
                        range: "<100".to_string(),
                        effect: "x2".to_string(),
                    },
                    SizeBucket {
                        range: "<200".to_string(),
                        effect: "x1".to_string(),
                    },
                ]),
            }),
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };
        let result = validate_scoring(&config);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors[0].contains("overlapping ranges"));
    }

    #[test]
    fn test_greater_vs_less_no_overlap() {
        let config = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: Some(SizeConfig {
                exclude: None,
                buckets: Some(vec![
                    SizeBucket {
                        range: ">200".to_string(),
                        effect: "x2".to_string(),
                    },
                    SizeBucket {
                        range: "<100".to_string(),
                        effect: "x1".to_string(),
                    },
                ]),
            }),
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };
        assert!(validate_scoring(&config).is_ok());
    }

    #[test]
    fn test_valid_label_config() {
        let config = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: None,
            labels: Some(vec![
                LabelEffect {
                    name: "urgent".to_string(),
                    effect: "+10".to_string(),
                },
                LabelEffect {
                    name: "wip".to_string(),
                    effect: "x0.5".to_string(),
                },
            ]),
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };
        assert!(validate_scoring(&config).is_ok());
    }

    #[test]
    fn test_invalid_label_effect() {
        let config = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: None,
            labels: Some(vec![LabelEffect {
                name: "urgent".to_string(),
                effect: "bad".to_string(),
            }]),
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };
        let result = validate_scoring(&config);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors[0].contains("scoring.labels[0].effect"));
        assert!(errors[0].contains("bad"));
    }

    #[test]
    fn test_empty_label_name() {
        let config = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: None,
            labels: Some(vec![LabelEffect {
                name: "  ".to_string(),
                effect: "+10".to_string(),
            }]),
            previously_reviewed: None,
            draft: None,
            since_my_review: None,
        };
        let result = validate_scoring(&config);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors[0].contains("scoring.labels[0].name"));
        assert!(errors[0].contains("must not be empty"));
    }

    #[test]
    fn test_valid_previously_reviewed() {
        let config = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: None,
            labels: None,
            previously_reviewed: Some("x0.5".to_string()),
            draft: None,
            since_my_review: None,
        };
        assert!(validate_scoring(&config).is_ok());
    }

    #[test]
    fn test_invalid_previously_reviewed() {
        let config = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: None,
            labels: None,
            previously_reviewed: Some("invalid".to_string()),
            draft: None,
            since_my_review: None,
        };
        let result = validate_scoring(&config);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors[0].contains("scoring.previously_reviewed"));
        assert!(errors[0].contains("invalid"));
    }

    #[test]
    fn test_valid_exclude_patterns() {
        let config = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: Some(SizeConfig {
                exclude: Some(vec!["*.lock".to_string(), "*.json".to_string()]),
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
        assert!(validate_scoring(&config).is_ok());
    }

    #[test]
    fn test_invalid_exclude_pattern() {
        let config = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: Some(SizeConfig {
                exclude: Some(vec!["[invalid".to_string()]),
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
        let result = validate_scoring(&config);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors[0].contains("scoring.size.exclude[0]"));
        assert!(errors[0].contains("[invalid"));
    }

    #[test]
    fn test_exclude_patterns_validated_with_other_errors() {
        let config = ScoringConfig {
            base_score: Some(-10.0), // Error 1
            age: None,
            approvals: None,
            size: Some(SizeConfig {
                exclude: Some(vec!["[bad".to_string()]), // Error 2
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
        let result = validate_scoring(&config);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 2);
        assert!(errors.iter().any(|e| e.contains("base_score")));
        assert!(errors.iter().any(|e| e.contains("scoring.size.exclude[0]")));
    }

    #[test]
    fn test_multiple_validation_errors_with_new_fields() {
        let config = ScoringConfig {
            base_score: Some(-10.0),      // Error 1
            age: Some("bad".to_string()), // Error 2
            approvals: None,
            size: None,
            labels: Some(vec![
                LabelEffect {
                    name: "".to_string(),
                    effect: "bad".to_string(),
                }, // Error 3 & 4
            ]),
            previously_reviewed: Some("invalid".to_string()), // Error 5
            draft: None,
            since_my_review: None,
        };
        let result = validate_scoring(&config);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 5);
    }

    // LOCKED: regression for since-my-review review workflow (feat/since-my-review).
    // Valid since_my_review effects must pass startup validation.
    #[test]
    fn test_valid_since_my_review() {
        let config = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: None,
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: Some(crate::scoring::SinceMyReviewScoring {
                pushed: Some("x5".to_string()),
                mentioned: Some("x4".to_string()),
                review_requested: Some("x3".to_string()),
                awaiting_author: Some("x0.2".to_string()),
            }),
        };
        assert!(validate_scoring(&config).is_ok());
    }

    // LOCKED: regression for since-my-review review workflow (feat/since-my-review).
    // Invalid since_my_review effects must fail at startup.
    #[test]
    fn test_invalid_since_my_review_effects() {
        let config = ScoringConfig {
            base_score: None,
            age: None,
            approvals: None,
            size: None,
            labels: None,
            previously_reviewed: None,
            draft: None,
            since_my_review: Some(crate::scoring::SinceMyReviewScoring {
                pushed: Some("bad".to_string()),
                mentioned: None,
                review_requested: None,
                awaiting_author: Some("also bad".to_string()),
            }),
        };
        let result = validate_scoring(&config);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 2);
        assert!(errors
            .iter()
            .any(|e| e.contains("scoring.since_my_review.pushed") && e.contains("bad")));
        assert!(errors
            .iter()
            .any(|e| e.contains("scoring.since_my_review.awaiting_author")));
    }
}

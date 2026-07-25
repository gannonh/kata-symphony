//! Human route correction measurement.
//!
//! After Symphony publishes a route, a maintainer may disagree and swap the
//! route label by hand. Comparing the labels currently on the issue against the
//! five labels recorded on that publication turns the disagreement into a
//! measurable signal.
//!
//! The comparison always uses the mapping recorded on the publication intent,
//! never the live configuration, so reloading `WORKFLOW.md` with a different
//! route mapping cannot retroactively reinterpret an older publication.
//!
//! Correction is a proxy for disagreement. Absence of a correction does not
//! prove the route was right.

/// Canonical routes in the order an automatic publication intent records its
/// route labels.
pub const ROUTE_KEYS: [&str; 5] = [
    "implement",
    "spec",
    "needs_information",
    "park",
    "human_owned",
];

/// Outcome of comparing an issue's current labels with a published route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteAgreement {
    /// The published route label is still the only managed route label present.
    Agreed,
    /// Exactly one managed route label is present and it is a different route.
    Corrected { route: String, label: String },
    /// Zero or several managed route labels are present, so the issue's route
    /// is ambiguous. This is a diagnostic, not an agreement observation.
    Inconsistent { observed: Vec<String> },
}

/// Compare an issue's current labels against a recorded publication.
///
/// `recorded_labels` holds the five route labels in [`ROUTE_KEYS`] order as
/// captured when the route was published.
pub fn compare_route_labels(
    published_route: &str,
    recorded_labels: &[String],
    current_labels: &[String],
) -> RouteAgreement {
    if recorded_labels.len() != ROUTE_KEYS.len() {
        return RouteAgreement::Inconsistent {
            observed: Vec::new(),
        };
    }

    let current: Vec<String> = current_labels
        .iter()
        .map(|label| normalize(label))
        .collect();
    let mut present: Vec<(usize, String)> = Vec::new();
    for (index, recorded) in recorded_labels.iter().enumerate() {
        let recorded = normalize(recorded);
        if recorded.is_empty() {
            continue;
        }
        // A route mapped to the same label twice must not count twice.
        if present.iter().any(|(_, label)| *label == recorded) {
            continue;
        }
        if current.contains(&recorded) {
            present.push((index, recorded));
        }
    }

    match present.as_slice() {
        [(index, label)] => {
            let route = ROUTE_KEYS[*index];
            if route.eq_ignore_ascii_case(published_route.trim()) {
                RouteAgreement::Agreed
            } else {
                RouteAgreement::Corrected {
                    route: route.to_string(),
                    label: label.clone(),
                }
            }
        }
        observed => RouteAgreement::Inconsistent {
            observed: observed.iter().map(|(_, label)| label.clone()).collect(),
        },
    }
}

fn normalize(label: &str) -> String {
    label.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recorded() -> Vec<String> {
        vec![
            "ready-for-agent".to_string(),
            "ready-to-spec".to_string(),
            "needs-info".to_string(),
            "wait-to-implement".to_string(),
            "ready-for-human".to_string(),
        ]
    }

    /// The published label still standing alone means the maintainer left the
    /// decision in place, which must not be counted as disagreement.
    #[test]
    fn published_label_left_in_place_is_agreement() {
        let agreement = compare_route_labels(
            "implement",
            &recorded(),
            &["ready-for-agent".to_string(), "bug".to_string()],
        );

        assert_eq!(agreement, RouteAgreement::Agreed);
    }

    /// A maintainer swapping the route label is the disagreement signal A1
    /// exists to measure.
    #[test]
    fn swapped_route_label_is_a_correction() {
        let agreement = compare_route_labels(
            "implement",
            &recorded(),
            &["needs-info".to_string(), "bug".to_string()],
        );

        assert_eq!(
            agreement,
            RouteAgreement::Corrected {
                route: "needs_information".to_string(),
                label: "needs-info".to_string(),
            }
        );
    }

    /// Label comparison must survive GitHub casing and padding differences,
    /// otherwise a cosmetic difference would read as a route change.
    #[test]
    fn label_comparison_ignores_case_and_padding() {
        let agreement = compare_route_labels(
            "implement",
            &recorded(),
            &["  Ready-For-Agent ".to_string()],
        );

        assert_eq!(agreement, RouteAgreement::Agreed);
    }

    /// Two route labels at once means the issue has no single route, so calling
    /// either one a correction would invent a decision nobody made.
    #[test]
    fn multiple_route_labels_are_inconsistent_not_a_correction() {
        let agreement = compare_route_labels(
            "implement",
            &recorded(),
            &["ready-for-agent".to_string(), "needs-info".to_string()],
        );

        assert_eq!(
            agreement,
            RouteAgreement::Inconsistent {
                observed: vec!["ready-for-agent".to_string(), "needs-info".to_string()],
            }
        );
    }

    /// Every route label removed is equally ambiguous: the route was erased,
    /// not corrected to something else.
    #[test]
    fn no_route_label_is_inconsistent() {
        let agreement = compare_route_labels("implement", &recorded(), &["bug".to_string()]);

        assert_eq!(
            agreement,
            RouteAgreement::Inconsistent {
                observed: Vec::new(),
            }
        );
    }
}

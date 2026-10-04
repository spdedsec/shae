pub fn closest(target: &str, candidates: &[String]) -> Option<String> {
    if candidates.is_empty() {
        return None;
    }

    let threshold = 1.max(target.len() / 3).min(3);

    let mut best_match: Option<&String> = None;
    let mut best_dist = usize::MAX;

    for candidate in candidates {
        let dist = strsim::osa_distance(target, candidate);
        if dist <= threshold && dist < best_dist {
            best_dist = dist;
            best_match = Some(candidate);
        }
    }

    best_match.cloned()
}

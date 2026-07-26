use std::collections::{BTreeSet, HashMap};

pub(crate) fn copy_present_keys(
    target: &mut HashMap<String, String>,
    source: &HashMap<String, String>,
    keys: &[&str],
) {
    for key in keys {
        if let Some(value) = source.get(*key) {
            target.insert((*key).to_string(), value.clone());
        }
    }
}

pub(crate) fn metadata_has_any(metadata: &HashMap<String, String>, keys: &[&str]) -> bool {
    keys.iter().any(|key| metadata.contains_key(*key))
}

pub(crate) fn metadata_has_all(metadata: &HashMap<String, String>, keys: &[&str]) -> bool {
    keys.iter().all(|key| metadata.contains_key(*key))
}

pub(crate) fn metadata_is_true(metadata: &HashMap<String, String>, key: &str) -> bool {
    metadata.get(key).map(String::as_str) == Some("true")
}

pub(crate) fn metadata_matches(
    metadata: &HashMap<String, String>,
    key: &str,
    expected: &str,
) -> bool {
    metadata.get(key).map(String::as_str) == Some(expected)
}

pub(crate) fn insert_joined_set(
    metadata: &mut HashMap<String, String>,
    key: &str,
    values: &BTreeSet<String>,
) {
    if !values.is_empty() {
        metadata.insert(
            key.to_string(),
            values.iter().cloned().collect::<Vec<_>>().join(","),
        );
    }
}

pub(crate) fn capture_trimmed_value_from_note(
    note: &str,
    prefix: &str,
    target: &mut Option<String>,
) -> bool {
    if let Some(value) = note.strip_prefix(prefix) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            *target = Some(trimmed.to_string());
            return true;
        }
    }
    false
}

pub(crate) fn capture_trimmed_pair_from_note(
    note: &str,
    prefix: &str,
    left: &mut Option<String>,
    right: &mut Option<String>,
) -> bool {
    if let Some(value) = note.strip_prefix(prefix) {
        if let Some((left_value, right_value)) = value.split_once(':') {
            let left_trimmed = left_value.trim();
            let right_trimmed = right_value.trim();
            if !left_trimmed.is_empty() {
                *left = Some(left_trimmed.to_string());
            }
            if !right_trimmed.is_empty() {
                *right = Some(right_trimmed.to_string());
            }
            return !left_trimmed.is_empty() || !right_trimmed.is_empty();
        }
    }
    false
}

pub(crate) fn collect_trimmed_set_value_from_note(
    note: &str,
    prefix: &str,
    target: &mut BTreeSet<String>,
) -> bool {
    if let Some(value) = note.strip_prefix(prefix) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            target.insert(trimmed.to_string());
            return true;
        }
    }
    false
}

pub(crate) fn collect_trimmed_set_values_from_note(
    note: &str,
    mappings: &mut [(&str, &mut BTreeSet<String>)],
) -> bool {
    mappings
        .iter_mut()
        .any(|(prefix, target)| collect_trimmed_set_value_from_note(note, prefix, *target))
}

pub(crate) fn collect_flagged_trimmed_set_value_from_note(
    note: &str,
    prefix: &str,
    target: &mut BTreeSet<String>,
    flag: &mut bool,
) -> bool {
    if collect_trimmed_set_value_from_note(note, prefix, target) {
        *flag = true;
        return true;
    }
    false
}

pub(crate) fn collect_flagged_trimmed_set_values_from_note(
    note: &str,
    mappings: &mut [(&str, &mut BTreeSet<String>, &mut bool)],
) -> bool {
    mappings.iter_mut().any(|(prefix, target, flag)| {
        collect_flagged_trimmed_set_value_from_note(note, prefix, *target, *flag)
    })
}

pub(crate) fn collect_flagged_trimmed_set_values_from_note_with_shared_flag(
    note: &str,
    flag: &mut bool,
    mappings: &mut [(&str, &mut BTreeSet<String>)],
) -> bool {
    mappings.iter_mut().any(|(prefix, target)| {
        collect_flagged_trimmed_set_value_from_note(note, prefix, *target, flag)
    })
}

pub(crate) fn collect_csv_set_values_from_note(
    note: &str,
    prefix: &str,
    target: &mut BTreeSet<String>,
) -> bool {
    if let Some(value) = note.strip_prefix(prefix) {
        let mut matched = false;
        for item in value.split(',') {
            let trimmed = item.trim();
            if !trimmed.is_empty() {
                target.insert(trimmed.to_string());
                matched = true;
            }
        }
        return matched;
    }
    false
}

pub(crate) fn collect_csv_set_values_from_note_map(
    note: &str,
    mappings: &mut [(&str, &mut BTreeSet<String>)],
) -> bool {
    mappings
        .iter_mut()
        .any(|(prefix, target)| collect_csv_set_values_from_note(note, prefix, *target))
}

pub(crate) fn parse_colon_fields(surface: &str) -> HashMap<String, String> {
    surface
        .split(':')
        .filter_map(|part| {
            part.split_once('=')
                .map(|(key, value)| (key.to_string(), value.to_string()))
        })
        .collect()
}

pub(crate) fn surface_has_nonempty_fields(fields: &HashMap<String, String>, keys: &[&str]) -> bool {
    keys.iter()
        .all(|key| fields.get(*key).map(|value| !value.trim().is_empty()) == Some(true))
}

pub(crate) fn surface_has_true_fields(fields: &HashMap<String, String>, keys: &[&str]) -> bool {
    keys.iter()
        .all(|key| fields.get(*key).map(String::as_str) == Some("true"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_trimmed_pair_from_note_parses_provider_pair() {
        let mut left = None;
        let mut right = None;

        let matched = capture_trimmed_pair_from_note(
            "after_llm:provider:openai:gpt-5",
            "after_llm:provider:",
            &mut left,
            &mut right,
        );

        assert!(matched);
        assert_eq!(left.as_deref(), Some("openai"));
        assert_eq!(right.as_deref(), Some("gpt-5"));
    }

    #[test]
    fn collect_csv_and_flagged_batches_share_helpers() {
        let mut csv_values = BTreeSet::new();
        let mut flagged_values = BTreeSet::new();
        let mut present = false;

        assert!(collect_csv_set_values_from_note_map(
            "prefix:a,b",
            &mut [("prefix:", &mut csv_values)],
        ));
        assert!(
            collect_flagged_trimmed_set_values_from_note_with_shared_flag(
                "flag:value",
                &mut present,
                &mut [("flag:", &mut flagged_values)],
            )
        );

        assert_eq!(
            csv_values.into_iter().collect::<Vec<_>>(),
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(
            flagged_values.into_iter().collect::<Vec<_>>(),
            vec!["value".to_string()]
        );
        assert!(present);
    }
}

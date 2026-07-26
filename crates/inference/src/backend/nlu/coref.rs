use benshu_infra::traits::nlu::{DialogueContext, EntityReference, ReferenceType};
use std::collections::HashMap;

/// CoreferenceResolver implements heuristic-based entity resolution.
/// It follows the "Most Recent Mention" (MRM) pattern for pronouns
/// and "Semantic Match" for definite nouns.
pub struct CoreferenceResolver {
    /// Mapping of common pronouns to their expected entity types (optional hint)
    pronoun_map: HashMap<&'static str, Vec<&'static str>>,
}

impl Default for CoreferenceResolver {
    fn default() -> Self {
        let mut p_map = HashMap::new();
        p_map.insert("he", vec!["person", "user"]);
        p_map.insert("him", vec!["person", "user"]);
        p_map.insert("she", vec!["person"]);
        p_map.insert("her", vec!["person"]);
        p_map.insert("it", vec!["file", "artifact", "task", "object"]);
        p_map.insert("they", vec!["group", "organization"]);

        Self { pronoun_map: p_map }
    }
}

impl CoreferenceResolver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolves references in a given text using dialogue context.
    /// This is a high-performance, non-blocking synchronous call.
    pub fn resolve(&self, text: &str, context: &DialogueContext) -> Vec<EntityReference> {
        let mut refs = Vec::new();
        let words = tokenize_with_spans(text);

        for (i, word) in words.iter().enumerate() {
            let clean_word = word
                .text
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase();

            // 1. Pronoun Resolution
            if self.is_pronoun(&clean_word) {
                if let Some(resolved) = self.resolve_pronoun(&clean_word, context) {
                    refs.push(EntityReference {
                        surface_form: word.text.to_string(),
                        resolved_entity_id: resolved,
                        confidence: 0.8, // Heuristic confidence
                        reference_type: ReferenceType::Pronoun,
                        start: word.start,
                        end: word.end,
                    });
                }
            }

            // 2. Definite Noun Resolution ("the file", "that task")
            if (clean_word == "the" || clean_word == "that" || clean_word == "this")
                && i + 1 < words.len()
            {
                let next_word = words[i + 1]
                    .text
                    .trim_matches(|c: char| !c.is_alphanumeric())
                    .to_lowercase();
                if let Some(resolved) = self.resolve_definite_noun(&next_word, context) {
                    refs.push(EntityReference {
                        surface_form: format!("{} {}", word.text, words[i + 1].text),
                        resolved_entity_id: resolved,
                        confidence: 0.9,
                        reference_type: ReferenceType::DefiniteNoun,
                        start: word.start,
                        end: words[i + 1].end,
                    });
                }
            }
        }

        refs
    }

    fn is_pronoun(&self, word: &str) -> bool {
        self.pronoun_map.contains_key(word)
    }

    fn resolve_pronoun(&self, pronoun: &str, context: &DialogueContext) -> Option<String> {
        let target_types = self.pronoun_map.get(pronoun)?;

        // Scan recent entities from newest to oldest
        for entity_id in context.recent_entities.iter().rev() {
            // Check if entity type matches any of target_types
            // Format: "type:id"
            if let Some(colon_idx) = entity_id.find(':') {
                let e_type = &entity_id[..colon_idx];
                if target_types.contains(&e_type) {
                    return Some(entity_id.clone());
                }
            }

            // Generic fallback for "it" to the very last entity
            if pronoun == "it" && !context.recent_entities.is_empty() {
                return Some(entity_id.clone());
            }
        }
        None
    }

    fn resolve_definite_noun(&self, noun: &str, context: &DialogueContext) -> Option<String> {
        for entity_id in context.recent_entities.iter().rev() {
            if entity_id.to_lowercase().contains(noun) {
                return Some(entity_id.clone());
            }
        }
        None
    }
}

#[derive(Debug, Clone, Copy)]
struct TokenSpan<'a> {
    text: &'a str,
    start: usize,
    end: usize,
}

fn tokenize_with_spans(text: &str) -> Vec<TokenSpan<'_>> {
    let mut tokens = Vec::new();
    let mut current_start: Option<usize> = None;

    for (idx, ch) in text.char_indices() {
        if ch.is_whitespace() {
            if let Some(start) = current_start.take() {
                tokens.push(TokenSpan {
                    text: &text[start..idx],
                    start,
                    end: idx,
                });
            }
        } else if current_start.is_none() {
            current_start = Some(idx);
        }
    }

    if let Some(start) = current_start {
        tokens.push(TokenSpan {
            text: &text[start..text.len()],
            start,
            end: text.len(),
        });
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_reports_byte_spans_for_pronouns_and_definite_nouns() {
        let resolver = CoreferenceResolver::new();
        let context = DialogueContext {
            recent_entities: vec!["file:report.txt".into(), "person:alice".into()],
            ..Default::default()
        };
        let text = "Alice opened the report, then she shared it.";

        let refs = resolver.resolve(text, &context);

        let definite = refs
            .iter()
            .find(|r| r.reference_type == ReferenceType::DefiniteNoun)
            .expect("expected definite noun reference");
        assert_eq!(&text[definite.start..definite.end], "the report,");

        let pronoun = refs
            .iter()
            .find(|r| r.surface_form == "she")
            .expect("expected pronoun reference");
        assert_eq!(&text[pronoun.start..pronoun.end], "she");
    }
}

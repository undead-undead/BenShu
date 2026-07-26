#[derive(Debug, Clone, Default)]
pub(crate) struct CharacterNameDecision {
    pub(crate) accepted: bool,
}

pub(crate) fn audit_character_name_candidate(name: &str, _language: &str) -> CharacterNameDecision {
    let name = name.trim();
    let accepted = if name.is_empty()
        || character_name_is_placeholder(name)
        || character_name_has_mechanical_repetition(name)
        || name.chars().any(char::is_control)
    {
        false
    } else if name.chars().any(is_cjk_unified) {
        let count = name.chars().count();
        (2..=12).contains(&count)
            && name
                .chars()
                .all(|ch| is_cjk_unified(ch) || matches!(ch, '·' | '•'))
    } else {
        latin_character_name_is_usable(name)
    };

    CharacterNameDecision { accepted }
}

pub(crate) fn allocate_character_name(
    project_key: &str,
    request_id: &str,
    role: &str,
    language: &str,
    used_names: &BTreeSet<String>,
) -> Option<String> {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(format!("{project_key}\0{request_id}\0{role}").as_bytes());
    let seed = u64::from_be_bytes(
        digest[..8]
            .try_into()
            .expect("sha256 prefix is eight bytes"),
    ) as usize;
    let candidates = if language_prefers_cjk_names(language) {
        cjk_name_candidates(seed)
    } else {
        latin_name_candidates(seed)
    };
    candidates.into_iter().find(|candidate| {
        character_name_is_distinct_from_used(candidate, used_names)
            && audit_character_name_candidate(candidate, language).accepted
    })
}

fn character_name_is_distinct_from_used(candidate: &str, used_names: &BTreeSet<String>) -> bool {
    if used_names.contains(candidate) {
        return false;
    }
    let candidate_chars = candidate.chars().collect::<Vec<_>>();
    used_names.iter().all(|used| {
        let used_chars = used.chars().collect::<Vec<_>>();
        if candidate_chars.iter().all(|ch| is_cjk_unified(*ch))
            && used_chars.iter().all(|ch| is_cjk_unified(*ch))
        {
            let candidate_given = candidate_chars.get(1..).unwrap_or_default();
            let used_given = used_chars.get(1..).unwrap_or_default();
            candidate_given != used_given
                && (candidate_chars.first() != used_chars.first()
                    || candidate_chars.len() != used_chars.len())
                && candidate_chars.last() != used_chars.last()
        } else {
            candidate.to_lowercase() != used.to_lowercase()
        }
    })
}

fn language_prefers_cjk_names(language: &str) -> bool {
    let lowered = language.to_ascii_lowercase();
    lowered.contains("zh") || lowered.contains("chinese") || language.contains('中')
}

fn cjk_name_candidates(seed: usize) -> Vec<String> {
    const SURNAMES: &[char] = &[
        '钟', '许', '阮', '季', '陶', '裴', '梁', '闻', '姜', '唐', '秦', '岑', '祝', '南', '韩',
        '程', '顾', '商', '谢', '宋', '叶', '沈', '陆', '温',
    ];
    const FIRST: &[char] = &[
        '照', '砚', '望', '予', '栖', '泊', '知', '启', '听', '怀', '昭', '景', '清', '星', '承',
        '谨', '屿', '云', '维', '晏',
    ];
    const SECOND: &[char] = &[
        '宁', '安', '序', '舟', '禾', '澜', '声', '白', '遥', '衡', '川', '棠', '原', '真', '岚',
        '野', '弦', '桥', '朔', '言',
    ];
    let candidate_count = SURNAMES.len() * FIRST.len() * SECOND.len();
    (0..candidate_count)
        .map(|offset| {
            // 479 is coprime to 24 * 20 * 20, so a full pass visits every
            // combination while retaining a stable project-specific order.
            let index = seed.wrapping_add(offset * 479) % candidate_count;
            format!(
                "{}{}{}",
                SURNAMES[index % SURNAMES.len()],
                FIRST[(index / SURNAMES.len()) % FIRST.len()],
                SECOND[(index / (SURNAMES.len() * FIRST.len()) + offset) % SECOND.len()]
            )
        })
        .collect()
}

fn latin_name_candidates(seed: usize) -> Vec<String> {
    const FIRST: &[&str] = &[
        "Mara", "Iris", "Nora", "Elian", "Soren", "Talia", "Milo", "Vera", "Jonas", "Celia",
        "Ronan", "Lena", "Alden", "Mira", "Dorian", "Nadia",
    ];
    const LAST: &[&str] = &[
        "Vale", "Rowan", "Maren", "Hale", "Arden", "Voss", "Sayer", "Keene", "Orin", "Dane",
        "Morrow", "Reeve", "Quill", "Sloan", "Wren", "Blake",
    ];
    let candidate_count = FIRST.len() * LAST.len();
    (0..candidate_count)
        .map(|offset| {
            let index = seed.wrapping_add(offset * 17) % candidate_count;
            format!(
                "{} {}",
                FIRST[index % FIRST.len()],
                LAST[(index / FIRST.len() + offset) % LAST.len()]
            )
        })
        .collect()
}

fn character_name_has_mechanical_repetition(name: &str) -> bool {
    let chars = name.trim().chars().collect::<Vec<_>>();
    chars.len() >= 3
        && chars.iter().all(|ch| is_cjk_unified(*ch))
        && (chars.windows(2).any(|pair| pair[0] == pair[1])
            || (chars.len() == 3 && chars.first() == chars.last()))
}

fn latin_character_name_is_usable(name: &str) -> bool {
    let count = name.chars().count();
    if !(2..=80).contains(&count) || !name.chars().any(|ch| ch.is_alphabetic()) {
        return false;
    }
    name.chars()
        .all(|ch| ch.is_alphabetic() || matches!(ch, ' ' | '-' | '\'' | '’' | '.'))
}

fn character_name_is_placeholder(name: &str) -> bool {
    let lowered = name.to_ascii_lowercase();
    matches!(
        name,
        "主角"
            | "主人公"
            | "男主"
            | "女主"
            | "反派"
            | "对手"
            | "配角"
            | "同伴"
            | "导师"
            | "敌人"
            | "坏人"
            | "人物"
            | "角色"
            | "少年"
            | "少女"
            | "青年"
            | "男子"
            | "女子"
            | "男人"
            | "女人"
            | "自己"
            | "自身"
            | "自我"
    ) || matches!(
        lowered.as_str(),
        "protagonist"
            | "main character"
            | "hero"
            | "heroine"
            | "antagonist"
            | "opponent"
            | "supporting character"
            | "character"
            | "mentor"
    )
}

fn is_cjk_unified(ch: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&ch)
        || ('\u{3400}'..='\u{4dbf}').contains(&ch)
        || ('\u{20000}'..='\u{2a6df}').contains(&ch)
        || ('\u{2a700}'..='\u{2b73f}').contains(&ch)
        || ('\u{2b740}'..='\u{2b81f}').contains(&ch)
        || ('\u{2b820}'..='\u{2ceaf}').contains(&ch)
        || ('\u{f900}'..='\u{faff}').contains(&ch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_contract_names_without_project_specific_blacklists() {
        assert!(audit_character_name_candidate("林默", "zh-CN").accepted);
        assert!(audit_character_name_candidate("钟望宁", "zh-CN").accepted);
        assert!(audit_character_name_candidate("Mara Vale", "en").accepted);
        assert!(audit_character_name_candidate("Mara Vale", "zh-CN").accepted);
    }

    #[test]
    fn rejects_placeholders_and_malformed_surfaces() {
        assert!(!audit_character_name_candidate("主角", "zh-CN").accepted);
        assert!(!audit_character_name_candidate("钟钟宁", "zh-CN").accepted);
        assert!(!audit_character_name_candidate("白澈白", "zh-CN").accepted);
        assert!(!audit_character_name_candidate("Mara/Schema", "en").accepted);
    }

    #[test]
    fn allocates_stable_non_colliding_names() {
        let used = BTreeSet::new();
        let first =
            allocate_character_name("project", "mentor", "导师", "zh-CN", &used).expect("name");
        let repeat =
            allocate_character_name("project", "mentor", "导师", "zh-CN", &used).expect("name");
        assert_eq!(first, repeat);

        let mut used = used;
        used.insert(first.clone());
        let replacement = allocate_character_name("project", "mentor", "导师", "zh-CN", &used)
            .expect("replacement");
        assert_ne!(first, replacement);
    }

    #[test]
    fn allocated_cjk_names_avoid_accidental_shared_surnames() {
        let mut used = BTreeSet::new();
        let first = allocate_character_name("project", "primary", "主角", "zh-CN", &used)
            .expect("first name");
        used.insert(first.clone());
        let second = allocate_character_name("project", "opponent", "对手", "zh-CN", &used)
            .expect("second name");

        assert_ne!(first.chars().next(), second.chars().next());
    }

    #[test]
    fn generated_cjk_names_avoid_visually_confusing_authority_names() {
        let used = BTreeSet::from(["秦承弦".to_string(), "林深".to_string()]);

        assert!(!character_name_is_distinct_from_used("秦知弦", &used));
        assert!(!character_name_is_distinct_from_used("梁承弦", &used));
        assert!(!character_name_is_distinct_from_used("梁知弦", &used));
        assert!(character_name_is_distinct_from_used("林望安", &used));
        assert!(character_name_is_distinct_from_used("梁知白", &used));
    }
}
use std::collections::BTreeSet;

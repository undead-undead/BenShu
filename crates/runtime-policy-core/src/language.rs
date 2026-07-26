use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanguageContract {
    pub response_language: String,
    pub artifact_language: String,
    pub source: String,
}

impl LanguageContract {
    pub fn system_prompt(&self) -> String {
        if self.response_language == "same_as_user" {
            return "### LANGUAGE CONTRACT\n\
                    Respond in the same natural language as the user's latest request unless the user explicitly asks for a different language. \
                    Apply the same rule to titles, names, summaries, progress updates, and generated artifact body text. \
                    Tool names and required JSON/schema keys may remain in their required technical language, but user-facing content must not drift languages."
                .to_string();
        }

        format!(
            "### LANGUAGE CONTRACT\n\
             Detected user language: {language}.\n\
             Respond to the user in {language} unless the user explicitly asks for a different language. \
             Apply {language} to titles, names, summaries, progress updates, and generated artifact body text. \
             Tool names and required JSON/schema keys may remain in their required technical language, but user-facing content must not drift languages.",
            language = self.response_language
        )
    }

    pub fn delegate_suffix(&self) -> String {
        if self.response_language == "same_as_user" {
            return " Language contract: preserve the user's natural language for all user-facing responses, artifact titles, summaries, names, and body text unless the user explicitly requested another language; schema/tool keys may remain technical.".to_string();
        }

        format!(
            " Language contract: user-facing responses, artifact titles, summaries, names, and body text must use {}; schema/tool keys may remain technical unless the user requested another language.",
            self.response_language
        )
    }
}

pub fn resolve_language_contract(text: &str) -> LanguageContract {
    let response_language = detect_primary_language(text);
    LanguageContract {
        artifact_language: response_language.clone(),
        response_language,
        source: "latest_user_message".to_string(),
    }
}

fn detect_primary_language(text: &str) -> String {
    let mut cjk = 0usize;
    let mut hiragana_katakana = 0usize;
    let mut hangul = 0usize;
    let mut cyrillic = 0usize;
    let mut arabic = 0usize;
    let mut latin = 0usize;

    for ch in text.chars() {
        match ch {
            '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' => cjk += 1,
            '\u{3040}'..='\u{30FF}' => hiragana_katakana += 1,
            '\u{AC00}'..='\u{D7AF}' => hangul += 1,
            '\u{0400}'..='\u{04FF}' => cyrillic += 1,
            '\u{0600}'..='\u{06FF}' => arabic += 1,
            'A'..='Z' | 'a'..='z' => latin += 1,
            _ => {}
        }
    }

    if cjk >= hiragana_katakana.max(hangul).max(cyrillic).max(arabic) && cjk > 0 {
        return "zh-CN".to_string();
    }
    if hiragana_katakana > 0 {
        return "ja".to_string();
    }
    if hangul > 0 {
        return "ko".to_string();
    }
    if cyrillic > 0 {
        return "ru".to_string();
    }
    if arabic > 0 {
        return "ar".to_string();
    }
    if latin > 0 {
        return "en".to_string();
    }
    "same_as_user".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_chinese_from_mixed_tool_request() {
        let contract = resolve_language_contract("帮我写一个玄幻小说，保存成 txt");

        assert_eq!(contract.response_language, "zh-CN");
        assert!(contract.system_prompt().contains("zh-CN"));
    }

    #[test]
    fn detects_english_for_plain_ascii_request() {
        let contract = resolve_language_contract("write a science fiction story");

        assert_eq!(contract.response_language, "en");
        assert!(contract.delegate_suffix().contains("en"));
    }
}

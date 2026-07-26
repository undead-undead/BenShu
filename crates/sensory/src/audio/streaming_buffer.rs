use anyhow::Result;

/// A helper to split streaming LLM text into tts-ready sentences or chunks.
pub struct StreamingTextBuffer {
    buffer: String,
    word_count: usize,
    first_chunk_sent: bool,
    threshold_words: usize,
}

impl StreamingTextBuffer {
    pub fn new(threshold_words: usize) -> Self {
        Self {
            buffer: String::new(),
            word_count: 0,
            first_chunk_sent: false,
            threshold_words,
        }
    }

    /// Default: 3 words for the first chunk, then wait for punctuation.
    pub fn voice_default() -> Self {
        Self::new(3)
    }

    /// Add a chunk of text and return a list of ready sentences/chunks.
    pub fn push(&mut self, text: &str) -> Vec<String> {
        self.buffer.push_str(text);

        // Count words in current buffer (naive)
        let words: Vec<&str> = self.buffer.split_whitespace().collect();
        self.word_count = words.len();

        let mut ready = Vec::new();

        // Rule 1: Fallback for the very first chunk (e.g. 3 words)
        if !self.first_chunk_sent && self.word_count >= self.threshold_words {
            // Find a good place to cut (e.g. after a word)
            // But we don't want to cut a word in half.
            // If the chunk ended with a space or punctuation, it's safe.
            if text.ends_with(' ') || text.contains(|c: char| c.is_ascii_punctuation()) {
                ready.push(self.buffer.clone());
                self.buffer.clear();
                self.word_count = 0;
                self.first_chunk_sent = true;
            }
        }

        // Rule 2: Wait for sentence boundaries (., !, ?, \n)
        let mut last_idx = 0;
        for (i, c) in self.buffer.char_indices() {
            if c == '.' || c == '!' || c == '?' || c == '\n' {
                let sentence = &self.buffer[last_idx..=i];
                if self.word_count_of(sentence) >= 1 {
                    ready.push(sentence.trim().to_string());
                    last_idx = i + 1;
                }
            }
        }

        if last_idx > 0 {
            self.buffer = self.buffer[last_idx..].to_string();
            self.word_count = self.word_count_of(&self.buffer);
            self.first_chunk_sent = true;
        }

        ready
    }

    fn word_count_of(&self, s: &str) -> usize {
        s.split_whitespace().count()
    }

    /// Get remaining text (useful at the end of stream)
    pub fn finish(&mut self) -> Option<String> {
        if !self.buffer.trim().is_empty() {
            let res = self.buffer.clone();
            self.buffer.clear();
            Some(res)
        } else {
            None
        }
    }
}

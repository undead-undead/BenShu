//! Intent Analysis for Recursive Retrieval
//!
//! Analyzes user queries to determine the best retrieval strategy and target paths.

use crate::error::Result;
use serde::{Deserialize, Serialize};

/// Type of context to search
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextType {
    /// Abstract (L0) - High level overview
    Abstract,
    /// Overview (L1) - Detailed summary
    Overview,
    /// Full Content (L2) - Actual file content
    Full,
}

/// A structured query derived from intent analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypedQuery {
    pub query: String,
    pub context_type: ContextType,
    pub priority: u8,
}

/// The plan execution strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryPlan {
    pub original_query: String,
    pub steps: Vec<TypedQuery>,
    pub target_paths: Vec<String>,
}

use once_cell::sync::Lazy;
use regex::Regex;
use std::sync::Arc;

static PATH_REGEX: Lazy<Regex> = Lazy::new(|| {
    // Requires either start with / or ./ or contain a / or . (file extension)
    Regex::new(r#"(?i)(?:in|path:)\s*((?:\./|/)[a-zA-Z0-9._\-/]+|[a-zA-Z0-9._\-/]+\.[a-z]{1,5})"#)
        .unwrap()
});

static DEEP_ANALYSIS_KEYWORDS: &[&str] = &[
    "分析",
    "原理",
    "逻辑",
    "源码",
    "全量",
    "详细",
    "实现",
    "analyze",
    "how does",
    "implementation",
    "source code",
    "logic",
    "details",
    "full",
    "internal",
    "deep",
];

static OVERVIEW_KEYWORDS: &[&str] = &[
    "概要",
    "总结",
    "是什么",
    "介绍",
    "大纲",
    "summary",
    "overview",
    "what is",
    "intro",
    "outline",
];

pub struct IntentAnalyzer {
    path_regex: Arc<Regex>,
}

impl IntentAnalyzer {
    pub fn new() -> Self {
        Self {
            path_regex: Arc::new(PATH_REGEX.clone()),
        }
    }

    /// Analyze a user query and generate a retrieval plan
    pub async fn analyze(&self, query: &str) -> Result<QueryPlan> {
        let mut steps = Vec::new();
        let query_lower = query.to_lowercase();

        // 1. Extract Target Paths
        let mut target_paths = Vec::new();
        for cap in self.path_regex.captures_iter(query) {
            if let Some(path) = cap.get(1) {
                target_paths.push(path.as_str().to_string());
            }
        }

        // 2. Clean query
        let cleaned_query = self.path_regex.replace_all(query, "").trim().to_string();

        // Guard: If query only contains path, use the path as query or fallback
        let query_for_search = if cleaned_query.is_empty() {
            if !target_paths.is_empty() {
                target_paths[0].clone()
            } else {
                query.to_string()
            }
        } else {
            cleaned_query
        };

        // 3. Determine Search Depth & Steps

        // L0: Abstract (Broadest)
        steps.push(TypedQuery {
            query: query_for_search.clone(),
            context_type: ContextType::Abstract,
            priority: 10,
        });

        // L1: Overview
        let keyword_overview = OVERVIEW_KEYWORDS.iter().any(|k| query_lower.contains(k));
        let needs_overview = keyword_overview || query_for_search.len() > 15;

        if needs_overview {
            steps.push(TypedQuery {
                query: query_for_search.clone(),
                context_type: ContextType::Overview,
                // Keyword match gets higher priority than mere length
                priority: if keyword_overview {
                    10
                } else if query_for_search.len() > 30 {
                    9
                } else {
                    7
                },
            });
        }

        // L2: Full Content
        let keyword_deep = DEEP_ANALYSIS_KEYWORDS
            .iter()
            .any(|k| query_lower.contains(k));
        let needs_full = keyword_deep
            || query_lower.contains("::")
            || (query_lower.contains(".") && query_for_search.len() < 50);

        if needs_full {
            steps.push(TypedQuery {
                query: query_for_search.clone(),
                context_type: ContextType::Full,
                priority: if keyword_deep { 8 } else { 5 },
            });
        }

        Ok(QueryPlan {
            original_query: query.to_string(),
            steps,
            target_paths,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_intent_path_extraction() {
        let analyzer = IntentAnalyzer::new();
        let plan = analyzer
            .analyze("Search implementation in /src/fts.rs")
            .await
            .unwrap();
        assert_eq!(plan.target_paths, vec!["/src/fts.rs"]);
        assert!(plan
            .steps
            .iter()
            .any(|s| s.context_type == ContextType::Full));
    }

    #[tokio::test]
    async fn test_intent_deep_analysis() {
        let analyzer = IntentAnalyzer::new();
        let plan = analyzer.analyze("分析一下 indexer 逻辑").await.unwrap();
        assert!(plan
            .steps
            .iter()
            .any(|s| s.context_type == ContextType::Full));
        assert!(plan.steps.iter().any(|s| s.priority >= 8));
    }

    #[tokio::test]
    async fn test_intent_empty_clean_query() {
        let analyzer = IntentAnalyzer::new();
        let plan = analyzer.analyze("path: /test/file.txt").await.unwrap();
        assert_eq!(plan.target_paths, vec!["/test/file.txt"]);
        assert!(!plan.steps[0].query.is_empty());
        assert_eq!(plan.steps[0].query, "/test/file.txt");
    }

    #[tokio::test]
    async fn test_intent_overview() {
        let analyzer = IntentAnalyzer::new();
        let plan = analyzer.analyze("总结一下项目大纲").await.unwrap();
        let overview_step = plan
            .steps
            .iter()
            .find(|s| s.context_type == ContextType::Overview)
            .unwrap();
        assert_eq!(overview_step.priority, 10);
    }
}

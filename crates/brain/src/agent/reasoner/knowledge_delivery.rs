use regex::Regex;

fn durable_storage_targets() -> &'static [&'static str] {
    &[
        "知识库",
        "资料库",
        "数据库",
        "文档库",
        "素材库",
        "语料库",
        "档案库",
        "检索库",
        "向量库",
        "入库",
        "knowledge base",
        "knowledge-base",
        "database",
        "document store",
        "document-store",
        "document repository",
        "retrieval storage",
        "retrieval store",
        "rag store",
        "vector store",
        "corpus",
        "archive",
    ]
}

fn durable_storage_actions() -> &'static [&'static str] {
    &[
        "保存", "存进", "存入", "存到", "写入", "导入", "加入", "放进", "放到", "收进", "收入",
        "入", "收到", "入库", "save", "store", "write", "import", "ingest", "persist", "add to",
        "put into",
    ]
}

fn durable_storage_negative_markers() -> &'static [&'static str] {
    &[
        "不要",
        "别",
        "不必",
        "无需",
        "不要把",
        "不要将",
        "do not",
        "don't",
        "dont",
        "without",
        "no ",
    ]
}

fn has_directed_durable_storage_request(lowered: &str) -> bool {
    let action_only_requests = ["入库"];
    if action_only_requests.iter().any(|action| {
        lowered.match_indices(action).any(|(action_start, _)| {
            !lowered[..action_start].ends_with('刚')
                && ["把", "将", "请", "帮我", "需要", "并", "然后", "再"]
                    .iter()
                    .any(|lead| {
                        lowered[..action_start]
                            .rfind(lead)
                            .is_some_and(|lead_start| action_start - lead_start <= 120)
                    })
        })
    }) {
        return true;
    }

    durable_storage_actions().iter().any(|action| {
        lowered
            .match_indices(action)
            .any(|(action_start, action_text)| {
                let action_end = action_start + action_text.len();
                durable_storage_targets().iter().any(|target| {
                    lowered.match_indices(target).any(|(target_start, _)| {
                        target_start >= action_end && target_start - action_end <= 120
                    })
                })
            })
    })
}

fn has_negated_directed_durable_storage_request(lowered: &str) -> bool {
    durable_storage_negative_markers().iter().any(|negative| {
        lowered
            .match_indices(negative)
            .any(|(negative_start, negative_text)| {
                let negative_end = negative_start + negative_text.len();

                ["入库"].iter().any(|action| {
                    lowered.match_indices(action).any(|(action_start, _)| {
                        action_start >= negative_end && action_start - negative_start <= 120
                    })
                }) || durable_storage_actions().iter().any(|action| {
                    lowered
                        .match_indices(action)
                        .any(|(action_start, action_text)| {
                            let action_end = action_start + action_text.len();
                            action_start >= negative_end
                                && action_start - negative_start <= 120
                                && durable_storage_targets().iter().any(|target| {
                                    lowered.match_indices(target).any(|(target_start, _)| {
                                        target_start >= action_end
                                            && target_start - action_end <= 120
                                    })
                                })
                        })
                })
            })
    })
}

pub(super) fn query_denies_knowledge_persistence(lowered: &str) -> bool {
    let explicit_denials = [
        "不要保存到知识库",
        "不要保存进知识库",
        "不要存入知识库",
        "不要写入知识库",
        "不要导入知识库",
        "不要加入知识库",
        "不要入库",
        "别保存到知识库",
        "别保存进知识库",
        "别存入知识库",
        "别写入知识库",
        "别导入知识库",
        "别加入知识库",
        "别入库",
        "不保存到知识库",
        "不保存进知识库",
        "不存入知识库",
        "不写入知识库",
        "不导入知识库",
        "不加入知识库",
        "不入库",
        "do not save to the knowledge base",
        "don't save to the knowledge base",
        "dont save to the knowledge base",
        "do not store this in the knowledge base",
        "don't store this in the knowledge base",
        "dont store this in the knowledge base",
        "do not write this into the knowledge base",
        "don't write this into the knowledge base",
        "dont write this into the knowledge base",
        "do not import this into the knowledge base",
        "don't import this into the knowledge base",
        "dont import this into the knowledge base",
        "without saving to the knowledge base",
        "without writing to the knowledge base",
        "no knowledge base write",
    ];

    explicit_denials
        .iter()
        .any(|marker| lowered.contains(marker))
        || has_negated_directed_durable_storage_request(lowered)
}

pub(super) fn query_requests_knowledge_persistence(query: &str) -> bool {
    let lowered = query.to_lowercase();
    if query_denies_knowledge_persistence(&lowered) {
        return false;
    }

    has_directed_durable_storage_request(&lowered)
}

pub(super) fn query_requests_post_import_delivery(query: &str) -> bool {
    let lowered = query.to_lowercase();
    query_requests_knowledge_persistence(query)
        && (lowered.contains("predict")
            || lowered.contains("prediction")
            || lowered.contains("forecast")
            || lowered.contains("summarize")
            || lowered.contains("summary")
            || lowered.contains("analyze")
            || lowered.contains("analysis")
            || lowered.contains("report")
            || query.contains("预测")
            || query.contains("总结")
            || query.contains("汇总")
            || query.contains("分析")
            || query.contains("报告")
            || query.contains("最后")
            || query.contains("基于")
            || query.contains("根据")
            || query_requests_creative_synthesis(query))
}

pub(super) fn query_requests_prediction(query: &str) -> bool {
    let lowered = query.to_lowercase();
    lowered.contains("predict")
        || lowered.contains("prediction")
        || lowered.contains("forecast")
        || query.contains("预测")
}

pub(super) fn query_requests_creative_synthesis(query: &str) -> bool {
    let lowered = query.to_lowercase();
    lowered.contains("creative")
        || lowered.contains("original")
        || lowered.contains("write a")
        || lowered.contains("draft a")
        || lowered.contains("novel")
        || lowered.contains("story")
        || query.contains("原创")
        || query.contains("创作")
        || query.contains("创造")
        || query.contains("写一部")
        || query.contains("写一个")
        || query.contains("写一篇")
        || query.contains("撰写")
        || (query.contains("写") && (query.contains("小说") || query.contains("故事")))
        || query.contains("小说名字")
        || query.contains("开篇")
}

pub(super) fn query_requests_file_artifact(query: &str) -> bool {
    let lowered = query.to_lowercase();
    lowered.contains("save as")
        || lowered.contains("write to file")
        || lowered.contains("txt")
        || lowered.contains(".txt")
        || lowered.contains("pdf")
        || lowered.contains(".pdf")
        || lowered.contains("markdown")
        || lowered.contains(".md")
        || query.contains("保存成")
        || query.contains("保存为")
        || query.contains("做成pdf")
        || query.contains("生成pdf")
        || query.contains("输出pdf")
        || query.contains("写成文件")
        || query.contains("写入文件")
        || query.contains("txt文档")
        || query.contains("文本文件")
        || query_requests_large_generated_text_artifact(query)
}

fn query_requests_large_generated_text_artifact(query: &str) -> bool {
    if !query_requests_creative_synthesis(query) {
        return false;
    }

    let lowered = query.to_lowercase();
    if lowered.contains("longform artifact")
        || lowered.contains("large document")
        || lowered.contains("book-length")
        || lowered.contains("novel-length")
        || query.contains("长篇")
        || query.contains("超长")
        || query.contains("百万字")
        || query.contains("万字")
    {
        return true;
    }

    Regex::new(r"(?i)(\d{5,})\s*(?:words?|chars?|characters?)")
        .expect("valid large text artifact regex")
        .captures(query)
        .and_then(|caps| caps.get(1))
        .and_then(|value| value.as_str().parse::<usize>().ok())
        .is_some_and(|count| count >= 20_000)
        || Regex::new(r"(\d{1,4})\s*(?:万)\s*(?:字|字符)")
            .expect("valid chinese large text artifact regex")
            .captures(query)
            .and_then(|caps| caps.get(1))
            .and_then(|value| value.as_str().parse::<usize>().ok())
            .is_some_and(|wan| wan >= 2)
}

pub(super) fn numeric_record_rows_from_text(text: &str) -> Vec<(String, String, Vec<u8>)> {
    let row_re =
        Regex::new(r"(?m)(20\d{2}-\d{2}-\d{2})\s+(\d{5,})\s+((?:\d{1,2}\s+){5,}\d{1,2})(?:\s|$)")
            .expect("numeric record row regex is valid");

    row_re
        .captures_iter(text)
        .filter_map(|caps| {
            let date = caps.get(1)?.as_str().to_string();
            let issue = caps.get(2)?.as_str().to_string();
            let numbers = caps
                .get(3)?
                .as_str()
                .split_whitespace()
                .filter_map(|part| part.parse::<u8>().ok())
                .collect::<Vec<_>>();
            (numbers.len() >= 6).then_some((date, issue, numbers))
        })
        .collect()
}

pub(super) fn ranked_metadata_items_from_result(result: &str) -> Vec<(String, String, String)> {
    result
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let without_bullet = trimmed.strip_prefix("- ")?;
            let (_, after_rank) = without_bullet.split_once(". ")?;
            let (title, rest) = after_rank.split_once(" | public metadata: ")?;
            let (metadata, source) = rest
                .split_once(" | source: ")
                .map(|(metadata, source)| (metadata, source))
                .unwrap_or((rest, ""));
            let title = title.trim();
            (!title.is_empty()).then_some((
                title.to_string(),
                metadata.trim().to_string(),
                source.trim().to_string(),
            ))
        })
        .take(10)
        .collect()
}

pub(super) fn summarize_lookup_delivery(
    query: &str,
    content: &str,
    prefers_chinese: bool,
    first_retrieval_snippet: Option<String>,
    compact_result: String,
) -> String {
    let contact_records = extract_requested_contact_records(query, content);
    if !contact_records.is_empty() {
        return if prefers_chinese {
            format!("根据知识库查询结果：\n{}", contact_records.join("\n"))
        } else {
            format!(
                "According to the knowledge-base results:\n{}",
                contact_records.join("\n")
            )
        };
    }

    let contact_phones = extract_requested_contact_phones(query, content);
    if !contact_phones.is_empty() {
        let mut lines = Vec::new();
        for (name, phone) in contact_phones {
            lines.push(format!("- {name}：{phone}"));
        }
        return if prefers_chinese {
            format!("根据知识库查询结果：\n{}", lines.join("\n"))
        } else {
            format!(
                "According to the knowledge-base results:\n{}",
                lines.join("\n")
            )
        };
    }

    if let Some(snippet) = first_retrieval_snippet {
        return if prefers_chinese {
            format!("我在知识库里找到的最相关内容是：{}", snippet)
        } else {
            format!("The most relevant knowledge-base result says: {}", snippet)
        };
    }

    let lowered_compact = compact_result.to_lowercase();
    if lowered_compact.contains("no results found")
        || lowered_compact.contains("no matching knowledge documents found")
        || lowered_compact.contains("no relevant information found")
    {
        return if prefers_chinese {
            "我没有在知识库里找到相关内容。".to_string()
        } else {
            "I did not find relevant content in the knowledge base.".to_string()
        };
    }
    if prefers_chinese {
        format!("我在知识库里找到了相关内容：{}", compact_result)
    } else {
        format!(
            "I found relevant knowledge-base content: {}",
            compact_result
        )
    }
}

fn clean_short_field_value(value: &str) -> String {
    value
        .trim()
        .trim_matches(|ch: char| {
            matches!(
                ch,
                '"' | '\'' | ',' | '，' | '。' | ':' | '：' | '-' | ' ' | '\t'
            )
        })
        .split(|ch: char| ch == ',' || ch == '，' || ch == '\n')
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches('"')
        .to_string()
}

fn extract_phone_like_value(value: &str) -> Option<String> {
    let mut current = String::new();
    let mut best = String::new();

    for ch in value.chars() {
        if ch.is_ascii_digit() || matches!(ch, '-' | '+' | '(' | ')' | ' ') {
            current.push(ch);
        } else {
            if current.chars().filter(|ch| ch.is_ascii_digit()).count()
                > best.chars().filter(|ch| ch.is_ascii_digit()).count()
            {
                best = current.trim().to_string();
            }
            current.clear();
        }
    }

    if current.chars().filter(|ch| ch.is_ascii_digit()).count()
        > best.chars().filter(|ch| ch.is_ascii_digit()).count()
    {
        best = current.trim().to_string();
    }

    if best.chars().filter(|ch| ch.is_ascii_digit()).count() >= 7 {
        Some(best)
    } else {
        None
    }
}

fn extract_requested_contact_phones(query: &str, content: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut current_name: Option<String> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.contains(',') && !trimmed.contains("姓名,") {
            let cells: Vec<_> = trimmed.split(',').map(str::trim).collect();
            if cells.len() >= 3 {
                let name = cells[0].trim_matches('"').to_string();
                if query.contains(&name) {
                    if let Some(phone) =
                        cells.iter().find_map(|cell| extract_phone_like_value(cell))
                    {
                        pairs.push((name, phone));
                    }
                }
            }
        }

        if let Some(value) =
            extract_value_after_any_marker(trimmed, &["姓名：", "姓名:", "\"name\":", "'name':"])
        {
            let name = clean_short_field_value(value);
            if !name.is_empty() {
                current_name = Some(name);
            }
        }

        let phone_source = if let Some(value) =
            extract_value_after_any_marker(trimmed, &["电话：", "电话:", "\"phone\":", "'phone':"])
        {
            value
        } else {
            trimmed
        };

        if let (Some(name), Some(phone)) = (
            current_name
                .as_ref()
                .filter(|name| query.contains(name.as_str())),
            extract_phone_like_value(phone_source),
        ) {
            pairs.push((name.clone(), phone));
            current_name = None;
        }
    }

    pairs.sort();
    pairs.dedup();
    pairs
}

fn extract_contact_name_candidates(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for marker in ["联系人", "姓名", "contact", "name"] {
        let mut rest = text;
        while let Some((_, tail)) = rest.split_once(marker) {
            let value = tail
                .trim_start_matches(|ch: char| {
                    ch.is_whitespace()
                        || matches!(ch, ':' | '：' | '=' | '\'' | '"' | '`' | '是' | '为')
                })
                .chars()
                .take_while(|ch| {
                    !ch.is_whitespace()
                        && !matches!(
                            ch,
                            '\'' | '"'
                                | '`'
                                | ','
                                | '，'
                                | '。'
                                | '.'
                                | ';'
                                | '；'
                                | '的'
                                | '和'
                                | '、'
                        )
                })
                .collect::<String>();
            let value = value.trim().to_string();
            if !value.is_empty() && value.chars().count() <= 24 {
                names.push(value);
            }
            rest = tail;
        }
    }
    names.sort();
    names.dedup();
    names
}

fn extract_city_like_value(text: &str) -> Option<String> {
    for marker in ["城市：", "城市:", "城市", "\"city\":", "'city':", "city:"] {
        if let Some((_, value)) = text.split_once(marker) {
            let city = value
                .trim_start_matches(|ch: char| {
                    ch.is_whitespace()
                        || matches!(ch, ':' | '：' | '=' | '\'' | '"' | '`' | '是' | '为')
                })
                .chars()
                .take_while(|ch| {
                    !matches!(
                        ch,
                        '\'' | '"' | '`' | ',' | '，' | '。' | '.' | ';' | '；' | '\n' | '\r'
                    )
                })
                .collect::<String>()
                .trim()
                .to_string();
            if !city.is_empty() && city.chars().count() <= 32 {
                return Some(city);
            }
        }
    }
    None
}

fn extract_requested_contact_records(query: &str, content: &str) -> Vec<String> {
    let mut names = extract_contact_name_candidates(query);
    names.extend(extract_contact_name_candidates(content));
    names.sort();
    names.dedup();

    let mut records = Vec::new();
    for name in names {
        if !query.contains(&name) || !content.contains(&name) {
            continue;
        }
        let mut phone = None;
        let mut city = None;
        for line in content.lines() {
            let line = line.trim();
            if !(line.contains(&name) || line.contains("电话") || line.contains("phone")) {
                continue;
            }
            if phone.is_none() {
                phone = extract_phone_like_value(line);
            }
            if city.is_none() {
                city = extract_city_like_value(line);
            }
        }
        let mut fields = Vec::new();
        if let Some(phone) = phone {
            fields.push(format!("电话 {phone}"));
        }
        if let Some(city) = city {
            fields.push(format!("城市 {city}"));
        }
        if !fields.is_empty() {
            records.push(format!("- {name}：{}", fields.join("，")));
        }
    }
    records.sort();
    records.dedup();
    records
}

fn extract_value_after_any_marker<'a>(text: &'a str, markers: &[&str]) -> Option<&'a str> {
    markers
        .iter()
        .find_map(|marker| text.split_once(marker).map(|(_, value)| value.trim()))
}

use benshu_brain::agent::message::ContentPart;
use benshu_builtin_tools::tool::document_understand::normalize_media_attachments;
use benshu_builtin_tools::tool::document_understand::DocumentUnderstandTool;
use benshu_infra::bus::MediaAttachment;
use std::sync::Arc;

pub async fn inbound_media_to_parts(
    document_router: Arc<DocumentUnderstandTool>,
    media: Option<Vec<MediaAttachment>>,
) -> Vec<ContentPart> {
    normalize_media_attachments(document_router, media).await
}

pub fn append_media_context_parts(parts: &mut Vec<ContentPart>, media_parts: Vec<ContentPart>) {
    let mut merged_text = String::new();
    let mut parsed_attachment_parts = Vec::new();
    let mut non_text_parts = Vec::new();

    for part in media_parts {
        match part {
            ContentPart::Text { text } => {
                let trimmed = text.trim();
                if trimmed.starts_with("[Parsed ") {
                    parsed_attachment_parts.push(ContentPart::Text { text });
                } else if !trimmed.is_empty() {
                    if !merged_text.is_empty() {
                        merged_text.push('\n');
                    }
                    merged_text.push_str(trimmed);
                }
            }
            other => non_text_parts.push(other),
        }
    }

    if !merged_text.is_empty() || !parsed_attachment_parts.is_empty() {
        if let Some(ContentPart::Text { text }) = parts.first_mut() {
            if !text.trim().is_empty() {
                text.push_str("\n\n");
            }
            text.push_str("以下是本轮聊天附件的临时解析内容，请优先基于这些内容回答，不要把它当作需要重新读取的文件路径：");
            if !merged_text.is_empty() {
                text.push('\n');
                text.push_str(&merged_text);
            }
        } else {
            parts.insert(
                0,
                ContentPart::Text {
                    text: if merged_text.is_empty() {
                        "以下是本轮聊天附件的临时解析内容，请优先基于这些内容回答，不要把它当作需要重新读取的文件路径："
                            .to_string()
                    } else {
                        format!(
                            "以下是本轮聊天附件的临时解析内容，请优先基于这些内容回答，不要把它当作需要重新读取的文件路径：\n{}",
                            merged_text
                        )
                    },
                },
            );
        }
    }

    parts.extend(parsed_attachment_parts);
    parts.extend(non_text_parts);
}

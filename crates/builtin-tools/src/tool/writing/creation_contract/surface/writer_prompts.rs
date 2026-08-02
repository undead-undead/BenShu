use super::*;

pub(crate) fn creation_planning_language_boundary(language: &str) -> &'static str {
    if language.to_ascii_lowercase().starts_with("zh") || language.contains("中文") {
        "全部面向用户的合同内容必须使用中文；书名、人物名、术语名只给中文原创名，不附英文译名或拼音；不要混入韩文/日文/英文括注、LaTeX/箭头符号残片、工具字段或 JSON 片段。"
    } else {
        "Use the requested artifact language consistently; do not add bilingual translations, tool fields, JSON fragments, or unrelated foreign-script residue unless the user explicitly asks for them."
    }
}

pub fn final_prompt_from_approved_creation_draft(
    draft: &SessionCreationDraftState,
    approved: &Value,
    approval_message: &str,
) -> String {
    let project_path = project_path_from_approved_creation_draft(approved).unwrap_or_default();
    let approval_scope = creation_draft_turn_scope(approval_message, &draft.artifact_kind);
    let turn_scope = if approval_scope == CreationDraftTurnScope::Configured {
        persisted_creation_execution_scope(&draft.planning_notes)
            .unwrap_or(CreationDraftTurnScope::Configured)
    } else {
        approval_scope
    };
    let execution_scope_note =
        creation_execution_scope_note_for_scope(turn_scope).unwrap_or_default();
    let is_followup_continuation = creation_draft_message_requests_continuation_generation(
        approval_message,
        &approval_message.to_ascii_lowercase(),
    );
    if draft.artifact_kind == "fiction" {
        let character_authority_display =
            canonical_character_authority_for_prompt(draft, approved, &project_path);
        let outline_payload = if !project_path.trim().is_empty() {
            "以 project_path 内 project.json 为持久化权威；contract.md、story_bible.json 和 story_bible.md 是可再生视图。章节合同与已批准章节摘要提供执行证据；书名、角色、终局、分卷、章节目标与连续性不得从聊天草案占位文本重新推断。".to_string()
        } else if is_followup_continuation {
            "以 project_path 内现有 manifest、story bible、truth、已批准章节摘要为准；不要复用历史聊天里的旧失败稿内容。".to_string()
        } else {
            creation_outline_payload(draft)
        };
        let brief_display = if is_followup_continuation {
            "以当前项目合同为准"
        } else {
            empty_display(&draft.brief, "由 writer 根据用户意图补齐")
        };
        let title_line = if draft.title.trim().is_empty() {
            "标题状态：用户未指定；writer 必须生成新的原创标题，并且不能使用占位标题。".to_string()
        } else {
            format!("标题：{}", draft.title)
        };
        let max_chapters_display = match turn_scope {
            CreationDraftTurnScope::FirstUnit => "1".to_string(),
            CreationDraftTurnScope::ExplicitUnits(value) => value.to_string(),
            CreationDraftTurnScope::AllRemaining => {
                "全部剩余章节（直到满足总目标或质量门阻塞）".to_string()
            }
            CreationDraftTurnScope::Configured => draft
                .max_chapters_per_turn
                .map(|value| value.to_string())
                .unwrap_or_else(|| "按 worker 配置".to_string()),
        };
        let turn_scope_text = match turn_scope {
            CreationDraftTurnScope::FirstUnit => {
                "本轮范围：用户本轮只要求先写第一章/下一章；不要因为总目标字数存在而连续生成全书，完成本章后返回进度，后续由用户继续。"
            }
            CreationDraftTurnScope::ExplicitUnits(value) => {
                return format!(
                    "{DIRECT_WRITER_CONTINUATION_MARKER}\n\
{execution_scope_note}\n\
用户已经在多轮对话中确认小说创作草案，请不要继续追问，直接通过 writer worker 使用 novel_studio 继续正式写作。\n\
project_path: {project_path}\n\
{title_line}\n语言：{}\n题材/方向：{}\n简述：{}\n角色权威表：{}\n总目标字数：{}\n每章目标字数档位：{}（合同只接受用户明确选择的 {} 档位）\n每轮最多章节：{}\n导出格式：{}\n\
已确认规划要点：{}\n\
用户最新要求：{}\n\
本轮范围：用户本轮明确要求生成 {value} 章；从当前项目进度连续写满 {value} 章后返回进度，不要继续越界生成，也不要因为总目标字数存在而生成全书。\n\
要求：从该项目继续执行章节写作；正文保存到 artifact/TXT，不要把长正文塞进聊天框，只返回进度、章节、字数、路径和简短摘要。",
                    empty_display(&draft.language, "跟随用户语言"),
                    empty_display(&draft.genre, "由 writer 决定"),
                    brief_display,
                    empty_display(
                        &character_authority_display,
                        "以 project_path 内已批准合同/manifest 为准"
                    ),
                    draft
                        .target_units
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "未指定".to_string()),
                    draft
                        .chapter_unit_target
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "动态决定".to_string()),
                    longform_policy::novel_chapter_unit_band_label(),
                    max_chapters_display,
                    draft.export_format,
                    empty_display(&outline_payload, "由 writer 根据已确认创作合同生成"),
                    compact_creation_text(approval_message.trim(), 500)
                );
            }
            CreationDraftTurnScope::AllRemaining => {
                "本轮范围：用户本轮要求直接生成完剩余内容；从当前项目进度继续，按已确认总目标推进到完成，仍必须受后台检查点和质量门限制；不要把长正文塞进聊天框。"
            }
            CreationDraftTurnScope::Configured => {
                "本轮范围：按用户本轮请求完成当前/下一章并返回进度；不要因为总目标字数存在而连续生成全书，后续由用户继续。"
            }
        };
        format!(
            "{DIRECT_WRITER_CONTINUATION_MARKER}\n\
{execution_scope_note}\n\
用户已经在多轮对话中确认小说创作草案，请不要继续追问，直接通过 writer worker 使用 novel_studio 继续正式写作。\n\
project_path: {project_path}\n\
{title_line}\n语言：{}\n题材/方向：{}\n简述：{}\n角色权威表：{}\n总目标字数：{}\n每章目标字数档位：{}（合同只接受用户明确选择的 {} 档位）\n每轮最多章节：{}\n导出格式：{}\n\
已确认规划要点：{}\n\
用户最新要求：{}\n\
{turn_scope_text}\n\
要求：从该项目继续执行章节写作；正文保存到 artifact/TXT，不要把长正文塞进聊天框，只返回进度、章节、字数、路径和简短摘要。",
            empty_display(&draft.language, "跟随用户语言"),
            empty_display(&draft.genre, "由 writer 决定"),
            brief_display,
            empty_display(
                &character_authority_display,
                "以 project_path 内已批准合同/manifest 为准"
            ),
            draft
                .target_units
                .map(|value| value.to_string())
                .unwrap_or_else(|| "未指定".to_string()),
            draft
                .chapter_unit_target
                .map(|value| value.to_string())
                .unwrap_or_else(|| "动态决定".to_string()),
            longform_policy::novel_chapter_unit_band_label(),
            max_chapters_display,
            draft.export_format,
            empty_display(&outline_payload, "由 writer 根据已确认创作合同生成"),
            compact_creation_text(approval_message.trim(), 500)
        )
    } else {
        format!(
            "{DIRECT_WRITER_CONTINUATION_MARKER}\n\
{execution_scope_note}\n\
用户已经在多轮对话中确认写作文档草案，请不要继续追问，直接通过 writer worker 使用 writing_studio 继续正式写作。\n\
project_path: {project_path}\n\
标题：{}\n类型：{}\n语言：{}\n主题/论点：{}\n读者：{}\n用途：{}\n目标字数：{}\n导出格式：{}\n\
本轮范围：{}\n\
要求：按该项目合同写作、审查并导出；正文保存到 artifact/TXT，不要把长正文塞进聊天框，只返回进度、字数、路径和简短摘要。",
            empty_display(&draft.title, "由 writer 生成"),
            empty_display(&draft.document_type, &draft.artifact_kind),
            empty_display(&draft.language, "跟随用户语言"),
            empty_display(&draft.thesis_or_premise, &draft.brief),
            empty_display(&draft.audience, "未指定"),
            empty_display(&draft.purpose, "未指定"),
            draft
                .target_units
                .map(|value| value.to_string())
                .unwrap_or_else(|| "未指定".to_string()),
            draft.export_format,
            if turn_scope == CreationDraftTurnScope::FirstUnit {
                "用户本轮只要求先写第一节/第一部分；不要因为总目标字数存在而连续生成完整文档。"
            } else if turn_scope == CreationDraftTurnScope::AllRemaining {
                "用户本轮要求直接完成剩余文档；从当前项目进度继续，仍必须受后台检查点和质量门限制。"
            } else {
                "按已确认的文档合同推进，但必须受后台检查点和质量门限制。"
            }
        )
    }
}

pub fn final_prompt_from_novel_content_operation(
    draft: &SessionCreationDraftState,
    approved: &Value,
    user_message: &str,
    operation: NovelContentOperation,
) -> String {
    let project_path = project_path_from_approved_creation_draft(approved).unwrap_or_default();
    let chapter_scope = requested_novel_content_chapter_scope(user_message);
    let target_chapter = referenced_artifact_segment_numbers(user_message)
        .into_iter()
        .next();
    let command = crate::tool::writing::session_route::WritingCommand {
        project_path: project_path.clone(),
        operation: Some(match operation {
            NovelContentOperation::Read => {
                crate::tool::writing::session_route::WritingOperationKind::Read
            }
            NovelContentOperation::Add => {
                crate::tool::writing::session_route::WritingOperationKind::Add
            }
            NovelContentOperation::Delete => {
                crate::tool::writing::session_route::WritingOperationKind::Delete
            }
            NovelContentOperation::Modify => {
                crate::tool::writing::session_route::WritingOperationKind::Modify
            }
        }),
        target_chapter,
        metadata_only: message_requests_metadata_only_content_operation(user_message),
        surface_cleanup: false,
        project_status: crate::tool::writing::session_route::intent_requests_project_status(
            user_message,
        ),
        user_request: compact_creation_text(user_message.trim(), 500),
    };
    let command_line = crate::tool::writing::session_route::writing_command_line(&command);
    let operation_line = match operation {
        NovelContentOperation::Read => {
            "操作类型：查询章节内容。必须先读取目标章节；只返回摘要、角色/情节要点和文件路径，不改写正文。"
        }
        NovelContentOperation::Add => {
            "操作类型：增加章节内容。必须先读取目标章节，再按用户要求把新增内容自然融入该章，保持人物、时间线和伏笔连续。"
        }
        NovelContentOperation::Delete => {
            "操作类型：删除章节内容。必须先读取目标章节，再删除用户指定的人物/段落/情节/线索，同时修补上下文衔接，不能留下断裂。"
        }
        NovelContentOperation::Modify => {
            "操作类型：修改章节内容。必须先读取目标章节，再按用户要求改写相关内容，保持作品合同、人物名、时间线和情绪线稳定。"
        }
    };
    let write_flow = match operation {
        NovelContentOperation::Read => {
            "执行流程：用当前项目读取目标章节；不要调用写入/修订动作；不要把完整正文塞进聊天框，除非用户明确要求全文。"
        }
        NovelContentOperation::Add | NovelContentOperation::Delete | NovelContentOperation::Modify => {
            "执行流程：先读取目标章节原文；基于原文生成完整修订后章节正文；写回同一章节；随后审查/验证；同步 TXT 导出；最后只返回章节号、字数、路径、修改摘要和审查状态。"
        }
    };
    format!(
        "{NOVEL_CONTENT_OPERATION_MARKER}\n\
	用户正在对已经存在的小说项目做正文级增删查改，请不要新建小说项目，也不要只修改创作合同。\n\
	{command_line}\n\
	project_path: {project_path}\n\
语言：{}\n\
题材/方向：{}\n\
当前项目合同简述：{}\n\
用户原话：{}\n\
目标章节：{}\n\
{operation_line}\n\
{write_flow}\n\
工具边界：通过 writer worker 操作当前项目的 novel_studio。查询时使用 read_chapter；增删改时必须先 read_chapter，再 revise_chapter 写回同一章，随后 audit_chapter/必要时继续修订，并确保 current.txt/章节合集.txt 指向修订后的内容。\n\
输出边界：聊天框只输出进度、章节号、字数、文件路径、简短摘要、是否通过审查；不要把长正文直接塞进聊天历史。",
        empty_display(&draft.language, "跟随用户语言"),
        empty_display(&draft.genre, "由当前项目合同决定"),
        empty_display(&draft.brief, "由当前项目合同决定"),
        user_message.trim(),
        chapter_scope
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_confirmation_inherits_persisted_full_book_execution_scope() {
        let draft = build_initial_creation_draft(
            "writer-prompt-full-book-scope",
            "fiction",
            "写一本10万字的历史架空小说，每章2500字。",
        )
        .expect("draft");
        let prompt = final_prompt_from_approved_creation_draft(
            &draft,
            &serde_json::json!({"project_path": "/tmp/full-book-scope"}),
            "确认合同，开始写作。",
        );

        assert!(
            prompt.contains("用户本轮要求直接生成完剩余内容"),
            "{prompt}"
        );
        assert!(prompt.contains("每轮最多章节：全部剩余章节"), "{prompt}");
        assert!(
            prompt.contains("__creation_execution_scope:all_remaining"),
            "{prompt}"
        );
    }
}

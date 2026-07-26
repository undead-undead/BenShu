use super::core::{Agent, MARKER_INTERJECTION};
use super::runtime_support::{
    PauseController, PreemptiveBridge, RuntimeExecutionSeed, RuntimeStageSignal,
};
use crate::agent::agent_identity::AgentIdentity;
use crate::agent::context::{BackgroundPressureBand, ContextManager};
use crate::agent::memory::{
    BackgroundCompressionDecision, BackgroundEnvelope, BackgroundQualitySignal,
    BackgroundSessionPersistenceStatus, Fact, FactProtection, FactReviewPayload, MemoryManager,
    RecentWindowSummary, RelationshipBackgroundLayer, SessionBackgroundState,
};
use crate::agent::message::{Content, ContentPart, Message, Role};
use crate::agent::multi_agent::{
    AgentMessage, MultiAgent, TextGenerationProgress, TextGenerationProgressSink,
    TextGenerationProgressStage,
};
use crate::agent::protocol::{
    AgentRole, ChatOutcome, ReasonerConfig, ReasoningStrategy, TaskOwnership, TokenUsage,
    ToolCallData,
};
use crate::agent::provider::Provider;
use crate::agent::reasoner::{runtime_session_title, Reasoner};
use crate::agent::session::{AgentSession, SessionStatus};
use crate::error::{Error, Result};
use crate::hooks::{HookResult, HookTiming, RuntimeHookCapture, RuntimeHookRefs};
use crate::skills::tool::{
    capability_route_requires_real_tool_call, classify_extended_pre_flight_level,
    classify_query_capability_route, extended_pre_flight_allows_auto_stepdown,
    extended_pre_flight_runs_complexity_estimator, extended_pre_flight_runs_jit_distillation,
    query_requests_image_generation, should_run_extended_pre_flight_for_turn, CapabilityRouteHint,
    ExtendedPreFlightLevel, ToolDefinition,
};
use benshu_hardness::{
    decide_initial_reasoning_strategy, is_explicit_image_generation_first_attempt,
    InitialReasoningStrategy, InitialReasoningStrategyInput,
};
use benshu_infra::agent::AgentEventData;
use benshu_provider_core::StreamingChoice;
use benshu_state::{TaskArtifactRef, TaskCheckpoint, TaskState, TaskStatus};
use benshu_telemetry::{RuntimeStage, TraceStatus};
use chrono::Utc;
use futures::StreamExt;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, instrument, warn};
use uuid::Uuid;

const CHAT_RESPONSE_INLINE_CHAR_LIMIT: usize = 20_000;
const CHAT_RESPONSE_INLINE_HEAD_CHARS: usize = 12_000;
const CHAT_RESPONSE_INLINE_TAIL_CHARS: usize = 4_000;
const CONTINUOUS_TEXT_PREFILL_TOKENS_PER_SECOND_FLOOR: u64 = 180;
const CONTINUOUS_TEXT_WHITESPACE_TAIL_LIMIT: usize = 512;
const CREATION_PLANNING_DIALOGUE_MARKER: &str = "[BENSHU_CREATION_PLANNING_DIALOGUE]";

impl<P: Provider + 'static> Agent<P> {
    fn estimate_prompt_tokens_for_prefill_timeout(prompt: &str) -> u64 {
        let chars = prompt.chars().count() as u64;
        let ascii = prompt.chars().filter(|ch| ch.is_ascii()).count() as u64;
        let non_ascii = chars.saturating_sub(ascii);
        let ascii_tokens = ascii.div_ceil(4);
        let non_ascii_tokens = non_ascii.div_ceil(2);
        ascii_tokens
            .saturating_add(non_ascii_tokens)
            .max(chars.div_ceil(4))
    }

    fn dynamic_continuous_text_first_chunk_timeout_secs(
        prompt: &str,
        configured_timeout_secs: u64,
    ) -> u64 {
        if Self::prompt_is_creation_planning_dialogue(prompt) {
            return configured_timeout_secs.clamp(30, 60);
        }
        let estimated_prompt_tokens = Self::estimate_prompt_tokens_for_prefill_timeout(prompt);
        let prefill_budget_secs = estimated_prompt_tokens
            .div_ceil(CONTINUOUS_TEXT_PREFILL_TOKENS_PER_SECOND_FLOOR)
            .saturating_add(30);
        configured_timeout_secs
            .max(prefill_budget_secs)
            .clamp(10, 600)
    }

    fn dynamic_continuous_text_idle_timeout_secs(
        prompt: &str,
        configured_timeout_secs: u64,
    ) -> u64 {
        if Self::prompt_is_creation_planning_dialogue(prompt) {
            return configured_timeout_secs.clamp(60, 180);
        }
        configured_timeout_secs.clamp(15, 180)
    }

    fn continuous_text_whitespace_tail_limit(
        target_chars: Option<usize>,
        generated_non_whitespace_chars: usize,
    ) -> usize {
        let useful_floor = target_chars
            .filter(|value| *value > 0)
            .map(|target| target.saturating_div(5).clamp(256, 1024))
            .unwrap_or(512);
        if generated_non_whitespace_chars >= useful_floor {
            CONTINUOUS_TEXT_WHITESPACE_TAIL_LIMIT
        } else {
            usize::MAX
        }
    }

    fn prompt_is_creation_planning_dialogue(prompt: &str) -> bool {
        prompt.contains(CREATION_PLANNING_DIALOGUE_MARKER)
            || prompt.contains("BENSHU_CREATION_PLANNING_DIALOGUE")
    }

    fn is_lightweight_realtime_tool_trace(tool_trace: &[ToolCallData]) -> bool {
        !tool_trace.is_empty()
            && tool_trace.iter().all(|call| {
                matches!(
                    call.name.as_str(),
                    "weather_lookup" | "price_lookup" | "fx_lookup" | "latest_info_lookup"
                )
            })
    }

    fn user_text_requests_background_write(text: &str) -> bool {
        let lower = text.to_lowercase();
        text.contains("记住")
            || text.contains("长期记忆")
            || text.contains("长期偏好")
            || text.contains("背景层")
            || lower.contains("remember")
            || lower.contains("long-term memory")
            || lower.contains("stable background")
    }

    fn message_has_durable_runtime_state(message: &Message) -> bool {
        if message.source_path.is_some() || message.source_collection.is_some() {
            return true;
        }
        if message
            .metadata
            .get("runtime_effect")
            .is_some_and(|effect| {
                effect.contains("artifact.")
                    || effect.contains("knowledge.imported")
                    || effect.contains("continuous.")
                    || effect.contains("task.")
            })
        {
            return true;
        }
        [
            "task_state",
            "task_title",
            "task_goal",
            "task_completed",
            "task_pending",
            "artifact_path",
            "artifact_uri",
            "output_path",
            "checkpoint_path",
            "continuous_task_id",
            "test_result",
            "verification_result",
        ]
        .iter()
        .any(|key| {
            message
                .metadata
                .get(*key)
                .is_some_and(|value| !value.trim().is_empty())
        })
    }

    fn should_attempt_background_refresh_after_turn(
        messages: &[Message],
        tool_trace: &[ToolCallData],
        pressure_band: BackgroundPressureBand,
        current_background: Option<&BackgroundEnvelope>,
    ) -> bool {
        if Self::is_lightweight_realtime_tool_trace(tool_trace) {
            return false;
        }
        if matches!(
            pressure_band,
            BackgroundPressureBand::High | BackgroundPressureBand::Critical
        ) {
            return true;
        }
        if messages.iter().any(Self::message_has_durable_runtime_state) {
            return true;
        }
        if messages.iter().rev().take(4).any(|message| {
            matches!(message.role, Role::User)
                && Self::user_text_requests_background_write(&message.content.as_text())
        }) {
            return true;
        }

        let conversational_messages = messages
            .iter()
            .filter(|message| !matches!(message.role, Role::System))
            .count();
        conversational_messages >= 12
            && current_background.is_some_and(|background| !background.is_empty())
    }

    fn compact_assistant_response_for_chat_history(response: &str) -> String {
        let count = response.chars().count();
        if count <= CHAT_RESPONSE_INLINE_CHAR_LIMIT {
            return response.to_string();
        }

        let head = response
            .chars()
            .take(CHAT_RESPONSE_INLINE_HEAD_CHARS)
            .collect::<String>();
        let tail = response
            .chars()
            .rev()
            .take(CHAT_RESPONSE_INLINE_TAIL_CHARS)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>();
        format!(
            "{head}\n\n[聊天历史保护：本次助手输出共 {count} 字符，已截断为预览。长正文应由写作/文件工具保存为 artifact，并在聊天中交付进度、章节号、字数、文件路径、摘要和审查状态。]\n\n{tail}"
        )
    }

    fn latest_user_text(messages: &[Message]) -> Option<String> {
        messages
            .iter()
            .rev()
            .find(|message| message.role == Role::User)
            .map(|message| message.content.as_text())
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
    }

    fn query_requests_capability_self_description(query: &str) -> bool {
        let normalized = query.trim().to_lowercase();
        if normalized.is_empty() {
            return false;
        }

        let capability_markers = [
            "你能做什么",
            "你可以做什么",
            "你会做什么",
            "你有什么能力",
            "你有哪些能力",
            "你有什么功能",
            "你有哪些功能",
            "你支持什么",
            "你会什么",
            "你能帮我做什么",
            "当前工具",
            "可用工具",
            "有哪些工具",
            "可用 worker",
            "有哪些 worker",
            "what can you do",
            "what are your capabilities",
            "available capabilities",
            "available tools",
            "what tools",
            "available workers",
            "what workers",
        ];

        capability_markers
            .iter()
            .any(|marker| normalized.contains(marker))
    }

    fn query_prefers_chinese_response(query: &str) -> bool {
        query.chars().any(|ch| {
            ('\u{4e00}'..='\u{9fff}').contains(&ch) || ('\u{3400}'..='\u{4dbf}').contains(&ch)
        })
    }

    fn truncate_capability_description(text: &str, max_chars: usize) -> String {
        let trimmed = text.trim().replace('\n', " ");
        if trimmed.chars().count() <= max_chars {
            return trimmed;
        }
        let mut out = trimmed.chars().take(max_chars).collect::<String>();
        out.push_str("...");
        out
    }

    fn worker_roles_from_delegate_definition(definition: &ToolDefinition) -> Vec<String> {
        definition
            .parameters
            .get("properties")
            .and_then(|properties| properties.get("role"))
            .and_then(|role| role.get("enum"))
            .and_then(|values| values.as_array())
            .into_iter()
            .flatten()
            .filter_map(|value| value.as_str())
            .filter(|role| *role != "auto")
            .map(str::to_string)
            .collect()
    }

    fn build_capability_self_description_response(
        query: &str,
        definitions: &[ToolDefinition],
    ) -> String {
        let zh = Self::query_prefers_chinese_response(query);
        let mut sorted = definitions.to_vec();
        sorted.sort_by(|left, right| left.name.cmp(&right.name));

        let worker_roles = sorted
            .iter()
            .find(|definition| definition.name == "delegate")
            .map(Self::worker_roles_from_delegate_definition)
            .unwrap_or_default();

        let visible_tools = sorted
            .iter()
            .take(16)
            .map(|definition| {
                format!(
                    "- `{}`: {}",
                    definition.name,
                    Self::truncate_capability_description(&definition.description, 96)
                )
            })
            .collect::<Vec<_>>();

        if zh {
            let workers = if worker_roles.is_empty() {
                "当前没有从运行时目录暴露具体 worker 名称；如果 `delegate` 可用，我会按 worker policy 自动选择。"
                    .to_string()
            } else {
                format!("当前可调度 worker：{}。", worker_roles.join("、"))
            };
            let tools = if visible_tools.is_empty() {
                "当前没有启用可见工具。".to_string()
            } else {
                let hidden = sorted.len().saturating_sub(visible_tools.len());
                let suffix = if hidden > 0 {
                    format!("\n- 另有 {hidden} 个工具未在摘要里展开，可继续问完整列表。")
                } else {
                    String::new()
                };
                format!(
                    "当前运行时可见工具摘要：\n{}{}",
                    visible_tools.join("\n"),
                    suffix
                )
            };
            return format!(
                "我是 BenShu，前台只由我和你对话。我会直接处理普通聊天、解释、轻量推理、会话上下文和记忆/RAG 相关问题；当任务需要搜索、文件、代码、写作、PDF、知识入库、语音或长任务执行时，我会按当前面板/worker/tool 配置路由给最合适的单责 worker。\n\n{workers}\n\n{tools}\n\n这份能力说明来自当前运行时工具定义和 worker 目录，不是固定文案；你新增或卸载 worker/tool 后，这里会跟着变化。"
            );
        }

        let workers = if worker_roles.is_empty() {
            "No concrete worker names are exposed by the runtime directory right now; when `delegate` is enabled I will select through worker policy.".to_string()
        } else {
            format!("Current dispatchable workers: {}.", worker_roles.join(", "))
        };
        let tools = if visible_tools.is_empty() {
            "No visible tools are enabled right now.".to_string()
        } else {
            let hidden = sorted.len().saturating_sub(visible_tools.len());
            let suffix = if hidden > 0 {
                format!(
                    "\n- {hidden} more tools are available; ask for the full list to expand them."
                )
            } else {
                String::new()
            };
            format!(
                "Current runtime-visible tool summary:\n{}{}",
                visible_tools.join("\n"),
                suffix
            )
        };
        format!(
            "I am BenShu, the single visible frontstage agent. I answer normal chat, explanations, lightweight reasoning, session context, memory, and RAG directly. When a task needs search, files, code, writing, PDF work, knowledge import, voice, or durable long-running execution, I route it to the narrowest configured worker.\n\n{workers}\n\n{tools}\n\nThis capability summary is built from the current runtime tool definitions and worker directory, so it updates when workers or tools change."
        )
    }

    async fn maybe_handle_capability_self_description(
        &self,
        messages: &[Message],
        execution_seed: &RuntimeExecutionSeed,
        input_message_count: usize,
    ) -> Result<Option<ChatOutcome>> {
        let Some(query) = Self::latest_user_text(messages) else {
            return Ok(None);
        };
        if !Self::query_requests_capability_self_description(&query) {
            return Ok(None);
        }

        let enabled_snapshot = self
            .enabled_tools
            .as_ref()
            .map(|enabled| enabled.read().clone());
        let definitions = self
            .tools
            .definitions_filtered(enabled_snapshot.as_ref())
            .await;
        let response = Self::build_capability_self_description_response(&query, &definitions);

        let mut persisted_messages = messages
            .iter()
            .filter(|message| !Self::is_transient_runtime_system_message(message))
            .cloned()
            .collect::<Vec<_>>();
        persisted_messages.push(Message::assistant(response.clone()));
        self.checkpoint(&persisted_messages, 1, SessionStatus::Completed)
            .await?;

        let outcome = ChatOutcome {
            response,
            thoughts: vec![],
            tool_calls: vec![],
            metabolic_stats: Some(self.current_metabolic_pressure()),
            ownership: TaskOwnership::direct(
                self.config.role.clone(),
                execution_seed.session_id.clone(),
            ),
            delegation: None,
            handover: None,
            runtime_task: None,
            run_trace: None,
        };
        Ok(Some(self.attach_runtime_refs(
            outcome,
            execution_seed,
            messages,
            input_message_count,
        )))
    }

    fn direct_memory_slot_key(text: &str) -> Option<String> {
        let lowered = text.to_lowercase();
        let slot_term = [
            "测试验证码",
            "验证码",
            "手机号",
            "电话",
            "地址",
            "偏好",
            "名字",
            "姓名",
            "邮箱",
            "标记",
            "账号",
            "密码",
            "生日",
            "token",
            "code",
            "phone",
            "email",
            "preference",
            "name",
            "address",
        ]
        .into_iter()
        .find(|term| lowered.contains(term))?;

        let marker = text
            .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.'))
            .find(|token| {
                token.chars().count() >= 8
                    && token
                        .chars()
                        .any(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit())
                    && token
                        .chars()
                        .any(|ch| ch == '-' || ch == '_' || ch.is_ascii_digit())
            });

        Some(match marker {
            Some(marker) => format!("{slot_term} {marker}"),
            None => slot_term.to_string(),
        })
    }

    fn query_prefers_transient_session_context(text: &str) -> bool {
        let lowered = text.to_lowercase();
        [
            "当前会话",
            "同会话",
            "这轮会话",
            "本轮会话",
            "临时暗号",
            "不要保存为长期记忆",
            "不要写入长期记忆",
            "只根据当前会话",
            "same session",
            "current session",
            "this session",
            "do not save",
            "don't save",
        ]
        .iter()
        .any(|needle| lowered.contains(&needle.to_lowercase()))
    }

    fn query_should_skip_direct_memory_crud(text: &str) -> bool {
        let lowered = text.to_lowercase();
        Self::query_prefers_transient_session_context(text)
            || [
                "知识库",
                "资料库",
                "网页",
                "网址",
                "保存进知识库",
                "保存到知识库",
                "写入知识库",
                "导入知识库",
                "多模态",
                "受治理记忆",
                "模态",
                "source_url",
                "artifact_locator",
                "generation_provenance",
                "understanding",
                "knowledge base",
                "knowledge-base",
                "import url",
                "multimodal",
                "writeback",
            ]
            .iter()
            .any(|needle| lowered.contains(&needle.to_lowercase()))
            || lowered.contains("http://")
            || lowered.contains("https://")
    }

    fn has_explicit_long_term_memory_intent(text: &str) -> bool {
        let lowered = text.to_lowercase();
        [
            "长期记忆",
            "长久记住",
            "永久记住",
            "以后都记住",
            "记住我的",
            "记住我",
            "请记住",
            "帮我记住",
            "保存到长期记忆",
            "写入长期记忆",
            "存到长期记忆",
            "从你的记忆里",
            "从长期记忆里",
            "你记忆里",
            "你还记得我的",
            "你记得我的",
            "忘记我之前告诉你的",
            "删除你记忆里的",
            "从记忆里删除",
            "更新你记忆里的",
            "修改你记忆里的",
            "复核你记忆里的",
            "标记为需要复核",
            "remember my",
            "remember that my",
            "please remember",
            "save to long-term memory",
            "store in long-term memory",
            "from your memory",
            "forget what i told you",
            "delete from memory",
            "update your memory",
        ]
        .iter()
        .any(|needle| lowered.contains(&needle.to_lowercase()))
    }

    fn direct_memory_extract_value(text: &str) -> Option<String> {
        for (left, right) in [('「', '」'), ('“', '”'), ('"', '"'), ('\'', '\'')] {
            if let Some((_, rest)) = text.split_once(left) {
                if let Some((value, _)) = rest.split_once(right) {
                    let value = value.trim();
                    if !value.is_empty() {
                        return Some(value.to_string());
                    }
                }
            }
        }

        let markers = [
            "当前正确值是",
            "正确值是",
            "现在改成",
            "改成",
            "改为",
            "更新为",
            "修改为",
            "现在是",
            "是",
        ];
        for marker in markers {
            if let Some((_, rest)) = text.split_once(marker) {
                let value = rest
                    .trim()
                    .trim_matches(|ch: char| {
                        matches!(
                            ch,
                            '。' | '，' | ',' | '.' | '；' | ';' | ':' | '：' | ' ' | '\n'
                        )
                    })
                    .split(|ch: char| matches!(ch, '。' | '，' | ',' | '；' | ';' | '\n'))
                    .next()
                    .unwrap_or("")
                    .trim()
                    .trim_matches(|ch: char| ch == '「' || ch == '」' || ch == '"' || ch == '\'');
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }

        None
    }

    fn direct_memory_fact_matches_slot(content: &str, slot_key: &str) -> bool {
        let content = content.to_lowercase();
        slot_key
            .to_lowercase()
            .split_whitespace()
            .all(|part| content.contains(part))
    }

    fn direct_memory_value_from_content(slot_key: &str, content: &str) -> String {
        for separator in ["：", ":"] {
            if let Some((left, right)) = content.split_once(separator) {
                if left.trim().contains(slot_key) {
                    let value = right.trim();
                    if !value.is_empty() {
                        return value.to_string();
                    }
                }
            }
        }
        content.trim().to_string()
    }

    async fn direct_memory_delete_slot(
        memory: &Arc<dyn crate::agent::memory::Memory>,
        slot_key: &str,
    ) -> Result<usize> {
        let facts = memory.retrieve_facts("default", None).await?;
        let mut deleted = 0usize;
        for fact in facts {
            if matches!(fact.protection, FactProtection::Normal)
                && Self::direct_memory_fact_matches_slot(&fact.content, slot_key)
            {
                memory.delete_fact("default", None, &fact.id).await?;
                deleted += 1;
            }
        }
        Ok(deleted)
    }

    async fn direct_memory_store_slot(
        memory: &Arc<dyn crate::agent::memory::Memory>,
        slot_key: &str,
        value: &str,
    ) -> Result<()> {
        Self::direct_memory_delete_slot(memory, slot_key).await?;
        let mut fact = Fact::new(format!("{}：{}", slot_key, value.trim()), "personal_memory");
        fact.verified = true;
        memory.store_fact("default", None, fact).await?;
        Ok(())
    }

    async fn direct_memory_lookup_slot(
        memory: &Arc<dyn crate::agent::memory::Memory>,
        slot_key: &str,
    ) -> Result<Option<String>> {
        let mut facts = memory.retrieve_facts("default", None).await?;
        facts.retain(|fact| Self::direct_memory_fact_matches_slot(&fact.content, slot_key));
        facts.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(facts
            .first()
            .map(|fact| Self::direct_memory_value_from_content(slot_key, &fact.content)))
    }

    async fn maybe_handle_direct_memory_crud(
        &self,
        messages: &[Message],
        execution_seed: &RuntimeExecutionSeed,
        input_message_count: usize,
    ) -> Result<Option<ChatOutcome>> {
        let Some(memory) = &self.memory else {
            return Ok(None);
        };
        let Some(query) = Self::latest_user_text(messages) else {
            return Ok(None);
        };
        if Self::query_should_skip_direct_memory_crud(&query) {
            return Ok(None);
        }
        if !Self::has_explicit_long_term_memory_intent(&query) {
            return Ok(None);
        }
        let Some(slot_key) = Self::direct_memory_slot_key(&query) else {
            return Ok(None);
        };

        let conditional_deleted_check =
            query.contains("如果已经删除") || query.contains("若已删除");
        let wants_review =
            query.contains("复核") || query.contains("纠错") || query.contains("需要复核");
        let wants_protect = query.contains("保护")
            || query.contains("置顶")
            || query.to_lowercase().contains("protect")
            || query.to_lowercase().contains("pin");
        let wants_delete = !conditional_deleted_check
            && (query.contains("删除") || query.contains("忘记") || query.contains("清除"));
        let wants_update =
            query.contains("更新") || query.contains("改成") || query.contains("修改");
        let wants_save =
            query.contains("记住") || query.contains("保存到长期记忆") || query.contains("保存");
        let wants_lookup = query.contains("找回")
            || query.contains("查")
            || query.contains("记忆")
            || query.contains("记得")
            || query.contains("只回答");

        let response = if wants_delete {
            Self::direct_memory_delete_slot(memory, &slot_key).await?;
            "我已经删除了。".to_string()
        } else if wants_update || wants_save {
            let Some(value) = Self::direct_memory_extract_value(&query) else {
                return Ok(None);
            };
            Self::direct_memory_store_slot(memory, &slot_key, &value).await?;
            if wants_update {
                "我已经更新了。".to_string()
            } else {
                "我已经记住了。".to_string()
            }
        } else if wants_protect {
            let facts = memory.retrieve_facts("default", None).await?;
            let target = if query.contains("置顶") || query.to_lowercase().contains("pin") {
                FactProtection::Pinned
            } else {
                FactProtection::Protected
            };
            let mut updated = 0usize;
            for fact in facts {
                if Self::direct_memory_fact_matches_slot(&fact.content, &slot_key) {
                    memory
                        .set_fact_protection("default", None, &fact.id, target.clone())
                        .await?;
                    updated += 1;
                }
            }
            if updated == 0 {
                "没有找到可保护的事实。".to_string()
            } else {
                "我已经保护了。".to_string()
            }
        } else if wants_review {
            let facts = memory.retrieve_facts("default", None).await?;
            let mut updated = 0usize;
            for fact in facts {
                if Self::direct_memory_fact_matches_slot(&fact.content, &slot_key) {
                    memory
                        .request_fact_review(
                            &fact.id,
                            FactReviewPayload {
                                review_reason: Some("user_challenge".to_string()),
                                challenger_summary: Some(query.clone()),
                                challenger_source: Some(
                                    "natural_language_memory_management".to_string(),
                                ),
                                review_requested_at: Some(Utc::now()),
                                resolution: None,
                            },
                        )
                        .await?;
                    updated += 1;
                }
            }
            if updated == 0 {
                "没有找到可标记复核的事实。".to_string()
            } else {
                "我已经标记为需要复核。".to_string()
            }
        } else if wants_lookup {
            Self::direct_memory_lookup_slot(memory, &slot_key)
                .await?
                .unwrap_or_else(|| "没有找到".to_string())
        } else {
            return Ok(None);
        };

        let mut persisted_messages = messages
            .iter()
            .filter(|message| !Self::is_transient_runtime_system_message(message))
            .cloned()
            .collect::<Vec<_>>();
        persisted_messages.push(Message::assistant(response.clone()));
        self.checkpoint(&persisted_messages, 1, SessionStatus::Completed)
            .await?;

        let outcome = ChatOutcome {
            response,
            thoughts: vec![],
            tool_calls: vec![],
            metabolic_stats: Some(self.current_metabolic_pressure()),
            ownership: TaskOwnership::direct(
                self.config.role.clone(),
                execution_seed.session_id.clone(),
            ),
            delegation: None,
            handover: None,
            runtime_task: None,
            run_trace: None,
        };
        Ok(Some(self.attach_runtime_refs(
            outcome,
            execution_seed,
            messages,
            input_message_count,
        )))
    }

    fn latest_user_requires_execution_tool(messages: &[Message]) -> bool {
        let Some(query) = messages
            .iter()
            .rev()
            .find(|message| message.role == Role::User)
            .map(|message| message.content.as_text())
        else {
            return false;
        };

        let normalized = query.trim();
        if normalized.is_empty() {
            return false;
        }
        if Self::query_prefers_transient_session_context(normalized) {
            return false;
        }

        let has_media_input = messages
            .iter()
            .rev()
            .find(|message| message.role == Role::User)
            .is_some_and(|message| {
                matches!(
                    &message.content,
                    Content::Parts(parts)
                        if parts.iter().any(|part| matches!(
                            part,
                            ContentPart::Image { .. }
                                | ContentPart::Audio { .. }
                                | ContentPart::Video { .. }
                        ))
                )
            });

        classify_query_capability_route(normalized).is_some_and(|route| {
            if has_media_input
                && matches!(
                    route,
                    CapabilityRouteHint::DocumentUnderstanding
                        | CapabilityRouteHint::VisualUnderstanding
                )
            {
                return false;
            }
            capability_route_requires_real_tool_call(route)
        })
    }

    fn has_recent_execution_required_prompt(messages: &[Message]) -> bool {
        messages.iter().rev().take(8).any(|message| {
            message.role == Role::System
                && message.content.as_text().contains(
                    crate::agent::reasoner::reasoner_constants::MARKER_TOOL_EXECUTION_REQUIRED,
                )
        })
    }

    fn maybe_inject_retry_execution_guard(messages: &mut Vec<Message>, error_ref: Option<&str>) {
        let Some(error_text) = error_ref.map(str::trim).filter(|value| !value.is_empty()) else {
            return;
        };

        if !Self::latest_user_requires_execution_tool(messages)
            || Self::has_recent_execution_required_prompt(messages)
        {
            return;
        }

        messages.push(Message::system(format!(
            "{}\n\nThe previous execution attempt failed because of a runtime/provider issue:\n{}\n\nThe latest user turn still requires a real tool execution path.\nYou must either:\n1. call the single best matching tool now, or\n2. reply plainly with the concrete runtime blocker.\nDo not switch into a long text-only answer that pretends the requested execution already happened.",
            crate::agent::reasoner::reasoner_constants::MARKER_TOOL_EXECUTION_REQUIRED,
            error_text
        )));
    }

    fn is_transient_runtime_system_message(message: &Message) -> bool {
        if message.role != Role::System {
            return false;
        }

        let content = message.content.as_text();
        let trimmed = content.trim();
        trimmed.starts_with("Current Session ID:")
            || trimmed.starts_with(
                crate::agent::reasoner::reasoner_constants::MARKER_TOOL_EXECUTION_REQUIRED,
            )
            || trimmed.starts_with(MARKER_INTERJECTION)
    }

    fn media_part_from_locator(locator: &str) -> Option<ContentPart> {
        let trimmed = locator.trim();
        if trimmed.is_empty() {
            return None;
        }

        let direct_url = trimmed.starts_with("file://")
            || trimmed.starts_with("http://")
            || trimmed.starts_with("https://");
        if direct_url {
            return Some(ContentPart::Image {
                source: crate::agent::message::ImageSource::Url {
                    url: trimmed.to_string(),
                },
            });
        }

        let direct_path = std::path::Path::new(trimmed);
        if direct_path.is_absolute() && direct_path.exists() {
            return Some(ContentPart::Image {
                source: crate::agent::message::ImageSource::Url {
                    url: format!("file://{}", trimmed),
                },
            });
        }

        if let Some(offset) = trimmed.find("/home/") {
            let candidate = &trimmed[offset..];
            let candidate_path = std::path::Path::new(candidate);
            if candidate_path.is_absolute() && candidate_path.exists() {
                return Some(ContentPart::Image {
                    source: crate::agent::message::ImageSource::Url {
                        url: format!("file://{}", candidate),
                    },
                });
            }
        }

        None
    }

    fn extract_media_parts(message: &Message) -> Vec<ContentPart> {
        let mut media_parts = match &message.content {
            Content::Parts(parts) => parts
                .iter()
                .filter_map(|part| match part {
                    ContentPart::Image { .. }
                    | ContentPart::Audio { .. }
                    | ContentPart::Video { .. } => Some(part.clone()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };

        if !media_parts.is_empty() {
            return media_parts;
        }

        let locator_candidates = [
            message.metadata.get("multimodal_source_path"),
            message.metadata.get("multimodal_source_url"),
            message.metadata.get("media_preprocess_source_ref"),
            message.source_path.as_ref(),
        ];
        for locator in locator_candidates.into_iter().flatten() {
            if let Some(media_part) = Self::media_part_from_locator(locator) {
                media_parts.push(media_part);
                break;
            }
        }

        media_parts
    }

    fn latest_user_media_message(messages: &[Message], before_index: usize) -> Option<&Message> {
        messages[..before_index].iter().rev().find(|message| {
            message.role == Role::User && !Self::extract_media_parts(message).is_empty()
        })
    }

    fn maybe_attach_recent_media_to_followup(messages: &mut [Message]) -> Option<(usize, Content)> {
        let last_user_index = messages
            .iter()
            .rposition(|message| message.role == Role::User)?;
        if Self::latest_user_message_has_media(&messages[last_user_index..=last_user_index]) {
            return None;
        }

        let source_message = Self::latest_user_media_message(messages, last_user_index)?;
        let media_parts = Self::extract_media_parts(source_message);
        if media_parts.is_empty() {
            return None;
        }
        let source_ref = source_message
            .metadata
            .get("media_preprocess_source_ref")
            .cloned()
            .or_else(|| {
                source_message
                    .metadata
                    .get("multimodal_source_path")
                    .cloned()
            });

        let original_content = messages[last_user_index].content.clone();
        let mut combined_parts = match &messages[last_user_index].content {
            Content::Text(text) => vec![ContentPart::Text { text: text.clone() }],
            Content::Parts(parts) => parts.clone(),
            other => vec![ContentPart::Text {
                text: other.as_text(),
            }],
        };
        combined_parts.extend(media_parts);

        messages[last_user_index].content = Content::Parts(combined_parts);
        messages[last_user_index]
            .metadata
            .insert("session_media_continuation".to_string(), "true".to_string());
        if let Some(source_ref) = source_ref {
            messages[last_user_index]
                .metadata
                .insert("session_media_continuation_source".to_string(), source_ref);
        }

        tracing::info!(
            "ForegroundRuntime: attached latest session media to follow-up user turn for multimodal continuity"
        );

        Some((last_user_index, original_content))
    }

    fn attach_provider_media_metadata_from_runtime_capture(
        message: &mut Message,
        hook_capture: &RuntimeHookCapture,
    ) {
        const PROVIDER_MEDIA_NOTE_MAPPINGS: [(&str, &str); 10] = [
            (
                "after_llm:provider_media_preprocess_consumed_by:",
                "provider_media_preprocess_consumed_by",
            ),
            (
                "after_llm:provider_media_preprocess_consumption_routes:",
                "provider_media_preprocess_consumption_routes",
            ),
            (
                "after_llm:provider_media_preprocess_outcomes:",
                "provider_media_preprocess_outcomes",
            ),
            (
                "after_llm:provider_media_preprocess_preprocess_failed_routes:",
                "provider_media_preprocess_preprocess_failed_routes",
            ),
            (
                "after_llm:provider_media_preprocess_model_failed_routes:",
                "provider_media_preprocess_model_failed_routes",
            ),
            (
                "after_llm:provider_media_preprocess_result_insufficient_routes:",
                "provider_media_preprocess_result_insufficient_routes",
            ),
            (
                "after_llm:provider_media_preprocess_followup_strategies:",
                "provider_media_preprocess_followup_strategies",
            ),
            (
                "after_llm:provider_media_preprocess_attachment_fallback_routes:",
                "provider_media_preprocess_attachment_fallback_routes",
            ),
            (
                "after_llm:provider_media_preprocess_alternate_model_fallback_routes:",
                "provider_media_preprocess_alternate_model_fallback_routes",
            ),
            (
                "after_llm:provider_media_preprocess_clarification_routes:",
                "provider_media_preprocess_clarification_routes",
            ),
        ];

        for note in &hook_capture.notes {
            for (prefix, metadata_key) in PROVIDER_MEDIA_NOTE_MAPPINGS {
                if let Some(value) = note.strip_prefix(prefix) {
                    let trimmed = value.trim();
                    if !trimmed.is_empty() {
                        message
                            .metadata
                            .insert(metadata_key.to_string(), trimmed.to_string());
                    }
                }
            }
        }
    }

    fn record_background_decision(&self, decision: BackgroundCompressionDecision) {
        let mut stats = self.background_runtime_stats.write();
        stats.total_attempts = stats.total_attempts.saturating_add(1);
        match decision {
            BackgroundCompressionDecision::Skip => {
                stats.skip_count = stats.skip_count.saturating_add(1);
            }
            BackgroundCompressionDecision::RejectCandidate => {
                stats.reject_count = stats.reject_count.saturating_add(1);
            }
            BackgroundCompressionDecision::RefreshSessionLayer => {
                stats.refresh_session_count = stats.refresh_session_count.saturating_add(1);
            }
            BackgroundCompressionDecision::PromoteRelationshipFact => {
                stats.promote_relationship_count =
                    stats.promote_relationship_count.saturating_add(1);
            }
            BackgroundCompressionDecision::RewriteWholeEnvelope => {
                stats.rewrite_count = stats.rewrite_count.saturating_add(1);
            }
        }
    }

    fn background_memory_manager(&self) -> Option<&MemoryManager> {
        self.memory
            .as_ref()
            .and_then(|memory| memory.as_any().downcast_ref::<MemoryManager>())
    }

    fn select_auto_stepdown_model(&self) -> Option<String> {
        self.config
            .auto_stepdown_targets
            .iter()
            .find_map(|(match_substring, target_model)| {
                if self.config.model.contains(match_substring) && self.config.model != *target_model
                {
                    Some(target_model.clone())
                } else {
                    None
                }
            })
    }

    fn latest_user_message_has_media(messages: &[Message]) -> bool {
        messages.iter().rev().any(|message| {
            message.role == Role::User
                && matches!(
                    &message.content,
                    Content::Parts(parts)
                        if parts.iter().any(|part| matches!(
                            part,
                            crate::agent::message::ContentPart::Image { .. }
                                | crate::agent::message::ContentPart::Audio { .. }
                                | crate::agent::message::ContentPart::Video { .. }
                        ))
                )
        })
    }

    fn should_hydrate_existing_session(messages: &[Message]) -> bool {
        !messages.is_empty()
            && !messages
                .iter()
                .any(|message| matches!(message.role, Role::Assistant | Role::Tool))
    }

    fn should_run_extended_pre_flight(
        last_user_msg: &str,
        direct_route: Option<CapabilityRouteHint>,
        has_media_input: bool,
    ) -> bool {
        should_run_extended_pre_flight_for_turn(last_user_msg, direct_route, has_media_input)
    }

    fn classify_extended_pre_flight(
        last_user_msg: &str,
        direct_route: Option<CapabilityRouteHint>,
        has_media_input: bool,
    ) -> ExtendedPreFlightLevel {
        classify_extended_pre_flight_level(last_user_msg, direct_route, has_media_input)
    }

    fn is_explicit_image_generation_turn(
        last_user_msg: &str,
        has_media_input: bool,
        attempt: &crate::agent::attempt::Attempt,
    ) -> bool {
        is_explicit_image_generation_first_attempt(
            has_media_input,
            attempt.retry_count as usize,
            query_requests_image_generation(last_user_msg),
        )
    }

    async fn persist_background_envelope_with_retry(
        &self,
        manager: &MemoryManager,
        session_id: &str,
        envelope: BackgroundEnvelope,
        reason: &str,
    ) -> Result<BackgroundSessionPersistenceStatus> {
        let retry_count = self.config.background_persistence_retry_count;
        let backoff_ms = self.config.background_persistence_retry_backoff_ms;
        let mut last_error: Option<Error> = None;

        for attempt in 0..=retry_count {
            match manager
                .persist_background_envelope(session_id, envelope.clone(), reason)
                .await
            {
                Ok(status) => return Ok(status),
                Err(error) => {
                    if attempt == retry_count {
                        last_error = Some(error);
                        break;
                    }

                    warn!(
                        "Background session persistence attempt {} failed for session {}: {}",
                        attempt + 1,
                        session_id,
                        error
                    );
                    last_error = Some(error);
                    tokio::time::sleep(Duration::from_millis(
                        backoff_ms.saturating_mul((attempt + 1) as u64),
                    ))
                    .await;
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            Error::MemoryConsistency(format!(
                "background persistence failed for session {} without concrete error",
                session_id
            ))
        }))
    }

    async fn should_store_jit_fact(
        &self,
        memory: &Arc<dyn crate::agent::memory::Memory>,
        session_id: &str,
        candidate: &Fact,
    ) -> Result<bool> {
        let mut facts = memory.retrieve_facts(session_id, None).await?;
        facts.sort_by_key(|fact| std::cmp::Reverse(fact.updated_at));

        let cooldown_secs = self.config.jit_fact_cooldown_secs as i64;
        let now = Utc::now();

        for fact in facts
            .into_iter()
            .filter(|fact| fact.category == candidate.category && fact.source == candidate.source)
            .take(self.config.jit_fact_dedupe_limit)
        {
            if fact.content == candidate.content {
                return Ok(false);
            }

            let age_secs = (now - fact.updated_at).num_seconds().max(0);
            if cooldown_secs > 0 && age_secs <= cooldown_secs {
                return Ok(false);
            }
        }

        Ok(true)
    }

    fn adopt_recovered_session_state(&self, session: &AgentSession) {
        let mut background = self.background_envelope.write();
        *background = session.background_envelope.clone();

        let mut seen = self.seen_tools.write();
        seen.clear();
        for tool in &session.executed_tools {
            seen.insert(tool.clone());
        }
    }

    fn merge_rejected_session_layer(
        existing: Option<&SessionBackgroundState>,
        candidate: Option<SessionBackgroundState>,
    ) -> Option<SessionBackgroundState> {
        match (existing, candidate) {
            (None, None) => None,
            (Some(existing), None) => {
                let mut existing = existing.clone();
                existing.sync_backend_context_storage();
                Some(existing)
            }
            (None, Some(mut candidate)) => {
                candidate.sync_backend_context_storage();
                Some(candidate)
            }
            (Some(existing), Some(mut candidate)) => {
                candidate.sync_backend_context_storage();
                if candidate.active_topics.is_empty() {
                    candidate.active_topics = existing.active_topics.clone();
                }
                if candidate.backend_contexts.is_empty() {
                    candidate.backend_contexts = existing.backend_contexts.clone();
                }
                if candidate.backend_context_records.is_empty() {
                    candidate.backend_context_records = existing.backend_context_records.clone();
                }
                if candidate.retrieved_memory_objects.is_empty() {
                    candidate.retrieved_memory_objects = existing.retrieved_memory_objects.clone();
                }
                if candidate.web_session_objects.is_empty() {
                    candidate.web_session_objects = existing.web_session_objects.clone();
                }
                if candidate.artifact_session_objects.is_empty() {
                    candidate.artifact_session_objects = existing.artifact_session_objects.clone();
                }
                if candidate.task_session_objects.is_empty() {
                    candidate.task_session_objects = existing.task_session_objects.clone();
                }
                if candidate.tool_session_objects.is_empty() {
                    candidate.tool_session_objects = existing.tool_session_objects.clone();
                }
                if candidate.multimodal_session_objects.is_empty() {
                    candidate.multimodal_session_objects =
                        existing.multimodal_session_objects.clone();
                }
                if candidate.open_loops.is_empty() {
                    candidate.open_loops = existing.open_loops.clone();
                }
                if candidate.recent_emotional_state.is_none() {
                    candidate.recent_emotional_state = existing.recent_emotional_state.clone();
                }
                if candidate.ongoing_goals.is_empty() {
                    candidate.ongoing_goals = existing.ongoing_goals.clone();
                }
                if candidate.workspace_focus.is_none() {
                    candidate.workspace_focus = existing.workspace_focus.clone();
                }
                if candidate.pending_followups.is_empty() {
                    candidate.pending_followups = existing.pending_followups.clone();
                }
                if candidate.summary.is_none() {
                    candidate.summary = existing.summary.clone();
                }
                for (key, value) in &existing.metadata {
                    candidate
                        .metadata
                        .entry(key.clone())
                        .or_insert_with(|| value.clone());
                }
                candidate.sync_backend_context_storage();
                Some(candidate)
            }
        }
    }

    /// Create a reasoner for foreground and streaming execution paths.
    pub(crate) fn reasoner(&self) -> Reasoner<P> {
        self.reasoner_for_session(self.current_runtime_session_id())
    }

    pub(crate) fn reasoner_for_session(&self, session_id: Option<String>) -> Reasoner<P> {
        let reasoner_config = ReasonerConfig {
            agent_name: Some(self.config.name.clone()),
            model: self.config.model.clone(),
            preamble: self.config.preamble.clone(),
            temperature: self.config.temperature,
            max_tokens: self.config.max_tokens,
            session_id,
            inference_priority: self.local_inference_priority(),
            json_mode: self.config.json_mode,
            extra_params: self.config.extra_params.clone(),
            enable_cache_control: self.config.enable_cache_control,
            max_history_messages: self.config.max_history_messages,
            smart_pruning: self.config.smart_pruning,
            efficiency_trigger_secs: self.config.efficiency_trigger_secs,
            max_reflexion_retries: self.config.max_reflexion_retries,
            llm_timeout: self.config.llm_timeout,
        };

        Reasoner::new(
            self.provider.clone(),
            reasoner_config,
            self.tools.clone(),
            self.enabled_tools.clone(),
            self.tactical_orchestrator.clone(),
        )
    }

    pub(crate) fn current_runtime_session_id(&self) -> Option<String> {
        self.runtime_hook_refs
            .read()
            .as_ref()
            .and_then(|refs| refs.session_id.clone())
            .or_else(|| self.session_id.clone())
    }

    fn current_runtime_session_id_or_default(&self) -> String {
        self.current_runtime_session_id()
            .unwrap_or_else(|| "default".to_string())
    }

    pub(crate) fn local_inference_priority(&self) -> i8 {
        match &self.config.role {
            AgentRole::Strategist => -32,
            AgentRole::RiskAnalyst => -24,
            AgentRole::Custom(name) if name.eq_ignore_ascii_case("benshu") => -64,
            AgentRole::Researcher => -8,
            AgentRole::Trader => 0,
            AgentRole::Custom(_) => 0,
        }
    }

    /// Resolve the reasoning strategy for a given attempt, adapting to metabolic pressure and task complexity.
    pub(crate) fn resolve_reasoning_strategy(
        &self,
        attempt: &crate::agent::attempt::Attempt,
        messages: &[Message],
    ) -> ReasoningStrategy {
        let metabolic = self.current_metabolic_pressure();
        let last_user_msg = messages
            .iter()
            .rev()
            .find(|message| message.role == Role::User)
            .map(|message| message.content.as_text())
            .unwrap_or_default();
        let direct_route = classify_query_capability_route(&last_user_msg);
        let has_media_input = Self::latest_user_message_has_media(messages);
        let explicit_image_generation_turn =
            Self::is_explicit_image_generation_turn(&last_user_msg, has_media_input, attempt);
        let light_frontstage_turn = !has_media_input
            && matches!(
                Self::classify_extended_pre_flight(&last_user_msg, direct_route, has_media_input),
                ExtendedPreFlightLevel::None
            )
            && attempt.retry_count == 0;
        match decide_initial_reasoning_strategy(InitialReasoningStrategyInput {
            force_react_due_to_resource_pressure: metabolic.is_throttled
                && metabolic.vram_pressure > self.config.vram_react_stepdown_threshold,
            throttled_by_metabolic_guard: metabolic.is_throttled,
            reflexion_enabled: self.config.enable_reflexion,
            explicit_image_generation_turn,
            light_frontstage_turn,
            has_media_input,
        }) {
            InitialReasoningStrategy::ReAct => ReasoningStrategy::ReAct,
            InitialReasoningStrategy::Reflexion => ReasoningStrategy::Reflexion,
        }
    }

    pub(crate) fn build_runtime_task(
        &self,
        seed: &RuntimeExecutionSeed,
        outcome: &ChatOutcome,
        input_message_count: usize,
    ) -> TaskState {
        let mut task = TaskState::new(
            "foreground_chat",
            "Interactive agent foreground run",
            serde_json::json!({
                "entrypoint": "agent.chat_with_cancel",
                "input_message_count": input_message_count,
            }),
            self.config.name.clone(),
        );
        let now = Utc::now();
        task.id = seed.task_id;
        task.created_at = seed.started_at;
        task.updated_at = now;
        task.status = TaskStatus::Completed;
        task.result = Some(serde_json::json!({
            "response_text": outcome.response,
            "thought_count": outcome.thoughts.len(),
            "tool_call_count": outcome.tool_calls.len(),
            "handover": outcome.handover,
        }));
        task.current_step = outcome.tool_calls.len() as u32;
        task.total_steps = Some(outcome.tool_calls.len() as u32);
        task.checkpoints = outcome
            .tool_calls
            .iter()
            .enumerate()
            .map(|(idx, call)| TaskCheckpoint {
                step: (idx + 1) as u32,
                label: format!("tool:{}", call.name),
                recorded_at: now,
                summary: Some(format!(
                    "duration_ms={} success={} preview={}",
                    call.duration_ms,
                    call.result
                        .as_deref()
                        .map(|result| !result.to_ascii_lowercase().contains("error executing tool"))
                        .unwrap_or(false),
                    benshu_compression::preview_text(
                        call.result.as_deref().unwrap_or_default(),
                        240,
                    )
                )),
            })
            .collect();
        task.artifacts = outcome
            .tool_calls
            .iter()
            .flat_map(|call| {
                call.result
                    .as_deref()
                    .map(|result| Self::extract_tool_artifacts(&call.name, result))
                    .unwrap_or_default()
            })
            .collect();
        task.session_id = seed.session_id.clone();
        task.thread_id = Some(seed.thread_id.clone());
        task.run_id = Some(seed.run_id);
        task.trace_id = Some(seed.run_id);
        task.root_task_id = Some(seed.task_id);
        task.tags = vec!["foreground".to_string(), "chat".to_string()];
        task
    }

    fn extract_tool_artifacts(tool_name: &str, result: &str) -> Vec<TaskArtifactRef> {
        let mut artifacts = Vec::new();

        if let Ok(value) = serde_json::from_str::<serde_json::Value>(result) {
            if let Some(items) = value
                .get("evidence_artifacts")
                .and_then(|items| items.as_array())
            {
                for artifact in items {
                    let Some(uri) = artifact.get("uri").and_then(|value| value.as_str()) else {
                        continue;
                    };
                    artifacts.push(TaskArtifactRef {
                        artifact_id: format!("{tool_name}:{}", uuid::Uuid::new_v4()),
                        kind: artifact
                            .get("kind")
                            .and_then(|value| value.as_str())
                            .unwrap_or("tool_output")
                            .to_string(),
                        uri: uri.to_string(),
                        media_type: artifact
                            .get("media_type")
                            .and_then(|value| value.as_str())
                            .map(ToOwned::to_owned),
                    });
                }
            }
        }

        // Worker summaries are intentionally compact text, so delegate chains can
        // surface artifact paths without forcing every worker result to be JSON.
        for line in result.lines() {
            let Some(rest) = line.trim().strip_prefix("evidence_artifacts:") else {
                continue;
            };
            for uri in rest.split(',').map(str::trim).filter(|uri| !uri.is_empty()) {
                artifacts.push(TaskArtifactRef {
                    artifact_id: format!("{tool_name}:{}", uuid::Uuid::new_v4()),
                    kind: "tool_output".to_string(),
                    uri: uri.to_string(),
                    media_type: Some("text/plain".to_string()),
                });
            }
        }

        artifacts
    }

    pub(crate) fn derive_runtime_thread_id(session_id: Option<&str>, task_id: Uuid) -> String {
        session_id
            .map(|sid| sid.to_string())
            .unwrap_or_else(|| format!("thread:{}", task_id))
    }

    pub(crate) fn foreground_task_slot_key(session_id: Option<&str>) -> String {
        session_id.unwrap_or("__foreground_default__").to_string()
    }

    pub(crate) async fn cancel_foreground_task_for_session(
        &self,
        session_id: Option<&str>,
    ) -> bool {
        let session_slot =
            Self::foreground_task_slot_key(session_id.or(self.session_id.as_deref()));
        let previous_task = {
            let mut active_task = self.active_task.lock().await;
            active_task.remove(&session_slot)
        };

        if let Some(previous_task) = previous_task {
            previous_task.cancel_token.cancel();
            tokio::spawn(async move {
                if let Err(error) = previous_task.join_handle.await {
                    warn!("Cancelled foreground task cleanup failed: {}", error);
                }
            });
            true
        } else {
            false
        }
    }

    pub(crate) fn has_active_foreground_task_for_session_internal(
        &self,
        session_id: Option<&str>,
    ) -> bool {
        let session_slot =
            Self::foreground_task_slot_key(session_id.or(self.session_id.as_deref()));
        self.active_task
            .try_lock()
            .map(|guard| guard.contains_key(&session_slot))
            .unwrap_or(false)
    }

    pub(crate) fn emit_runtime_stage(
        &self,
        seed: &RuntimeExecutionSeed,
        stage: RuntimeStage,
        status: TraceStatus,
        detail: Option<String>,
    ) {
        self.runtime_stage_capture.write().push(RuntimeStageSignal {
            stage,
            status: status.clone(),
            at: Utc::now(),
            detail: detail.clone(),
        });
        self.emit(AgentEventData::RuntimeStage {
            stage: stage.label().to_string(),
            status: format!("{status:?}").to_lowercase(),
            run_id: Some(seed.run_id.to_string()),
            task_id: Some(seed.task_id.to_string()),
            thread_id: Some(seed.thread_id.clone()),
            detail,
        });
    }

    pub(crate) fn reset_runtime_hook_state(&self, seed: &RuntimeExecutionSeed) {
        *self.runtime_hook_refs.write() = Some(RuntimeHookRefs {
            run_id: Some(seed.run_id.to_string()),
            task_id: Some(seed.task_id.to_string()),
            thread_id: Some(seed.thread_id.clone()),
            session_id: seed.session_id.clone(),
        });
        *self.runtime_hook_capture.write() = RuntimeHookCapture::default();
        self.runtime_stage_capture.write().clear();
    }

    pub(crate) fn build_runtime_hook_event(&self, timing: HookTiming) -> crate::hooks::HookEvent {
        let mut event = crate::hooks::HookEvent::new(timing);
        if let Some(refs) = self.runtime_hook_refs.read().clone() {
            let mut capture = self.runtime_hook_capture.write();
            capture.trace_injection_count = capture.trace_injection_count.saturating_add(1);

            if let Some(run_id) = refs.run_id {
                event.metadata.insert("run_id".to_string(), run_id);
            }
            if let Some(task_id) = refs.task_id {
                event.metadata.insert("task_id".to_string(), task_id);
            }
            if let Some(thread_id) = refs.thread_id {
                event.metadata.insert("thread_id".to_string(), thread_id);
            }
            if let Some(session_id) = refs.session_id {
                event.metadata.insert("session_id".to_string(), session_id);
            }
        }
        event
    }

    pub(crate) fn collect_dangling_tool_call_ids(messages: &[Message]) -> Vec<String> {
        let mut planned = BTreeSet::new();
        let mut completed = BTreeSet::new();

        for message in messages {
            if let Content::Parts(parts) = &message.content {
                for part in parts {
                    match part {
                        crate::agent::message::ContentPart::ToolCall { id, .. } => {
                            planned.insert(id.clone());
                        }
                        crate::agent::message::ContentPart::ToolResult { tool_call_id, .. } => {
                            completed.insert(tool_call_id.clone());
                        }
                        _ => {}
                    }
                }
            }
        }

        planned
            .into_iter()
            .filter(|id| !completed.contains(id))
            .collect()
    }

    pub(crate) fn attach_runtime_refs(
        &self,
        mut outcome: ChatOutcome,
        seed: &RuntimeExecutionSeed,
        messages: &[Message],
        input_message_count: usize,
    ) -> ChatOutcome {
        outcome.runtime_task = Some(self.build_runtime_task(seed, &outcome, input_message_count));
        outcome.run_trace = Some(self.build_run_trace(seed, &outcome, messages));
        outcome
    }

    pub(crate) async fn handle_agent_message(
        &self,
        message: AgentMessage,
    ) -> Result<Option<AgentMessage>> {
        let response = self.chat_internal_simple(message.content, None).await?;

        Ok(Some(AgentMessage {
            from: self.config.role.clone(),
            to: Some(message.from),
            content: response,
            msg_type: crate::agent::multi_agent::MessageType::Response,
        }))
    }

    /// Resume a previously saved session
    pub async fn resume(&self, session_id: &str) -> Result<String> {
        if let Some(memory) = &self.memory {
            if let Some(session) = memory.retrieve_session(session_id).await? {
                info!("Resuming agent session: {}", session_id);
                self.adopt_recovered_session_state(&session);

                return self
                    .chat(session.messages, Some(session_id.to_string()))
                    .await
                    .map(|o| o.response);
            }
        }
        Err(Error::Internal(format!(
            "Session not found: {}",
            session_id
        )))
    }

    /// Send a prompt and get a response (non-streaming)
    pub async fn chat_simple(
        &self,
        prompt: impl Into<String>,
        session_id: Option<String>,
    ) -> Result<String> {
        let prompt_str = prompt.into();
        self.emit(AgentEventData::Thinking {
            prompt: prompt_str.clone(),
        });

        let messages = vec![Message::user(prompt_str)];

        let outcome = self.chat(messages, session_id).await?;
        Ok(outcome.response)
    }

    async fn chat_internal_simple(
        &self,
        prompt: impl Into<String>,
        session_id: Option<String>,
    ) -> Result<String> {
        let prompt_str = prompt.into();
        self.emit(AgentEventData::Thinking {
            prompt: prompt_str.clone(),
        });

        let messages = vec![Message::user(prompt_str)];
        let outcome = self
            .chat_internal(
                messages,
                session_id,
                tokio_util::sync::CancellationToken::new(),
            )
            .await?;
        Ok(outcome.response)
    }

    /// Send messages and get a response (non-streaming)
    #[instrument(skip(self, messages), fields(model = %self.config.model, message_count = messages.len()))]
    pub async fn chat(
        &self,
        messages: Vec<Message>,
        session_id: Option<String>,
    ) -> Result<ChatOutcome> {
        self.run_foreground_chat(messages, session_id).await
    }

    async fn chat_internal(
        &self,
        messages: Vec<Message>,
        session_id: Option<String>,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> Result<ChatOutcome> {
        self.chat_with_cancel(messages, session_id, cancel_token)
            .await
    }

    /// Finalize the agent's response and cache it
    pub(crate) async fn finalize_outcome(
        &self,
        messages: &[Message],
        mut full_text: String,
        usage: Option<TokenUsage>,
        thoughts: Vec<String>,
        tool_trace: Vec<ToolCallData>,
        steps: usize,
    ) -> Result<ChatOutcome> {
        let mut hook_event = self
            .build_runtime_hook_event(HookTiming::BeforeResponse)
            .with_llm_response(full_text.clone());
        if let Some(last_user_input) = messages
            .iter()
            .rev()
            .find(|message| matches!(message.role, Role::User))
            .map(|message| message.text())
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
        {
            hook_event = hook_event.with_user_input(last_user_input);
        }
        self.apply_clarification_before_response_metadata(&mut hook_event.metadata, messages);
        hook_event
            .metadata
            .insert("thought_count".to_string(), thoughts.len().to_string());
        hook_event
            .metadata
            .insert("tool_call_count".to_string(), tool_trace.len().to_string());
        let planned_tool_call_count = messages
            .iter()
            .filter_map(|message| match &message.content {
                Content::Parts(parts) => Some(
                    parts
                        .iter()
                        .filter(|part| {
                            matches!(part, crate::agent::message::ContentPart::ToolCall { .. })
                        })
                        .count(),
                ),
                _ => None,
            })
            .sum::<usize>();
        let tool_result_count = messages
            .iter()
            .filter_map(|message| match &message.content {
                Content::Parts(parts) => Some(
                    parts
                        .iter()
                        .filter(|part| {
                            matches!(part, crate::agent::message::ContentPart::ToolResult { .. })
                        })
                        .count(),
                ),
                _ => None,
            })
            .sum::<usize>();
        let dangling_tool_call_ids = Self::collect_dangling_tool_call_ids(messages);
        hook_event.metadata.insert(
            "planned_tool_call_count".to_string(),
            planned_tool_call_count.to_string(),
        );
        hook_event.metadata.insert(
            "tool_result_count".to_string(),
            tool_result_count.to_string(),
        );
        hook_event.metadata.insert(
            "dangling_tool_call_count".to_string(),
            dangling_tool_call_ids.len().to_string(),
        );
        if !dangling_tool_call_ids.is_empty() {
            hook_event.metadata.insert(
                "dangling_tool_call_ids".to_string(),
                dangling_tool_call_ids.join(","),
            );
        }
        hook_event.metadata.insert(
            "response_chars".to_string(),
            full_text.chars().count().to_string(),
        );
        let direct_ownership =
            TaskOwnership::direct(self.config.role.clone(), self.current_runtime_session_id());
        hook_event.metadata.insert(
            "visible_owner".to_string(),
            direct_ownership.visible_owner.name().to_string(),
        );
        hook_event.metadata.insert(
            "memory_owner".to_string(),
            direct_ownership.memory_owner.name().to_string(),
        );
        hook_event.metadata.insert(
            "approval_owner".to_string(),
            direct_ownership.approval_owner.name().to_string(),
        );
        hook_event
            .metadata
            .insert("delegation_present".to_string(), "false".to_string());
        hook_event
            .metadata
            .insert("handover_present".to_string(), "false".to_string());
        hook_event.metadata.insert(
            "max_parallel_tools".to_string(),
            self.config.max_parallel_tools.to_string(),
        );
        self.apply_engram_windows_native_runtime_metadata(&mut hook_event.metadata)
            .await;
        if let Some((session_title, title_source)) =
            runtime_session_title(self.config.extra_params.as_ref())
        {
            hook_event
                .metadata
                .insert("session_title".to_string(), session_title);
            hook_event
                .metadata
                .insert("session_title_source".to_string(), title_source.to_string());
            hook_event
                .metadata
                .insert("session_title_present".to_string(), "true".to_string());
        } else {
            hook_event
                .metadata
                .insert("session_title_source".to_string(), "missing".to_string());
            hook_event
                .metadata
                .insert("session_title_present".to_string(), "false".to_string());
        }
        hook_event.metadata.insert(
            "post_run_summary".to_string(),
            format!(
                "thoughts={},tool_calls={}",
                thoughts.len(),
                tool_trace.len()
            ),
        );

        match self.hook_engine.fire(&hook_event).await {
            HookResult::Continue | HookResult::Skip => {}
            HookResult::Modify(modified) => {
                full_text = modified;
            }
            HookResult::Abort(reason) => {
                return Err(Error::AgentExecution(format!(
                    "Before-response hook aborted runtime: {reason}"
                )));
            }
        }
        full_text = Self::compact_assistant_response_for_chat_history(&full_text);

        let mut persisted_messages =
            Vec::with_capacity(messages.len() + usize::from(!full_text.trim().is_empty()));
        let mut persistence_messages = messages.to_vec();
        for message in &mut persistence_messages {
            if message
                .metadata
                .get("session_media_continuation")
                .is_some_and(|value| value == "true")
            {
                message.content = match &message.content {
                    Content::Parts(parts) => Content::Parts(
                        parts
                            .iter()
                            .filter(|part| {
                                !matches!(
                                    part,
                                    ContentPart::Image { .. }
                                        | ContentPart::Audio { .. }
                                        | ContentPart::Video { .. }
                                )
                            })
                            .cloned()
                            .collect(),
                    ),
                    other => other.clone(),
                };
                message.metadata.remove("session_media_continuation");
                message.metadata.remove("session_media_continuation_source");
            }
        }
        persisted_messages.extend_from_slice(&persistence_messages);
        if !full_text.trim().is_empty() {
            let mut assistant_message = Message::assistant(full_text.clone());
            let hook_capture = self.runtime_hook_capture.read().clone();
            Self::attach_provider_media_metadata_from_runtime_capture(
                &mut assistant_message,
                &hook_capture,
            );
            persisted_messages.push(assistant_message);
        }
        let background_notice = self
            .maybe_refresh_background_after_turn(&persisted_messages, &tool_trace)
            .await?;
        if let Some(notice) = background_notice {
            if full_text.trim().is_empty() {
                full_text = notice;
            } else if !full_text.contains(&notice) {
                full_text.push_str("\n\n");
                full_text.push_str(&notice);
            }
            if let Some(last_message) = persisted_messages.last_mut() {
                if matches!(last_message.role, Role::Assistant) {
                    last_message.content = Content::text(full_text.clone());
                }
            }
        }

        self.emit(AgentEventData::Response {
            content: full_text.clone(),
            usage: usage.clone(),
        });

        if let Some(cache) = &self.cache {
            if let Err(error) = cache.set(messages, full_text.clone()).await {
                warn!(
                    "Response cache write failed for session {:?}: {}",
                    self.current_runtime_session_id(),
                    error
                );
            }
        }

        if steps >= self.config.status_recap_threshold_steps {
            self.emit(AgentEventData::Thought {
                content: "Task took many steps. Ensuring final status is clear.".to_string(),
            });
        }
        self.checkpoint(&persisted_messages, steps, SessionStatus::Completed)
            .await?;

        let metabolic_stats = self.current_metabolic_pressure();

        Ok(ChatOutcome {
            response: full_text,
            thoughts,
            tool_calls: tool_trace,
            metabolic_stats: Some(metabolic_stats),
            ownership: direct_ownership,
            delegation: None,
            handover: None,
            runtime_task: None,
            run_trace: None,
        })
    }

    fn next_background_revision(
        current: Option<&BackgroundEnvelope>,
        reason: &str,
    ) -> crate::agent::memory::BackgroundRevision {
        let revision = current
            .map(|value| value.revision.revision + 1)
            .unwrap_or(1);
        let previous_revision = current.map(|value| value.revision.revision);

        crate::agent::memory::BackgroundRevision {
            revision,
            previous_revision,
            updated_at: Utc::now(),
            update_reason: Some(reason.to_string()),
        }
    }

    fn build_relationship_fact_candidate(
        &self,
        relationship_layer: &RelationshipBackgroundLayer,
    ) -> Option<Fact> {
        let mut content_parts = Vec::new();

        if let Some(summary) = relationship_layer
            .relationship_summary
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            content_parts.push(format!("Relationship summary: {}", summary.trim()));
        }

        if !relationship_layer.user_preferences.is_empty() {
            content_parts.push(format!(
                "User preferences: {}",
                relationship_layer.user_preferences.join(" | ")
            ));
        }

        if !relationship_layer.long_term_topics.is_empty() {
            content_parts.push(format!(
                "Long-term topics: {}",
                relationship_layer.long_term_topics.join(" | ")
            ));
        }

        if content_parts.is_empty() {
            return None;
        }

        let mut fact = Fact::new(content_parts.join("\n"), "relationship_background");
        fact.importance = self.config.background_relationship_fact_importance;
        fact.confidence = self.config.background_relationship_fact_confidence;
        fact.protection = FactProtection::Protected;
        fact.source = Some(format!(
            "background_compression:{}",
            self.current_runtime_session_id_or_default()
        ));
        Some(fact)
    }

    async fn maybe_promote_background_relationship_fact(
        &self,
        relationship_layer: &RelationshipBackgroundLayer,
    ) -> Result<bool> {
        let Some(memory) = &self.memory else {
            return Ok(false);
        };
        let Some(fact) = self.build_relationship_fact_candidate(relationship_layer) else {
            return Ok(false);
        };
        let fact_id = fact.id.clone();

        if let Some(manager) = self.background_memory_manager() {
            manager
                .promote_background_relationship_fact(
                    &self.current_runtime_session_id_or_default(),
                    None,
                    fact,
                    "background_relationship_promotion",
                )
                .await?;
        } else {
            memory
                .store_fact(&self.current_runtime_session_id_or_default(), None, fact)
                .await?;
        }

        let review_summary = Some(
            "background relationship promotion requires review before it becomes durable truth",
        );

        if let Some(manager) = self.background_memory_manager() {
            manager
                .request_background_relationship_review(
                    &fact_id,
                    review_summary,
                    self.current_runtime_session_id().as_deref(),
                )
                .await?;
        } else {
            memory
                .request_fact_review(
                    &fact_id,
                    crate::agent::memory::FactReviewPayload {
                        review_reason: Some("background_relationship_candidate".to_string()),
                        challenger_summary: review_summary.map(str::to_string),
                        challenger_source: Some(
                            self.current_runtime_session_id()
                                .as_deref()
                                .map(|value| format!("background_compression:{value}"))
                                .unwrap_or_else(|| "background_compression".to_string()),
                        ),
                        review_requested_at: Some(Utc::now()),
                        resolution: None,
                    },
                )
                .await?;
        }
        Ok(true)
    }

    async fn maybe_refresh_background(
        &self,
        messages: &[Message],
        assistant_response: &str,
    ) -> Result<Option<String>> {
        let mut background_messages =
            Vec::with_capacity(messages.len() + usize::from(!assistant_response.trim().is_empty()));
        background_messages.extend_from_slice(messages);
        if !assistant_response.trim().is_empty() {
            background_messages.push(Message::assistant(assistant_response.to_string()));
        }
        self.maybe_refresh_background_from_messages(&background_messages)
            .await
    }

    async fn background_pressure_band_for_messages(
        &self,
        background_messages: &[Message],
        current_background: Option<BackgroundEnvelope>,
    ) -> Result<BackgroundPressureBand> {
        let mut pressure_context_manager = self.context_manager.clone();
        if let Some(background) = current_background {
            pressure_context_manager.set_background_envelope(background);
        } else {
            pressure_context_manager.clear_background_envelope();
        }
        let _ = pressure_context_manager
            .build_context(
                background_messages,
                &crate::agent::attempt::Strategy::Standard,
                self.provider.is_local(),
            )
            .await?;
        Ok(pressure_context_manager
            .latest_context_metrics()
            .as_ref()
            .map(|metrics| metrics.pressure_band)
            .unwrap_or(BackgroundPressureBand::Normal))
    }

    async fn maybe_refresh_background_after_turn(
        &self,
        background_messages: &[Message],
        tool_trace: &[ToolCallData],
    ) -> Result<Option<String>> {
        let current_background = self.background_envelope.read().clone();
        if Self::is_lightweight_realtime_tool_trace(tool_trace) {
            debug!(
                "Background compression skipped after lightweight realtime turn for session {:?}",
                self.current_runtime_session_id()
            );
            self.record_background_decision(BackgroundCompressionDecision::Skip);
            return Ok(None);
        }

        let has_durable_runtime_state = background_messages
            .iter()
            .any(Self::message_has_durable_runtime_state);
        let has_explicit_background_write =
            background_messages.iter().rev().take(4).any(|message| {
                matches!(message.role, Role::User)
                    && Self::user_text_requests_background_write(&message.content.as_text())
            });
        let conversational_messages = background_messages
            .iter()
            .filter(|message| !matches!(message.role, Role::System))
            .count();
        let long_existing_background = conversational_messages >= 12
            && current_background
                .as_ref()
                .is_some_and(|background| !background.is_empty());
        let observed_pressure_band = self
            .context_manager
            .latest_context_metrics()
            .map(|metrics| metrics.pressure_band)
            .unwrap_or(BackgroundPressureBand::Normal);

        if !has_durable_runtime_state
            && !has_explicit_background_write
            && !long_existing_background
            && !matches!(
                observed_pressure_band,
                BackgroundPressureBand::High | BackgroundPressureBand::Critical
            )
        {
            debug!(
                "Background compression skipped after foreground turn for session {:?}: no durable task state, no explicit memory intent, no long existing background, and observed pressure band is {}",
                self.current_runtime_session_id(),
                observed_pressure_band.as_str()
            );
            self.record_background_decision(BackgroundCompressionDecision::Skip);
            return Ok(None);
        }

        let pressure_band = if matches!(
            observed_pressure_band,
            BackgroundPressureBand::High | BackgroundPressureBand::Critical
        ) {
            observed_pressure_band
        } else {
            self.background_pressure_band_for_messages(
                background_messages,
                current_background.clone(),
            )
            .await?
        };

        if !Self::should_attempt_background_refresh_after_turn(
            background_messages,
            tool_trace,
            pressure_band,
            current_background.as_ref(),
        ) {
            debug!(
                "Background compression skipped after foreground turn for session {:?}: no durable task state, no explicit memory intent, and pressure band is {}",
                self.current_runtime_session_id(),
                pressure_band.as_str()
            );
            self.record_background_decision(BackgroundCompressionDecision::Skip);
            return Ok(None);
        }

        self.maybe_refresh_background_from_messages(background_messages)
            .await
    }

    async fn maybe_refresh_background_from_messages(
        &self,
        background_messages: &[Message],
    ) -> Result<Option<String>> {
        let current_background = self.background_envelope.read().clone();
        let mut pressure_context_manager = self.context_manager.clone();
        if let Some(background) = current_background.clone() {
            pressure_context_manager.set_background_envelope(background);
        } else {
            pressure_context_manager.clear_background_envelope();
        }
        let _ = pressure_context_manager
            .build_context(
                background_messages,
                &crate::agent::attempt::Strategy::Standard,
                self.provider.is_local(),
            )
            .await?;
        let context_metrics = pressure_context_manager.latest_context_metrics();
        let pressure_band = context_metrics
            .as_ref()
            .map(|metrics| metrics.pressure_band)
            .unwrap_or(BackgroundPressureBand::Normal);

        let mut verdict = self
            .tactical_orchestrator
            .derive_background_tactics(&background_messages, current_background.as_ref())
            .await?;

        if matches!(pressure_band, BackgroundPressureBand::Critical)
            && matches!(
                verdict.decision,
                BackgroundCompressionDecision::Skip
                    | BackgroundCompressionDecision::RefreshSessionLayer
                    | BackgroundCompressionDecision::RewriteWholeEnvelope
            )
            && current_background
                .as_ref()
                .is_some_and(|background| !background.is_empty())
        {
            verdict.decision = BackgroundCompressionDecision::RewriteWholeEnvelope;
            verdict.reason = format!(
                "{} | escalated_by_prompt_pressure: critical occupancy band requires a full background rewrite",
                verdict.reason
            );
            verdict.quality_signal = BackgroundQualitySignal::Candidate;
        }

        self.record_background_decision(verdict.decision.clone());
        let mut user_notice = None;

        match verdict.decision {
            BackgroundCompressionDecision::Skip => {
                debug!(
                    "Background compression skipped for session {:?}: {}",
                    self.current_runtime_session_id(),
                    verdict.reason
                );
            }
            BackgroundCompressionDecision::RejectCandidate
            | BackgroundCompressionDecision::RefreshSessionLayer
            | BackgroundCompressionDecision::PromoteRelationshipFact
            | BackgroundCompressionDecision::RewriteWholeEnvelope => {
                if matches!(
                    verdict.decision,
                    BackgroundCompressionDecision::RejectCandidate
                ) {
                    warn!(
                        "Background compression rejected for session {:?}: {}",
                        self.current_runtime_session_id(),
                        verdict.reason
                    );
                }
                let mut envelope = current_background.unwrap_or_default();
                let reason = verdict.reason.clone();
                let revision = Self::next_background_revision(Some(&envelope), &reason);
                let session_candidate = if matches!(
                    verdict.decision,
                    BackgroundCompressionDecision::RejectCandidate
                ) {
                    Self::merge_rejected_session_layer(
                        envelope.session_layer.as_ref(),
                        verdict.session_candidate.clone(),
                    )
                } else {
                    verdict
                        .session_candidate
                        .clone()
                        .or_else(|| envelope.session_layer.clone())
                };
                let candidate_summary = verdict
                    .session_candidate
                    .as_ref()
                    .and_then(|candidate| candidate.summary.clone())
                    .filter(|value| !value.trim().is_empty());

                if matches!(
                    verdict.decision,
                    BackgroundCompressionDecision::PromoteRelationshipFact
                ) {
                    envelope.relationship_layer = verdict.relationship_candidate.clone();
                }
                envelope.session_layer = session_candidate;
                envelope.recent_window_summary =
                    candidate_summary.map(|summary| RecentWindowSummary {
                        summary,
                        pruned_message_count: 0,
                        covered_message_count: background_messages.len(),
                        metadata: std::collections::HashMap::new(),
                    });
                envelope.revision = revision;
                let should_emit_user_notice =
                    Self::should_emit_background_compression_user_notice(pressure_band, &verdict);
                let candidate_user_notice = if should_emit_user_notice {
                    Self::background_compression_user_notice(pressure_band, &verdict)
                } else {
                    None
                };
                envelope.source_refs = verdict.evidence_refs;
                envelope.quality_signal = match verdict.decision {
                    BackgroundCompressionDecision::RewriteWholeEnvelope => {
                        BackgroundQualitySignal::Candidate
                    }
                    BackgroundCompressionDecision::PromoteRelationshipFact => {
                        BackgroundQualitySignal::Guarded
                    }
                    BackgroundCompressionDecision::RejectCandidate => {
                        BackgroundQualitySignal::Rejected
                    }
                    _ => verdict.quality_signal,
                };
                envelope.compression_reason = Some(reason.clone());
                envelope.updated_at = Utc::now();
                envelope.metadata.insert(
                    "background_decision".to_string(),
                    format!("{:?}", verdict.decision).to_lowercase(),
                );
                envelope.metadata.insert(
                    "background_used_slm".to_string(),
                    verdict.used_slm.to_string(),
                );
                envelope.metadata.insert(
                    "background_source_ref_count_pre_cap".to_string(),
                    envelope.source_refs.len().to_string(),
                );
                if let Some(metrics) = context_metrics.as_ref() {
                    envelope.metadata.insert(
                        "background_prompt_occupancy_ratio".to_string(),
                        format!("{:.4}", metrics.prompt_occupancy_ratio),
                    );
                    envelope.metadata.insert(
                        "background_effective_background_tokens".to_string(),
                        metrics.effective_background_tokens.to_string(),
                    );
                    envelope.metadata.insert(
                        "background_estimated_final_prompt_tokens".to_string(),
                        metrics.estimated_final_prompt_tokens.to_string(),
                    );
                }
                envelope.metadata.insert(
                    "background_pressure_band".to_string(),
                    pressure_band.as_str().to_string(),
                );
                let stats = self.background_runtime_stats.read().clone();
                envelope.metadata.insert(
                    "background_total_attempts".to_string(),
                    stats.total_attempts.to_string(),
                );
                envelope.metadata.insert(
                    "background_skip_count".to_string(),
                    stats.skip_count.to_string(),
                );
                envelope.metadata.insert(
                    "background_reject_count".to_string(),
                    stats.reject_count.to_string(),
                );
                envelope.metadata.insert(
                    "background_refresh_session_count".to_string(),
                    stats.refresh_session_count.to_string(),
                );
                envelope.metadata.insert(
                    "background_promote_relationship_count".to_string(),
                    stats.promote_relationship_count.to_string(),
                );
                envelope.metadata.insert(
                    "background_rewrite_count".to_string(),
                    stats.rewrite_count.to_string(),
                );
                ContextManager::pressure_compact_envelope(&mut envelope, pressure_band);
                envelope.apply_budget_caps();
                envelope.metadata.insert(
                    "background_budget_compaction_applied".to_string(),
                    "true".to_string(),
                );
                envelope.metadata.insert(
                    "background_pressure_compaction_applied".to_string(),
                    (!matches!(pressure_band, BackgroundPressureBand::Normal)).to_string(),
                );
                envelope.metadata.insert(
                    "background_source_ref_count".to_string(),
                    envelope.source_refs.len().to_string(),
                );
                if matches!(
                    verdict.decision,
                    BackgroundCompressionDecision::PromoteRelationshipFact
                ) {
                    match verdict.relationship_candidate.as_ref() {
                        Some(relationship_layer) => {
                            match self
                                .maybe_promote_background_relationship_fact(relationship_layer)
                                .await
                            {
                                Ok(true) => {
                                    envelope.metadata.insert(
                                        "durable_promotion_pending".to_string(),
                                        "false".to_string(),
                                    );
                                    envelope.metadata.insert(
                                        "durable_promotion_status".to_string(),
                                        "pending_review".to_string(),
                                    );
                                    envelope.metadata.insert(
                                        "background_review_reason".to_string(),
                                        "background_relationship_candidate".to_string(),
                                    );
                                    envelope.metadata.insert(
                                        "background_review_source".to_string(),
                                        self.current_runtime_session_id()
                                            .as_deref()
                                            .map(|value| format!("background_compression:{value}"))
                                            .unwrap_or_else(|| {
                                                "background_compression".to_string()
                                            }),
                                    );
                                }
                                Ok(false) => {
                                    envelope.metadata.insert(
                                        "durable_promotion_pending".to_string(),
                                        "true".to_string(),
                                    );
                                    envelope.metadata.insert(
                                        "durable_promotion_status".to_string(),
                                        if self.memory.is_some() {
                                            "skipped_no_candidate".to_string()
                                        } else {
                                            "deferred_no_memory".to_string()
                                        },
                                    );
                                }
                                Err(error) => {
                                    warn!(
                                        "Background durable promotion failed for session {:?}: {}",
                                        self.current_runtime_session_id(),
                                        error
                                    );
                                    envelope.metadata.insert(
                                        "durable_promotion_pending".to_string(),
                                        "true".to_string(),
                                    );
                                    envelope.metadata.insert(
                                        "durable_promotion_status".to_string(),
                                        "failed".to_string(),
                                    );
                                    envelope.metadata.insert(
                                        "durable_promotion_error".to_string(),
                                        error.to_string(),
                                    );
                                }
                            }
                        }
                        None => {
                            envelope.metadata.insert(
                                "durable_promotion_pending".to_string(),
                                "true".to_string(),
                            );
                            envelope.metadata.insert(
                                "durable_promotion_status".to_string(),
                                "missing_relationship_candidate".to_string(),
                            );
                        }
                    }
                } else if matches!(
                    verdict.decision,
                    BackgroundCompressionDecision::RejectCandidate
                ) {
                    envelope
                        .metadata
                        .insert("durable_promotion_pending".to_string(), "false".to_string());
                    envelope.metadata.insert(
                        "durable_promotion_status".to_string(),
                        "rejected_candidate".to_string(),
                    );
                }

                let authoritative_envelope = envelope.clone();
                *self.background_envelope.write() = Some(envelope);

                let runtime_session_id = self.current_runtime_session_id();
                if let (Some(manager), Some(session_id)) = (
                    self.background_memory_manager(),
                    runtime_session_id.as_deref(),
                ) {
                    let persisted_status = self
                        .persist_background_envelope_with_retry(
                            manager,
                            session_id,
                            authoritative_envelope.clone(),
                            "background_session_refresh",
                        )
                        .await;

                    let mut background_guard = self.background_envelope.write();
                    if let Some(ref mut persisted_envelope) = *background_guard {
                        match persisted_status {
                            Ok(BackgroundSessionPersistenceStatus::Persisted) => {
                                persisted_envelope.metadata.insert(
                                    "background_session_persistence_status".to_string(),
                                    "persisted".to_string(),
                                );
                                if let Some(notice) = candidate_user_notice.clone() {
                                    user_notice = Some(notice);
                                }
                            }
                            Ok(BackgroundSessionPersistenceStatus::DeferredMissingSession) => {
                                persisted_envelope.metadata.insert(
                                    "background_session_persistence_status".to_string(),
                                    "deferred_missing_session".to_string(),
                                );
                            }
                            Err(error) => {
                                warn!(
                                    "Background session persistence failed for session {:?}: {}",
                                    self.current_runtime_session_id(),
                                    error
                                );
                                persisted_envelope.metadata.insert(
                                    "background_session_persistence_status".to_string(),
                                    "failed".to_string(),
                                );
                                persisted_envelope.metadata.insert(
                                    "background_session_persistence_error".to_string(),
                                    error.to_string(),
                                );
                            }
                        }
                    }
                } else if let Some(notice) = candidate_user_notice {
                    user_notice = Some(notice);
                }
                debug!(
                    "Background compression updated session {:?} with decision {:?}",
                    self.current_runtime_session_id(),
                    verdict.decision
                );
            }
        }

        Ok(user_notice)
    }

    fn should_emit_background_compression_user_notice(
        pressure_band: BackgroundPressureBand,
        verdict: &crate::agent::tactical::BackgroundCompressionVerdict,
    ) -> bool {
        if matches!(pressure_band, BackgroundPressureBand::Normal) {
            return false;
        }

        matches!(
            verdict.decision,
            BackgroundCompressionDecision::RefreshSessionLayer
                | BackgroundCompressionDecision::RewriteWholeEnvelope
        )
    }

    fn background_compression_user_notice(
        pressure_band: BackgroundPressureBand,
        verdict: &crate::agent::tactical::BackgroundCompressionVerdict,
    ) -> Option<String> {
        if matches!(pressure_band, BackgroundPressureBand::Normal) {
            return None;
        }

        if matches!(verdict.decision, BackgroundCompressionDecision::Skip) {
            return None;
        }

        let notice = match pressure_band {
            BackgroundPressureBand::High => "注：为保持长对话稳定，我已自动压缩较早背景内容。",
            BackgroundPressureBand::Critical => {
                "注：当前上下文接近上限，我已自动重写并压缩较早背景内容，以保持对话连续稳定。"
            }
            BackgroundPressureBand::Normal => return None,
        };

        Some(notice.to_string())
    }

    /// Preparatory steps before entering the reasoning loop
    pub(crate) async fn prepare_for_step(
        &self,
        messages: &mut Vec<Message>,
        steps: usize,
    ) -> Result<Option<ChatOutcome>> {
        if let Some(last) = messages.last() {
            if last.role == Role::User {
                self.emit(AgentEventData::Thinking {
                    prompt: last.content.as_text(),
                });
            }
        }

        self.checkpoint(messages, steps, SessionStatus::Thinking)
            .await?;

        if let Some(cache) = &self.cache {
            if let Ok(Some(cached_response)) = cache.get(messages).await {
                info!("Cache hit! Returning cached response.");
                return Ok(Some(ChatOutcome {
                    response: cached_response,
                    thoughts: vec![],
                    tool_calls: vec![],
                    metabolic_stats: None,
                    ownership: TaskOwnership::direct(
                        self.config.role.clone(),
                        self.current_runtime_session_id(),
                    ),
                    delegation: None,
                    handover: None,
                    runtime_task: None,
                    run_trace: None,
                }));
            }
        }
        Ok(None)
    }

    /// Phase 8.2 & 9.1: Pre-flight Analysis & Planning (Metabolic Aware)
    pub(crate) async fn handle_pre_flight(
        &self,
        messages: &mut Vec<Message>,
    ) -> Result<(usize, f32, Option<String>, Option<AgentRole>)> {
        let mut max_steps = self.config.default_max_steps;
        let mut risk_score = 0.0f32;
        let mut model_override: Option<String> = None;
        let handover: Option<AgentRole> = None;

        let metabolic = self.current_metabolic_pressure();
        let last_user_msg = messages
            .iter()
            .rev()
            .find(|m| m.role == Role::User)
            .map(|m| m.text())
            .unwrap_or_default();
        let direct_route = classify_query_capability_route(&last_user_msg);
        let has_media_input = Self::latest_user_message_has_media(messages);
        let pre_flight_level =
            Self::classify_extended_pre_flight(&last_user_msg, direct_route, has_media_input);

        if self.config.enable_meta_cognition
            && extended_pre_flight_runs_complexity_estimator(pre_flight_level)
        {
            if let Some(estimator) = &self.complexity_estimator {
                let tool_defs = self.tools.definitions().await;

                if let Ok(complexity) = estimator.estimate(&last_user_msg, &tool_defs).await {
                    risk_score = complexity.risk_score;
                    self.governance.set_risk_score(risk_score);

                    let limit_multiplier = if metabolic.is_throttled { 0.7 } else { 1.0 };

                    if let Some(over) = complexity.max_steps_override {
                        max_steps = (over as f32 * limit_multiplier) as usize;
                    } else {
                        max_steps =
                            ((complexity.estimated_steps + 5) as f32 * limit_multiplier) as usize;
                    }

                    info!(
                        "Pre-flight analysis for {}: Steps={}, Risk={:.2}, Throttled={}, Rationale: {}",
                        self.config.name, max_steps, risk_score, metabolic.is_throttled, complexity.rationale
                    );

                    if self.config.enable_jit_distillation
                        && extended_pre_flight_runs_jit_distillation(pre_flight_level)
                    {
                        let prev_intent = self.current_intent.read().clone();
                        if let Some(prev) = prev_intent {
                            if prev != complexity.intent {
                                if let Err(error) = self
                                    .process_jit_distillation(&prev, &complexity.intent, messages)
                                    .await
                                {
                                    warn!(
                                        "JIT distillation failed during intent transition {} -> {}: {}",
                                        prev, complexity.intent, error
                                    );
                                }
                            }
                        }
                    }
                    *self.current_intent.write() = Some(complexity.intent.clone());

                    if complexity.estimated_steps <= 2
                        && risk_score < 0.2
                        && extended_pre_flight_allows_auto_stepdown(pre_flight_level)
                    {
                        if let Some(selected_model) = self.select_auto_stepdown_model() {
                            debug!(
                                "Auto-Stepdown: Using configured low-complexity model {}",
                                selected_model
                            );
                            model_override = Some(selected_model);
                        }
                    }
                }
            }
        } else {
            self.governance.set_risk_score(0.0);
        }

        max_steps = max_steps.clamp(1, 100);

        Ok((max_steps, risk_score, model_override, handover))
    }

    pub(crate) async fn run_foreground_chat(
        &self,
        messages: Vec<Message>,
        session_id: Option<String>,
    ) -> Result<ChatOutcome> {
        let task_id = Uuid::new_v4().to_string();
        let cancel_token = CancellationToken::new();
        let pause_controller = PauseController::default();
        let task_session_id = session_id;
        let session_slot = Self::foreground_task_slot_key(
            task_session_id.as_deref().or(self.session_id.as_deref()),
        );

        let previous_task = {
            let mut active_task = self.active_task.lock().await;
            active_task.remove(&session_slot)
        };
        let is_preemptive = previous_task.is_some();

        if let Some(previous_task) = previous_task {
            previous_task.cancel_token.cancel();
            tokio::spawn(async move {
                if let Err(error) = previous_task.join_handle.await {
                    warn!("Preempted foreground task cleanup failed: {}", error);
                }
            });
        }

        let agent = self.clone();
        let task_messages = messages;
        let task_cancel = cancel_token.clone();
        let task_pause = pause_controller.clone();
        let task_id_for_cleanup = task_id.clone();
        let session_slot_for_cleanup = session_slot.clone();
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();

        let join_handle = tokio::spawn(async move {
            let result = if is_preemptive {
                agent
                    .process_preemptive_messages(
                        task_messages,
                        task_session_id,
                        task_cancel.clone(),
                        task_pause.clone(),
                    )
                    .await
            } else {
                agent
                    .chat_with_cancel_and_pause(
                        task_messages,
                        task_session_id,
                        task_cancel.clone(),
                        task_pause.clone(),
                    )
                    .await
            };
            if result_tx.send(result).is_err() {
                if task_cancel.is_cancelled() || is_preemptive {
                    debug!(
                        "Foreground caller dropped before completion after cancellation/preemption"
                    );
                } else {
                    warn!(
                        "Foreground caller dropped before completion; request likely timed out or the client disconnected"
                    );
                }
            }

            let mut active_task = agent.active_task.lock().await;
            if active_task
                .get(&session_slot_for_cleanup)
                .map(|handle| handle.task_id.as_str())
                == Some(task_id_for_cleanup.as_str())
            {
                active_task.remove(&session_slot_for_cleanup);
            }
        });

        {
            let mut active_task = self.active_task.lock().await;
            active_task.insert(
                session_slot,
                crate::agent::runtime_support::TaskHandle {
                    task_id,
                    cancel_token,
                    pause_controller,
                    join_handle,
                },
            );
        }

        result_rx.await.map_err(|_| {
            Error::Internal("Foreground reasoning task terminated unexpectedly".to_string())
        })?
    }

    async fn process_preemptive_messages(
        &self,
        mut new_messages: Vec<Message>,
        session_id: Option<String>,
        cancel_token: CancellationToken,
        pause_controller: PauseController,
    ) -> Result<ChatOutcome> {
        let mut messages = Vec::new();
        let effective_session_id = session_id.clone().or_else(|| self.session_id.clone());

        if let (Some(memory), Some(session_id)) = (&self.memory, effective_session_id.as_ref()) {
            if let Ok(Some(session)) = memory.retrieve_session(session_id).await {
                self.adopt_recovered_session_state(&session);
                messages = session.messages;
            } else {
                *self.background_envelope.write() = None;
            }
        }

        if !messages.is_empty() {
            messages.push(Message::system(format!(
                "{}\n\nUser interrupted with a new priority. The previous reasoning path was aborted mid-stream. \
                 Analyze the new input alongside the existing context and re-plan.",
                MARKER_INTERJECTION
            )));
        }

        if messages.is_empty() {
            messages = new_messages;
        } else {
            messages.append(&mut new_messages);
        }

        self.chat_with_cancel_and_pause(
            messages,
            effective_session_id,
            cancel_token,
            pause_controller,
        )
        .await
    }

    pub async fn chat_with_cancel(
        &self,
        messages: Vec<Message>,
        session_id: Option<String>,
        cancel_token: CancellationToken,
    ) -> Result<ChatOutcome> {
        self.chat_with_cancel_and_pause(
            messages,
            session_id,
            cancel_token,
            PauseController::default(),
        )
        .await
    }

    async fn chat_with_cancel_and_pause(
        &self,
        mut messages: Vec<Message>,
        session_id: Option<String>,
        cancel_token: CancellationToken,
        pause_controller: PauseController,
    ) -> Result<ChatOutcome> {
        let effective_session_id = session_id.clone().or_else(|| self.session_id.clone());
        if Self::should_hydrate_existing_session(&messages) {
            if let (Some(memory), Some(session_id)) = (&self.memory, effective_session_id.as_ref())
            {
                if let Ok(Some(session)) = memory.retrieve_session(session_id).await {
                    self.adopt_recovered_session_state(&session);
                    if !session.messages.is_empty() {
                        let mut merged = session.messages;
                        merged.append(&mut messages);
                        messages = merged;
                    }
                } else {
                    *self.background_envelope.write() = None;
                }
            }
        }
        Self::maybe_attach_recent_media_to_followup(&mut messages);
        let task_id = Uuid::new_v4();
        let execution_seed = RuntimeExecutionSeed {
            task_id,
            run_id: Uuid::new_v4(),
            started_at: Utc::now(),
            session_id: effective_session_id.clone(),
            thread_id: Self::derive_runtime_thread_id(effective_session_id.as_deref(), task_id),
        };
        self.reset_runtime_hook_state(&execution_seed);
        let input_message_count = messages.len();

        if let Some(sid) = &effective_session_id {
            messages.insert(0, Message::system(format!("Current Session ID: {}", sid)));
        }

        for msg in messages.iter_mut() {
            if msg.role == Role::User {
                let content_str = msg.content.as_text();
                let sanitized = self.security.check_input(&content_str);
                if sanitized.was_modified {
                    tracing::warn!(
                        "Injection attempt detected and sanitized: {:?}",
                        sanitized.warnings
                    );
                    msg.content = Content::Text(sanitized.content);
                }
            }
        }

        self.emit_runtime_stage(
            &execution_seed,
            RuntimeStage::Ingress,
            TraceStatus::Succeeded,
            Some("input accepted and sanitized".to_string()),
        );

        if let Some(outcome) = self
            .maybe_handle_capability_self_description(
                &messages,
                &execution_seed,
                input_message_count,
            )
            .await?
        {
            self.emit_runtime_stage(
                &execution_seed,
                RuntimeStage::ContextBuild,
                TraceStatus::Succeeded,
                Some("runtime capability summary generated".to_string()),
            );
            self.emit_runtime_stage(
                &execution_seed,
                RuntimeStage::Egress,
                TraceStatus::Succeeded,
                Some("response returned".to_string()),
            );
            return Ok(outcome);
        }

        if let Some(outcome) = self
            .maybe_handle_direct_memory_crud(&messages, &execution_seed, input_message_count)
            .await?
        {
            self.emit_runtime_stage(
                &execution_seed,
                RuntimeStage::PersistenceMemory,
                TraceStatus::Succeeded,
                Some("direct memory CRUD completed".to_string()),
            );
            self.emit_runtime_stage(
                &execution_seed,
                RuntimeStage::Egress,
                TraceStatus::Succeeded,
                Some("response returned".to_string()),
            );
            return Ok(outcome);
        }

        let (max_steps, risk_score, model_override, handover) =
            self.handle_pre_flight(&mut messages).await?;

        self.emit_runtime_stage(
            &execution_seed,
            RuntimeStage::Governance,
            TraceStatus::Succeeded,
            Some("pre-flight checks completed".to_string()),
        );
        self.emit_runtime_stage(
            &execution_seed,
            RuntimeStage::ContextBuild,
            TraceStatus::Succeeded,
            Some("context ready for reasoning".to_string()),
        );

        if let Some(target) = handover {
            return Ok(self.attach_runtime_refs(
                ChatOutcome {
                    response: format!(
                        "### HANDOVER: Recommending switch to specialized agent: {}",
                        target.name()
                    ),
                    thoughts: vec![format!(
                        "{} is better equipped to handle this query.",
                        target.name()
                    )],
                    tool_calls: vec![],
                    metabolic_stats: Some(self.current_metabolic_pressure()),
                    ownership: TaskOwnership::direct(
                        self.config.role.clone(),
                        effective_session_id.clone(),
                    ),
                    delegation: None,
                    handover: Some(target),
                    runtime_task: None,
                    run_trace: None,
                },
                &execution_seed,
                &messages,
                input_message_count,
            ));
        }

        let attempt = crate::agent::attempt::Attempt::new();

        self.perform_recovery_loop_with_cancel(
            messages,
            attempt,
            max_steps,
            risk_score,
            model_override,
            cancel_token,
            pause_controller,
            execution_seed,
            input_message_count,
        )
        .await
    }

    async fn perform_recovery_loop_with_cancel(
        &self,
        mut messages: Vec<Message>,
        mut attempt: crate::agent::attempt::Attempt,
        max_steps: usize,
        risk_score: f32,
        model_override: Option<String>,
        cancel_token: CancellationToken,
        pause_controller: PauseController,
        execution_seed: RuntimeExecutionSeed,
        input_message_count: usize,
    ) -> Result<ChatOutcome> {
        let mut last_error: Option<String> = None;

        let mut actual_max_steps = max_steps;
        if let Some(ap) = &self.autopilot {
            let optimized_depth = ap.get_optimized_depth().await;
            if optimized_depth == crate::agent::evolution::autopilot::ReasoningDepth::System1 {
                actual_max_steps = 1;
            }
        }

        loop {
            let error_ref = last_error.as_deref();
            Self::maybe_inject_retry_execution_guard(&mut messages, error_ref);
            let strategy = self.resolve_reasoning_strategy(&attempt, &messages);

            self.emit_runtime_stage(
                &execution_seed,
                RuntimeStage::Reasoning,
                TraceStatus::Started,
                Some(format!("strategy={strategy:?}").to_lowercase()),
            );

            let bridge = PreemptiveBridge {
                inner: self,
                task_cancel: cancel_token.clone(),
                task_pause: pause_controller.clone(),
            };

            let res = self
                .reasoner_for_session(execution_seed.session_id.clone())
                .execute_loop(
                    &bridge,
                    self,
                    &mut messages,
                    &attempt,
                    &strategy,
                    actual_max_steps,
                    error_ref,
                    risk_score,
                    model_override.clone(),
                )
                .await;

            match res {
                Ok(outcome) => {
                    let resumed_inputs = pause_controller.wait_if_paused(&cancel_token).await?;
                    if !resumed_inputs.is_empty() {
                        let joined = resumed_inputs.join("\n");
                        messages.push(Message::assistant(format!(
                            "Intermediate result produced before the pause was resumed:\n\n{}",
                            Self::compact_assistant_response_for_chat_history(&outcome.response)
                        )));
                        messages.push(Message::user(format!(
                            "User resumed the paused task with additional instructions. Continue the same task from the intermediate result above, applying these instructions without restarting from scratch.\n\n{joined}"
                        )));
                        self.emit(AgentEventData::Thought {
                            content:
                                "Paused task resumed with follow-up instructions before finalization"
                                    .to_string(),
                        });
                        continue;
                    }
                    self.emit_runtime_stage(
                        &execution_seed,
                        RuntimeStage::Reasoning,
                        TraceStatus::Succeeded,
                        Some(format!("thoughts={}", outcome.thoughts.len())),
                    );
                    self.emit_runtime_stage(
                        &execution_seed,
                        RuntimeStage::ToolPlanningFiltering,
                        TraceStatus::Succeeded,
                        Some(format!("planned_tool_calls={}", outcome.tool_calls.len())),
                    );
                    self.emit_runtime_stage(
                        &execution_seed,
                        RuntimeStage::Execution,
                        TraceStatus::Succeeded,
                        Some(if outcome.tool_calls.is_empty() {
                            "no tool calls executed".to_string()
                        } else {
                            format!("executed_tool_calls={}", outcome.tool_calls.len())
                        }),
                    );
                    if let Some(checker) = &self.fact_checker {
                        let context = messages
                            .iter()
                            .map(|m| format!("{}: {}", m.role.as_str(), m.content.as_text()))
                            .collect::<Vec<_>>()
                            .join("\n");

                        let validation = checker.verify(&outcome.response, &context).await;
                        if !validation.is_valid && validation.confidence > 0.7 {
                            warn!(
                                "🛡️ [FactCheck] Hallucination detected! Confidence: {}. Re-routing for correction.",
                                validation.confidence
                            );
                            let error_msg = format!(
                                "CRITICAL: The previous response contained factual inconsistencies: {}. Please verify your sources and provide a corrected response.",
                                validation.detected_hallucinations.join(", ")
                            );

                            self.emit(AgentEventData::Error {
                                message: error_msg.clone(),
                            });

                            if attempt.can_retry() {
                                last_error = Some(error_msg);
                                attempt.next();
                                continue;
                            }
                        }
                    }
                    self.emit_runtime_stage(
                        &execution_seed,
                        RuntimeStage::PersistenceMemory,
                        TraceStatus::Succeeded,
                        Some("runtime refs prepared".to_string()),
                    );
                    self.emit_runtime_stage(
                        &execution_seed,
                        RuntimeStage::TraceAudit,
                        TraceStatus::Succeeded,
                        Some("trace envelope prepared".to_string()),
                    );
                    self.emit_runtime_stage(
                        &execution_seed,
                        RuntimeStage::Egress,
                        TraceStatus::Succeeded,
                        Some("response returned".to_string()),
                    );
                    return Ok(self.attach_runtime_refs(
                        outcome,
                        &execution_seed,
                        &messages,
                        input_message_count,
                    ));
                }
                Err(e) => {
                    if cancel_token.is_cancelled() {
                        self.emit_runtime_stage(
                            &execution_seed,
                            RuntimeStage::Reasoning,
                            TraceStatus::Cancelled,
                            Some("task preempted by new input".to_string()),
                        );
                        return Err(Error::AgentExecution("Task preempted by new input".into()));
                    }

                    if e.is_retryable() && attempt.can_retry() {
                        warn!(
                            "Foreground recovery retry triggered for session {:?}: attempt={} error={}",
                            execution_seed.session_id,
                            attempt.retry_count + 1,
                            e
                        );
                        attempt.next();
                        tokio::time::sleep(attempt.backoff_duration()).await;
                        last_error = Some(e.to_string());
                        continue;
                    }
                    self.emit_runtime_stage(
                        &execution_seed,
                        RuntimeStage::Reasoning,
                        TraceStatus::Failed,
                        Some(e.to_string()),
                    );
                    return Err(e);
                }
            }
        }
    }

    pub async fn checkpoint(
        &self,
        messages: &[Message],
        step: usize,
        status: SessionStatus,
    ) -> Result<()> {
        if let (Some(memory), Some(session_id)) = (&self.memory, self.current_runtime_session_id())
        {
            let mut messages = messages
                .iter()
                .filter(|message| !Self::is_transient_runtime_system_message(message))
                .cloned()
                .collect::<Vec<_>>();

            let is_observing = self
                .evolution_manager
                .as_ref()
                .map(|em| em.observation_window().read().is_active())
                .unwrap_or(false);

            if is_observing {
                for msg in &mut messages {
                    msg.unverified = true;
                }
            }

            let role_name = self.agent_identity.read().as_ref().map(|i| i.role.clone());
            let executed_tools = self.seen_tools.read().iter().cloned().collect();

            let session = crate::agent::session::AgentSession {
                id: session_id.clone(),
                messages,
                step,
                status,
                updated_at: chrono::Utc::now(),
                is_distilled: false,
                hardened_skills: Vec::new(),
                agent_role: role_name,
                max_steps: self.config.default_max_steps,
                executed_tools,
                lifecycle: crate::agent::session::SessionLifecycle::default(),
                background_envelope: self.background_envelope.read().clone(),
            };
            memory.store_session(session).await?;
            debug!("Agent checkpoint saved for session: {}", session_id);
        }
        Ok(())
    }

    /// JIT Micro-distillation for topic shifts.
    pub(crate) async fn process_jit_distillation(
        &self,
        previous_intent: &str,
        new_intent: &str,
        _current_messages: &[Message],
    ) -> Result<()> {
        if let Some(memory) = &self.memory {
            let limit = 20;
            let session_id = self.current_runtime_session_id_or_default();
            let history = memory.retrieve(&session_id, None, limit).await;

            if history.len() < 3 {
                return Ok(());
            }

            if let Some(budget) = self.config.jit_token_budget {
                if let Some(usage) = self.provider.get_session_usage().await {
                    if usage.total_tokens > budget {
                        warn!(
                            "JIT Budget Exceeded ({} > {}): Bypassing micro-distillation for session {}.",
                            usage.total_tokens,
                            budget,
                            session_id
                        );
                        return Ok(());
                    }
                }
            }

            memory.emit_event(
                benshu_infra::traits::memory::MemoryEvent::JitTriggered {
                    previous_intent: previous_intent.to_string(),
                    new_intent: new_intent.to_string(),
                },
                benshu_infra::traits::memory::EventLevel::Info,
            );
            info!(
                "Agent: High topic drift detected (Intent: {}). Triggering JIT Micro-distillation.",
                previous_intent
            );

            let distiller = crate::agent::evolution::distillation::MemoryDistiller::new(
                memory.clone(),
                self.provider.clone(),
                self.config
                    .jit_distillation_model
                    .clone()
                    .unwrap_or_else(|| self.config.model.clone()),
            );

            let summary = distiller.jit_summarize(&history, Some(&session_id)).await?;

            info!(
                "JIT Distillation: Topic shift detected from '{}'. Storing episode: {}",
                previous_intent, summary
            );
            let mut fact = crate::agent::memory::Fact::new(
                format!("Topic Segment ({}) Summary: {}", previous_intent, summary),
                "episode".to_string(),
            );
            fact.source = Some(format!("jit:{}", previous_intent));
            if !self
                .should_store_jit_fact(memory, &session_id, &fact)
                .await?
            {
                debug!(
                    "Skipping JIT fact write for session {} due to dedupe/cooldown guard",
                    session_id
                );
                return Ok(());
            }
            if let Err(error) = memory.store_fact(&session_id, None, fact).await {
                warn!(
                    "JIT fact persistence failed for session {}: {}",
                    session_id, error
                );
            }
        }
        Ok(())
    }

    /// Ensure we have an active foreground token for a new task.
    pub fn ensure_active_token(&self) {
        let is_cancelled = self.current_task_token.read().is_cancelled();
        if is_cancelled {
            *self.current_task_token.write() = tokio_util::sync::CancellationToken::new();
        }
    }
}

#[async_trait::async_trait]
impl<P: Provider> MultiAgent for Agent<P> {
    fn role(&self) -> AgentRole {
        self.config.role.clone()
    }

    fn signal_shutdown(&self) {
        self.lifecycle_token.read().cancel();
        self.current_task_token.read().cancel();
        if let Some(em) = &self.evolution_manager {
            em.signal_shutdown();
        }
        self.task_runner.abort_all();
    }

    async fn handle_message(&self, message: AgentMessage) -> Result<Option<AgentMessage>> {
        self.handle_agent_message(message).await
    }

    async fn process(&self, input: &str) -> Result<String> {
        self.chat_internal_simple(input, None).await
    }

    async fn analyze_complexity(&self, prompt: &str) -> Result<String> {
        let mut extra = self
            .config
            .extra_params
            .clone()
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
        if !extra.is_object() {
            extra = serde_json::Value::Object(serde_json::Map::new());
        }
        if let serde_json::Value::Object(ref mut map) = extra {
            map.insert(
                "response_format".to_string(),
                serde_json::json!({ "type": "json_object" }),
            );
            map.insert("complexity_analysis".to_string(), serde_json::json!(true));
            map.insert("inference_priority".to_string(), serde_json::json!(-8));
            map.insert(
                "inference_runtime_owner".to_string(),
                serde_json::json!("complexity"),
            );
            map.insert(
                "brain_runtime_owner".to_string(),
                serde_json::json!("complexity"),
            );
        }

        let request = crate::agent::provider::ChatRequest {
            model: self.config.model.clone(),
            system_prompt: Some(
                "You are a complexity-analysis engine for BenShu. \
                 Return ONLY valid JSON. Do not call tools. Do not answer the task itself. \
                 Do not route to image generation or any other capability. \
                 Emit exactly one JSON object matching the requested schema."
                    .to_string(),
            ),
            messages: vec![crate::agent::message::Message::user(prompt)],
            temperature: Some(0.0),
            max_tokens: Some(256),
            session_id: Some(format!(
                "{}::complexity-l2",
                self.session_id
                    .clone()
                    .unwrap_or_else(|| self.config.role.name().to_lowercase())
            )),
            extra_params: Some(extra),
            ..Default::default()
        };

        self.provider
            .stream_completion(request)
            .await
            .map_err(Error::from)?
            .collect_text()
            .await
            .map_err(Error::from)
    }

    async fn generate_text_only(&self, prompt: &str) -> Result<String> {
        self.generate_text_only_with_max_tokens(prompt, None).await
    }

    async fn generate_text_only_with_max_tokens(
        &self,
        prompt: &str,
        max_tokens: Option<u64>,
    ) -> Result<String> {
        self.generate_text_only_with_progress(prompt, max_tokens, None)
            .await
    }

    async fn generate_text_only_with_progress(
        &self,
        prompt: &str,
        max_tokens: Option<u64>,
        progress: Option<TextGenerationProgressSink>,
    ) -> Result<String> {
        self.generate_text_only_with_limits(
            prompt,
            crate::agent::multi_agent::TextGenerationLimits {
                max_tokens,
                target_chars: None,
                hard_max_chars: None,
            },
            progress,
        )
        .await
    }

    async fn generate_text_only_with_limits(
        &self,
        prompt: &str,
        limits: crate::agent::multi_agent::TextGenerationLimits,
        progress: Option<TextGenerationProgressSink>,
    ) -> Result<String> {
        if let Some(progress) = progress.as_ref() {
            progress(TextGenerationProgress {
                stage: TextGenerationProgressStage::Started,
                generated_chars: 0,
                preview: None,
                snapshot: None,
            });
        }
        let mut extra = self
            .config
            .extra_params
            .clone()
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
        if !extra.is_object() {
            extra = serde_json::Value::Object(serde_json::Map::new());
        }
        if let serde_json::Value::Object(ref mut map) = extra {
            map.remove("response_format");
            map.insert("tool_choice".to_string(), serde_json::json!("none"));
            map.insert(
                "inference_runtime_owner".to_string(),
                serde_json::json!("continuous_text_step"),
            );
            map.insert(
                "brain_runtime_owner".to_string(),
                serde_json::json!("continuous_text_step"),
            );
            if let Some(max_tokens) = limits.max_tokens {
                map.insert(
                    "continuous_text_step_requested_max_tokens".to_string(),
                    serde_json::json!(max_tokens),
                );
            }
            if let Some(target_chars) = limits.target_chars {
                map.insert(
                    "continuous_text_step_target_chars".to_string(),
                    serde_json::json!(target_chars),
                );
            }
            if let Some(hard_max_chars) = limits.hard_max_chars {
                map.insert(
                    "continuous_text_step_hard_max_chars".to_string(),
                    serde_json::json!(hard_max_chars),
                );
            }
        }
        let configured_ceiling = self
            .config
            .extra_params
            .as_ref()
            .and_then(|value| value.get("continuous_text_step_max_tokens"))
            .and_then(|value| value.as_u64())
            .filter(|value| *value > 0)
            .unwrap_or(12_000)
            .clamp(256, 16_384);
        let step_max_tokens = limits
            .max_tokens
            .map(|requested| requested.min(configured_ceiling))
            .unwrap_or(configured_ceiling)
            .clamp(256, 16_384);
        let hard_max_chars = limits
            .hard_max_chars
            .filter(|value| *value > 0)
            .map(|value| value.max(256));
        let configured_first_chunk_timeout_secs = self
            .config
            .extra_params
            .as_ref()
            .and_then(|value| value.get("continuous_text_step_first_chunk_timeout_secs"))
            .and_then(|value| value.as_u64())
            .filter(|value| *value > 0)
            .unwrap_or(180)
            .clamp(10, 600);
        let first_chunk_timeout_secs = Self::dynamic_continuous_text_first_chunk_timeout_secs(
            prompt,
            configured_first_chunk_timeout_secs,
        );
        let idle_chunk_timeout_secs = self
            .config
            .extra_params
            .as_ref()
            .and_then(|value| value.get("continuous_text_step_idle_chunk_timeout_secs"))
            .and_then(|value| value.as_u64())
            .filter(|value| *value > 0)
            .unwrap_or(45)
            .clamp(15, 180);
        let idle_chunk_timeout_secs =
            Self::dynamic_continuous_text_idle_timeout_secs(prompt, idle_chunk_timeout_secs);

        let mut request = crate::agent::provider::ChatRequest {
            model: self.config.model.clone(),
            system_prompt: Some(
                "You are an isolated worker step writer for BenShu. \
                 Return only the requested artifact text for this one step. \
                 Do not call tools. Do not delegate. Do not include tool tags. \
                 Do not explain that you cannot execute tools. \
                 Before returning, silently proofread the artifact text for readability, \
                 obvious typos, near-homophone mistakes, malformed phrases, and repetition."
                    .to_string(),
            ),
            messages: vec![crate::agent::message::Message::user(prompt)],
            temperature: self.config.temperature,
            max_tokens: Some(step_max_tokens),
            session_id: Some(format!(
                "{}::continuous-text-step::{}",
                self.session_id
                    .clone()
                    .unwrap_or_else(|| self.config.role.name().to_lowercase()),
                uuid::Uuid::new_v4()
            )),
            extra_params: Some(extra),
            ..Default::default()
        };

        crate::agent::runtime_context_budget::clamp_local_chat_request_to_context(
            self.provider.as_ref(),
            self.config.max_tokens,
            self.config.response_reserve,
            &mut request,
        );

        let mut stream = match tokio::time::timeout(
            Duration::from_secs(first_chunk_timeout_secs),
            self.provider.stream_completion(request),
        )
        .await
        {
            Ok(result) => result.map_err(Error::from)?,
            Err(_) => {
                return Err(Error::StreamTimeout {
                    timeout_secs: first_chunk_timeout_secs,
                })
            }
        };
        let mut text = String::new();
        let mut generated_chars = 0usize;
        let mut generated_non_whitespace_chars = 0usize;
        let mut trailing_whitespace_chars = 0usize;
        let mut last_progress = Instant::now();
        let mut next_progress_chars = 256usize;
        loop {
            let timeout_secs = if generated_chars == 0 {
                first_chunk_timeout_secs
            } else {
                idle_chunk_timeout_secs
            };
            let next_chunk = match tokio::time::timeout(
                Duration::from_secs(timeout_secs),
                stream.next(),
            )
            .await
            {
                Ok(next_chunk) => next_chunk,
                Err(_) => return Err(Error::StreamTimeout { timeout_secs }),
            };
            let Some(chunk) = next_chunk else {
                break;
            };
            match chunk.map_err(Error::from)? {
                StreamingChoice::Message(delta) => {
                    if let Some(hard_max_chars) = hard_max_chars {
                        let remaining = hard_max_chars.saturating_sub(generated_chars);
                        if remaining == 0 {
                            break;
                        }
                        let delta_chars = delta.chars().count();
                        if delta_chars > remaining {
                            text.push_str(&delta.chars().take(remaining).collect::<String>());
                            generated_chars = hard_max_chars;
                            if let Some(progress) = progress.as_ref() {
                                progress(TextGenerationProgress {
                                    stage: TextGenerationProgressStage::Streaming,
                                    generated_chars,
                                    preview: Some(
                                        text.chars()
                                            .rev()
                                            .take(240)
                                            .collect::<Vec<_>>()
                                            .into_iter()
                                            .rev()
                                            .collect(),
                                    ),
                                    snapshot: Some(text.clone()),
                                });
                            }
                            break;
                        }
                    }
                    let delta_chars = delta.chars().count();
                    let delta_non_whitespace =
                        delta.chars().filter(|ch| !ch.is_whitespace()).count();
                    generated_chars += delta_chars;
                    generated_non_whitespace_chars += delta_non_whitespace;
                    if delta_non_whitespace == 0 {
                        trailing_whitespace_chars =
                            trailing_whitespace_chars.saturating_add(delta_chars);
                    } else {
                        trailing_whitespace_chars = 0;
                    }
                    text.push_str(&delta);
                    if trailing_whitespace_chars
                        >= Self::continuous_text_whitespace_tail_limit(
                            limits.target_chars,
                            generated_non_whitespace_chars,
                        )
                    {
                        break;
                    }
                    let should_report = generated_chars >= next_progress_chars
                        || last_progress.elapsed() >= Duration::from_secs(2);
                    if should_report {
                        if let Some(progress) = progress.as_ref() {
                            progress(TextGenerationProgress {
                                stage: TextGenerationProgressStage::Streaming,
                                generated_chars,
                                preview: Some(
                                    text.chars()
                                        .rev()
                                        .take(240)
                                        .collect::<Vec<_>>()
                                        .into_iter()
                                        .rev()
                                        .collect(),
                                ),
                                snapshot: Some(text.clone()),
                            });
                        }
                        last_progress = Instant::now();
                        next_progress_chars = generated_chars.saturating_add(512);
                    }
                }
                StreamingChoice::Done => break,
                _ => {}
            }
        }
        if let Some(progress) = progress.as_ref() {
            progress(TextGenerationProgress {
                stage: TextGenerationProgressStage::Completed,
                generated_chars,
                preview: Some(text.chars().take(240).collect()),
                snapshot: Some(text.clone()),
            });
        }
        Ok(text)
    }

    async fn chat(
        &self,
        messages: Vec<Message>,
        session_id: Option<String>,
    ) -> Result<crate::agent::ChatOutcome> {
        Agent::chat(self, messages, session_id).await
    }

    async fn run_memory_consolidation_once(&self) -> Result<Option<String>> {
        let Some(consolidator) = &self.sleep_consolidator else {
            return Ok(None);
        };

        let report = consolidator.consolidate().await.map_err(|error| {
            crate::error::Error::Internal(format!("memory consolidation failed: {}", error))
        })?;

        Ok(Some(format!(
            "记忆维护已完成：reviewed={}, verified={}, pruned={}, conflicted={}, conflicts_resolved={}, batches_processed={}, pending_review_reviewed={}, pending_review_verified={}, pending_review_pruned={}, pending_review_retained={}, decay_candidates={}, decay_skipped_protected={}, pruned_by_decay={}, redundant_pruned={}, sovereignty_neutralized={}, experiences_pruned={}, anti_patterns_pruned={}, persistence_failures={}, backlog_before={}, backlog_after={}, backlog_drained={}, duration_ms={}",
            report.entries_reviewed,
            report.entries_verified,
            report.entries_pruned,
            report.entries_conflicted,
            report.conflicts_resolved,
            report.batches_processed,
            report.pending_reviews_reviewed,
            report.pending_reviews_verified,
            report.pending_reviews_pruned,
            report.pending_reviews_retained,
            report.decay_candidates,
            report.decay_skipped_protected,
            report.pruned_by_decay,
            report.redundant_memories_pruned,
            report.sovereignty_violations_neutralized,
            report.experiences_pruned,
            report.anti_patterns_pruned,
            report.persistence_failures,
            report.pending_backlog_before,
            report.pending_backlog_after,
            report.backlog_drained,
            report.duration_ms
        )))
    }

    fn set_all_roles(&self, roles: Vec<AgentRole>) {
        *self.all_roles.write() = roles;
    }

    fn comm_client(&self) -> Option<benshu_comm::client::CommClient> {
        self.comm_client.clone()
    }

    fn agent_identity(&self) -> Option<Arc<parking_lot::RwLock<Option<AgentIdentity>>>> {
        Some(self.agent_identity.clone())
    }

    fn events(&self) -> tokio::sync::broadcast::Receiver<crate::agent::AgentEvent> {
        self.events.subscribe()
    }

    fn security(&self) -> Option<Arc<dyn crate::security::SecurityHandler>> {
        Some(Arc::clone(&self.security) as _)
    }

    fn cancel(&self) {
        self.current_task_token().cancel();
    }

    fn cancel_foreground_task(&self, session_id: Option<&str>) {
        let agent = self.clone();
        let session_id = session_id.map(|sid| sid.to_string());
        tokio::spawn(async move {
            let _ = agent
                .cancel_foreground_task_for_session(session_id.as_deref())
                .await;
        });
    }

    async fn pause_foreground_task(&self, session_id: Option<&str>, note: Option<&str>) -> bool {
        let session_slot =
            Self::foreground_task_slot_key(session_id.or(self.session_id.as_deref()));
        let controller = {
            let active_task = self.active_task.lock().await;
            active_task
                .get(&session_slot)
                .map(|handle| handle.pause_controller.clone())
        };
        let Some(controller) = controller else {
            return false;
        };
        if let Some(note) = note {
            controller.queue_input(note.to_string()).await;
        }
        controller.pause();
        true
    }

    async fn resume_foreground_task(
        &self,
        session_id: Option<&str>,
        instruction: Option<&str>,
    ) -> bool {
        let session_slot =
            Self::foreground_task_slot_key(session_id.or(self.session_id.as_deref()));
        let controller = {
            let active_task = self.active_task.lock().await;
            active_task
                .get(&session_slot)
                .map(|handle| handle.pause_controller.clone())
        };
        let Some(controller) = controller else {
            return false;
        };
        if let Some(instruction) = instruction {
            controller.queue_input(instruction.to_string()).await;
        }
        controller.resume();
        true
    }

    async fn is_foreground_task_paused(&self, session_id: Option<&str>) -> bool {
        let session_slot =
            Self::foreground_task_slot_key(session_id.or(self.session_id.as_deref()));
        let active_task = self.active_task.lock().await;
        active_task
            .get(&session_slot)
            .map(|handle| handle.pause_controller.is_paused())
            .unwrap_or(false)
    }

    fn ensure_active_token(&self) {
        Agent::ensure_active_token(self);
    }

    fn has_active_foreground_task(&self) -> bool {
        self.active_task
            .try_lock()
            .map(|guard| !guard.is_empty())
            .unwrap_or(false)
    }

    fn has_active_foreground_task_for_session(&self, session_id: Option<&str>) -> bool {
        self.has_active_foreground_task_for_session_internal(session_id)
    }
}

#[cfg(test)]
include!("foreground_runtime/tests.rs");

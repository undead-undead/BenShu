use super::core::Agent;
use crate::agent::message::{Content, Message, Role};
use crate::agent::provider::Provider;
use crate::agent::reasoner::{
    apply_media_followup_capability_route, approved_forge_request_from_messages,
    forged_session_tool_already_executed, forged_session_tool_names_from_messages,
    matched_skill_manual_name, matched_skill_manual_name_from_messages,
    resolve_skill_asset_path_from_messages, skill_asset_already_loaded,
    skill_manual_already_loaded, Reasoner,
};
use crate::agent::reasoner::{
    media_followup_capability_contract, media_followup_strategies_from_messages,
    render_media_followup_strategy_prompt,
};
use crate::agent::streaming::{StreamingChoice, StreamingResponse};
use crate::agent::truth_verification_policy::TruthVerificationPolicyEngine;
use crate::error::{Error, Result};
use crate::hooks::{HookEvent, HookResult, HookTiming};
use crate::skills::tool::{
    capability_route_requires_real_tool_call, capability_route_system_message,
    capability_route_tool_allowlist_for_query, classify_query_capability_route,
    coordinator_default_tool_names_for_query, coordinator_routing_judgment_only_message,
    coordinator_task_mode_label, coordinator_task_mode_should_include_media_followup_prompt,
    coordinator_task_mode_should_include_route_prompt,
    coordinator_task_mode_should_include_truth_guidance, coordinator_task_mode_system_message,
    query_requests_image_generation, query_requests_routing_judgment_only,
    select_coordinator_task_mode, CapabilityRouteHint, ToolDefinition,
};
use futures::stream;

fn message_has_media(message: &Message) -> bool {
    matches!(
        &message.content,
        Content::Parts(parts)
            if parts.iter().any(|part| matches!(
                part,
                crate::agent::message::ContentPart::Image { .. }
                    | crate::agent::message::ContentPart::Audio { .. }
                    | crate::agent::message::ContentPart::Video { .. }
            ))
    )
}

fn latest_user_message_with_media(messages: &[Message]) -> Option<&Message> {
    messages
        .iter()
        .rev()
        .find(|message| message.role == Role::User && message_has_media(message))
}

fn latest_user_message_has_media(messages: &[Message]) -> bool {
    latest_user_message_with_media(messages).is_some()
}

fn route_requires_real_tool_call_for_turn(
    route: CapabilityRouteHint,
    has_media_input: bool,
) -> bool {
    if has_media_input
        && matches!(
            route,
            CapabilityRouteHint::DocumentUnderstanding | CapabilityRouteHint::VisualUnderstanding
        )
    {
        return false;
    }

    capability_route_requires_real_tool_call(route)
}

fn should_force_direct_multimodal_answer(
    raw_capability_route: Option<CapabilityRouteHint>,
    has_media_input: bool,
    has_media_followup_contract: bool,
) -> bool {
    has_media_input
        && !has_media_followup_contract
        && matches!(
            raw_capability_route,
            Some(CapabilityRouteHint::DocumentUnderstanding)
                | Some(CapabilityRouteHint::VisualUnderstanding)
                | None
        )
}

impl<P: Provider + 'static> Agent<P> {
    pub(crate) fn inject_inference_contract_metadata(
        &self,
        extra: &mut serde_json::Value,
        session_id: Option<&str>,
        priority: i8,
    ) {
        if !extra.is_object() {
            *extra = serde_json::Value::Object(serde_json::Map::new());
        }

        if let serde_json::Value::Object(map) = extra {
            map.insert(
                "inference_priority".to_string(),
                serde_json::json!(priority),
            );
            map.insert(
                "inference_session_authority".to_string(),
                serde_json::json!("backend-local-cache"),
            );
            map.insert(
                "inference_runtime_owner".to_string(),
                serde_json::json!("inference"),
            );
            map.insert(
                "brain_runtime_owner".to_string(),
                serde_json::json!("brain"),
            );
            map.insert(
                "inference_degradation_surface".to_string(),
                serde_json::json!("provider-metadata"),
            );
            if let Some(session_id) = session_id {
                map.insert(
                    "inference_session_id".to_string(),
                    serde_json::json!(session_id),
                );
            }
        }
    }

    /// Stream a prompt response
    pub async fn stream(&self, prompt: impl Into<String>) -> Result<StreamingResponse> {
        let messages = vec![Message::user(prompt.into())];
        self.stream_chat(messages).await
    }

    async fn resolve_stream_tools(
        &self,
        messages: &[Message],
        extra: &serde_json::Value,
    ) -> (
        Vec<ToolDefinition>,
        usize,
        usize,
        bool,
        bool,
        Vec<String>,
        &'static str,
    ) {
        let matched_skill_manual = matched_skill_manual_name(extra)
            .or_else(|| matched_skill_manual_name_from_messages(messages));
        let matched_skill_asset_path =
            resolve_skill_asset_path_from_messages(messages, matched_skill_manual.as_deref());
        let latest_user_input = messages
            .iter()
            .rev()
            .find_map(|message| match message.role {
                Role::User => Some(message.text()),
                _ => None,
            })
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty());
        let direct_capability_route = latest_user_input
            .as_deref()
            .filter(|query| !query_requests_routing_judgment_only(query))
            .and_then(classify_query_capability_route)
            .filter(|route| capability_route_requires_real_tool_call(*route));
        let routing_judgment_only = latest_user_input
            .as_deref()
            .is_some_and(query_requests_routing_judgment_only);
        let pending_forge_followup_tools = if approved_forge_request_from_messages(messages) {
            forged_session_tool_names_from_messages(messages)
                .into_iter()
                .filter(|tool_name| !forged_session_tool_already_executed(messages, tool_name))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let requires_skill_manual_first =
            matched_skill_manual.as_deref().is_some_and(|skill_name| {
                self.tools.contains("read_skill_manual")
                    && !skill_manual_already_loaded(messages, skill_name)
            });
        let requires_skill_asset_first = !requires_skill_manual_first
            && matched_skill_manual
                .as_deref()
                .zip(matched_skill_asset_path.as_deref())
                .is_some_and(|(skill_name, asset_path)| {
                    self.tools.contains("read_skill_asset")
                        && skill_manual_already_loaded(messages, skill_name)
                        && !skill_asset_already_loaded(messages, asset_path)
                });

        if !pending_forge_followup_tools.is_empty() {
            let mut allowed: std::collections::HashSet<String> =
                pending_forge_followup_tools.iter().cloned().collect();
            if let Some(ref enabled) = self.enabled_tools {
                let enabled_set = enabled.read().clone();
                allowed.retain(|name| enabled_set.contains(name));
            }
            let total = self.tools.definitions_filtered(Some(&allowed)).await.len();
            let visible = self.tools.definitions_filtered(Some(&allowed)).await;
            return (
                visible,
                0,
                total,
                false,
                false,
                pending_forge_followup_tools,
                "minimal",
            );
        }

        if requires_skill_manual_first {
            let mut allowed = std::collections::HashSet::from(["read_skill_manual".to_string()]);
            if let Some(ref enabled) = self.enabled_tools {
                let enabled_set = enabled.read().clone();
                allowed.retain(|name| enabled_set.contains(name));
            }
            let total = self.tools.definitions_filtered(Some(&allowed)).await.len();
            let visible = self.tools.definitions_filtered(Some(&allowed)).await;
            return (visible, 0, total, true, false, Vec::new(), "minimal");
        }

        if requires_skill_asset_first {
            let mut allowed = std::collections::HashSet::from(["read_skill_asset".to_string()]);
            if let Some(ref enabled) = self.enabled_tools {
                let enabled_set = enabled.read().clone();
                allowed.retain(|name| enabled_set.contains(name));
            }
            let total = self.tools.definitions_filtered(Some(&allowed)).await.len();
            let visible = self.tools.definitions_filtered(Some(&allowed)).await;
            return (visible, 0, total, false, true, Vec::new(), "minimal");
        }

        if routing_judgment_only {
            return (Vec::new(), 0, 0, false, false, Vec::new(), "routing_only");
        }

        if let Some(ref enabled) = self.enabled_tools {
            if let Some(route) = direct_capability_route {
                let mut allowed: std::collections::HashSet<String> = match route {
                    CapabilityRouteHint::RealtimeLookup(_) => {
                        capability_route_tool_allowlist_for_query(
                            route,
                            latest_user_input.as_deref(),
                        )
                    }
                    _ => capability_route_tool_allowlist_for_query(
                        route,
                        latest_user_input.as_deref(),
                    ),
                };
                let enabled_set = enabled.read().clone();
                allowed.retain(|name| enabled_set.contains(name));
                if !allowed.is_empty() {
                    let total = self.tools.definitions_filtered(Some(&allowed)).await.len();
                    let visible = self.tools.definitions_filtered(Some(&allowed)).await;
                    return (visible, 0, total, false, false, Vec::new(), "minimal");
                }
            }
            let allowed = enabled.read().clone();
            let total = self.tools.definitions_filtered(Some(&allowed)).await.len();
            let (visible, deferred) = self
                .tools
                .definitions_prompt_visible_filtered(Some(&allowed))
                .await;
            return (visible, deferred, total, false, false, Vec::new(), "full");
        }

        if let Some(route) = direct_capability_route {
            let allowed: std::collections::HashSet<String> = match route {
                CapabilityRouteHint::RealtimeLookup(_) => {
                    capability_route_tool_allowlist_for_query(route, latest_user_input.as_deref())
                }
                _ => capability_route_tool_allowlist_for_query(route, latest_user_input.as_deref()),
            };
            if !allowed.is_empty() {
                let total = self.tools.definitions_filtered(Some(&allowed)).await.len();
                let visible = self.tools.definitions_filtered(Some(&allowed)).await;
                return (visible, 0, total, false, false, Vec::new(), "minimal");
            }
        }

        let total = self.tools.definitions().await.len();
        let (visible, deferred) = self.tools.definitions_prompt_visible_filtered(None).await;
        (visible, deferred, total, false, false, Vec::new(), "full")
    }

    /// Stream a chat response
    pub async fn stream_chat(&self, messages: Vec<Message>) -> Result<StreamingResponse> {
        let mut extra = self
            .config
            .extra_params
            .clone()
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
        let inference_priority = self.local_inference_priority();

        if self.config.json_mode {
            if let serde_json::Value::Object(ref mut map) = extra {
                if !map.contains_key("response_format") {
                    map.insert(
                        "response_format".to_string(),
                        serde_json::json!({ "type": "json_object" }),
                    );
                }
            }
        }

        self.inject_inference_contract_metadata(
            &mut extra,
            self.session_id.as_deref(),
            inference_priority,
        );

        let latest_user_input = messages
            .iter()
            .rev()
            .find_map(|message| match message.role {
                Role::User => Some(message.text()),
                _ => None,
            })
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty());

        let (
            mut tools,
            mut deferred_tool_count,
            mut total_tool_count,
            requires_skill_manual_first,
            requires_skill_asset_first,
            pending_forge_followup_tools,
            tool_surface_mode,
        ) = self.resolve_stream_tools(&messages, &extra).await;
        let media_followup_strategies = media_followup_strategies_from_messages(&messages);
        let media_followup_contract =
            media_followup_capability_contract(&media_followup_strategies);
        apply_media_followup_capability_route(&mut extra, media_followup_contract);
        let force_direct_multimodal_answer = should_force_direct_multimodal_answer(
            classify_query_capability_route(latest_user_input.as_deref().unwrap_or_default()),
            latest_user_message_has_media(&messages),
            media_followup_contract.is_some(),
        );
        if media_followup_contract
            .map(|contract| contract.prefer_document_understanding_tools)
            .unwrap_or(false)
            && pending_forge_followup_tools.is_empty()
            && !requires_skill_manual_first
            && !requires_skill_asset_first
            && !force_direct_multimodal_answer
        {
            let mut allowed: std::collections::HashSet<String> =
                crate::skills::tool::capability_route_preferred_tool_names(
                    CapabilityRouteHint::DocumentUnderstanding,
                )
                .iter()
                .map(|name| (*name).to_string())
                .collect();
            if let Some(ref enabled) = self.enabled_tools {
                let enabled_set = enabled.read().clone();
                allowed.retain(|name| enabled_set.contains(name));
            }
            if !allowed.is_empty() {
                total_tool_count = self.tools.definitions_filtered(Some(&allowed)).await.len();
                let (visible, deferred) = self
                    .tools
                    .definitions_prompt_visible_filtered(Some(&allowed))
                    .await;
                tools = visible;
                deferred_tool_count = deferred;
            }
        }

        if latest_user_input
            .as_deref()
            .is_some_and(query_requests_image_generation)
            && !latest_user_message_has_media(&messages)
            && !Reasoner::<P>::tool_surface_has_generate_image(&tools)
            && !tools.iter().any(|tool| tool.name == "delegate")
        {
            let fallback = Reasoner::<P>::image_generation_unavailable_fallback_text(
                latest_user_input.as_deref().unwrap_or_default(),
            );
            return Ok(StreamingResponse::from_stream(stream::iter(vec![
                Ok(StreamingChoice::Message(fallback)),
                Ok(StreamingChoice::Done),
            ])));
        }

        if force_direct_multimodal_answer {
            tools.clear();
            deferred_tool_count = 0;
            total_tool_count = 0;
        }

        let mut request = crate::agent::provider::ChatRequest {
            model: self.config.model.clone(),
            system_prompt: Some(self.config.preamble.clone()),
            messages,
            tools,
            temperature: self.config.temperature,
            max_tokens: self.config.max_tokens,
            extra_params: Some(extra),
            session_id: self.session_id.clone(),
            enable_cache_control: self.config.enable_cache_control,
            continuation_hint: self.session_id.as_ref().map(|session_id| {
                benshu_provider_core::ContinuationHint {
                    user_session_id: Some(session_id.clone()),
                    continuation_frontier_id: Some(format!("{session_id}::stream-chat")),
                    ..Default::default()
                }
            }),
        };
        let has_media_input = latest_user_message_has_media(&request.messages);
        let latest_user_input = request
            .messages
            .iter()
            .rev()
            .find(|message| matches!(message.role, Role::User))
            .map(|message| message.text());
        let routing_judgment_only = latest_user_input
            .as_deref()
            .is_some_and(query_requests_routing_judgment_only);
        let raw_capability_route = latest_user_input
            .as_deref()
            .and_then(classify_query_capability_route);
        let direct_capability_route = raw_capability_route
            .filter(|_| !routing_judgment_only)
            .filter(|route| route_requires_real_tool_call_for_turn(*route, has_media_input));
        let coordinator_task_mode = select_coordinator_task_mode(
            raw_capability_route,
            !media_followup_strategies.is_empty(),
        );
        if let Some(extra_map) = request
            .extra_params
            .as_mut()
            .and_then(|extra| extra.as_object_mut())
        {
            extra_map.insert(
                "task_mode".to_string(),
                serde_json::json!(coordinator_task_mode_label(coordinator_task_mode)),
            );
        }
        if coordinator_task_mode_should_include_media_followup_prompt(
            coordinator_task_mode,
            !media_followup_strategies.is_empty(),
        ) {
            if let Some(media_followup_prompt) =
                render_media_followup_strategy_prompt(&media_followup_strategies)
            {
                request.system_prompt = Some(match request.system_prompt.take() {
                    Some(existing) if !existing.trim().is_empty() => {
                        format!("{existing}\n\n{media_followup_prompt}")
                    }
                    _ => media_followup_prompt,
                });
            }
        }
        request.system_prompt = Some(match request.system_prompt.take() {
            Some(existing) if !existing.trim().is_empty() => format!(
                "{existing}\n\n{}",
                coordinator_task_mode_system_message(coordinator_task_mode)
            ),
            _ => coordinator_task_mode_system_message(coordinator_task_mode).to_string(),
        });
        if routing_judgment_only {
            request.system_prompt = Some(match request.system_prompt.take() {
                Some(existing) if !existing.trim().is_empty() => format!(
                    "{existing}\n\n{}",
                    coordinator_routing_judgment_only_message()
                ),
                _ => coordinator_routing_judgment_only_message().to_string(),
            });
        }
        let truth_verification_policy_active = latest_user_input.as_deref().is_some_and(|query| {
            TruthVerificationPolicyEngine::default().should_include_guidance_for_query(query)
        });
        let truth_verification_guidance_active =
            coordinator_task_mode_should_include_truth_guidance(
                coordinator_task_mode,
                truth_verification_policy_active,
                media_followup_contract.is_some(),
            );
        if truth_verification_guidance_active {
            let truth_verification_prompt =
                TruthVerificationPolicyEngine::default().guidance_prompt();
            request.system_prompt = Some(match request.system_prompt.take() {
                Some(existing) if !existing.trim().is_empty() => {
                    format!("{existing}\n\n{truth_verification_prompt}")
                }
                _ => truth_verification_prompt.to_string(),
            });
        }
        if let Some(route_system_prompt) = latest_user_input
            .as_deref()
            .and_then(classify_query_capability_route)
            .filter(|route| capability_route_requires_real_tool_call(*route))
            .filter(|route| {
                coordinator_task_mode_should_include_route_prompt(coordinator_task_mode, *route)
            })
            .and_then(|route| {
                latest_user_input.as_deref().and_then(|user_request| {
                    capability_route_system_message(
                        user_request,
                        route,
                        None,
                        matched_skill_manual_name(
                            request
                                .extra_params
                                .as_ref()
                                .unwrap_or(&serde_json::Value::Null),
                        )
                        .or_else(|| matched_skill_manual_name_from_messages(&request.messages))
                        .as_deref(),
                    )
                })
            })
        {
            request.system_prompt = Some(match request.system_prompt.take() {
                Some(existing) if !existing.trim().is_empty() => {
                    format!("{existing}\n\n{route_system_prompt}")
                }
                _ => route_system_prompt,
            });
        }

        let mut before_llm_hook = HookEvent::new(HookTiming::BeforeLlm);
        if let Some(input) = latest_user_input {
            before_llm_hook = before_llm_hook.with_user_input(input);
        }
        self.apply_provider_before_llm_metadata(
            &mut before_llm_hook.metadata,
            self.provider.name(),
            &request.model,
            request.tools.len(),
            total_tool_count,
            deferred_tool_count,
        );
        before_llm_hook.metadata.insert(
            "tool_surface_mode".to_string(),
            tool_surface_mode.to_string(),
        );
        before_llm_hook
            .metadata
            .insert("chat_route".to_string(), "coordinator".to_string());
        before_llm_hook.metadata.insert(
            "task_mode".to_string(),
            coordinator_task_mode_label(coordinator_task_mode).to_string(),
        );
        if routing_judgment_only {
            before_llm_hook
                .metadata
                .insert("routing_judgment_only".to_string(), "true".to_string());
        }
        if let Some(capability_route) = request
            .extra_params
            .as_ref()
            .and_then(|extra| extra.get("capability_route"))
            .and_then(|value| value.as_str())
        {
            before_llm_hook
                .metadata
                .insert("capability_route".to_string(), capability_route.to_string());
        }
        if let Some(skill_name) = matched_skill_manual_name(
            request
                .extra_params
                .as_ref()
                .unwrap_or(&serde_json::Value::Null),
        )
        .or_else(|| matched_skill_manual_name_from_messages(&request.messages))
        {
            before_llm_hook
                .metadata
                .insert("matched_skill_manual".to_string(), skill_name);
        }
        let matched_skill_manual = matched_skill_manual_name(
            request
                .extra_params
                .as_ref()
                .unwrap_or(&serde_json::Value::Null),
        )
        .or_else(|| matched_skill_manual_name_from_messages(&request.messages));
        let matched_skill_asset_path = resolve_skill_asset_path_from_messages(
            &request.messages,
            matched_skill_manual.as_deref(),
        );
        if let Some(asset_path) = matched_skill_asset_path.clone() {
            before_llm_hook
                .metadata
                .insert("matched_skill_asset_path".to_string(), asset_path);
        }
        self.apply_forge_followup_before_llm_metadata(
            &mut before_llm_hook.metadata,
            &pending_forge_followup_tools,
        );
        self.apply_media_followup_request_metadata(
            &mut before_llm_hook.metadata,
            &media_followup_strategies,
            media_followup_contract
                .as_ref()
                .map(|contract| contract.capability_route),
            media_followup_contract
                .as_ref()
                .map(|contract| contract.execution_surface),
        );
        if truth_verification_guidance_active {
            before_llm_hook.metadata.insert(
                "truth_verification_guidance_active".to_string(),
                "true".to_string(),
            );
        }
        if requires_skill_manual_first {
            before_llm_hook
                .metadata
                .insert("skill_manual_gate_active".to_string(), "true".to_string());
        }
        if requires_skill_asset_first {
            before_llm_hook
                .metadata
                .insert("skill_asset_gate_active".to_string(), "true".to_string());
        }

        match crate::agent::protocol::AgentLiaison::run_runtime_hook(self, before_llm_hook).await? {
            HookResult::Continue | HookResult::Skip => {}
            HookResult::Modify(injected_system_prompt) => {
                if !injected_system_prompt.trim().is_empty() {
                    request.system_prompt = Some(match request.system_prompt.take() {
                        Some(existing) if !existing.trim().is_empty() => {
                            format!("{existing}\n\n{injected_system_prompt}")
                        }
                        _ => injected_system_prompt,
                    });
                }
            }
            HookResult::Abort(reason) => {
                return Err(Error::agent_config(format!(
                    "Before-LLM middleware aborted runtime: {reason}"
                )));
            }
        }

        crate::agent::runtime_context_budget::clamp_local_chat_request_to_context(
            self.provider.as_ref(),
            self.config.max_tokens,
            self.config.response_reserve,
            &mut request,
        );

        self.provider
            .stream_completion(request)
            .await
            .map_err(Into::into)
    }
}

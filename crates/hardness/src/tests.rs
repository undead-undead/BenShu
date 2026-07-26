use crate::{
    classify_extended_pre_flight_level, classify_failure, decide_execution_tool_reply_requirement,
    decide_interventions, decide_reflexion_strategy_upgrade, extract_reflexion_critique_reason,
    is_explicit_image_generation_first_attempt, is_frontstage_single_image_turn,
    is_simple_media_understanding_turn, retry_allows_reflexion_upgrade, sanitize_task_complexity,
    should_append_reflexion_recovery_prompt, should_enqueue_failure_analysis,
    strip_frontstage_media_injection, ComplexityScore, ExecutionToolReplyRequirementInput,
    FailureAnalysisInput, FailureClass, InitialReasoningStrategy, InitialReasoningStrategyInput,
    InterventionGateInput, MediaKind, MessageSnapshot, PreFlightRouteClass, ReflexionUpgradeInput,
    ReflexionUpgradeReason, TaskComplexity, ToolFirstRecoveryInput,
};

#[test]
fn media_turns_do_not_upgrade_into_reflexion() {
    let decision = decide_reflexion_strategy_upgrade(ReflexionUpgradeInput {
        current_strategy_is_react: true,
        complexity_score: 92,
        retry_count: 1,
        max_reflexion_retries: 3,
        retry_recovery_eligible: false,
        explicit_image_generation_turn: false,
        has_media_input: true,
        simple_media_understanding: true,
    });

    assert!(!decision.should_upgrade);
    assert_eq!(decision.reason, None);
}

#[test]
fn bounded_retry_recovery_can_upgrade_into_reflexion() {
    let decision = decide_reflexion_strategy_upgrade(ReflexionUpgradeInput {
        current_strategy_is_react: true,
        complexity_score: 45,
        retry_count: 1,
        max_reflexion_retries: 3,
        retry_recovery_eligible: true,
        explicit_image_generation_turn: false,
        has_media_input: false,
        simple_media_understanding: false,
    });

    assert!(decision.should_upgrade);
    assert_eq!(decision.reason, Some(ReflexionUpgradeReason::RetryRecovery));
}

#[test]
fn simple_media_understanding_suppresses_error_reflexion_and_status_recap() {
    let decision = decide_interventions(InterventionGateInput {
        token_usage_total: None,
        token_budget: None,
        cpu_usage: 10.0,
        mem_pressure: 10.0,
        enable_reflexion: true,
        quality_error_detected: true,
        complexity_score: 0.95,
        predicted_output_tokens: 600,
        is_parallelizable: false,
        current_step: 6,
        estimated_steps: 6,
        total_chars: 6000,
        is_local_provider: false,
        is_sub_agent: false,
        is_specialist_worker: false,
        simple_media_understanding: true,
        lightweight_repo_inspection: false,
        compound_realtime_followup_execution: false,
        status_recap_threshold_steps: 5,
        status_recap_threshold_chars: 2000,
    });

    assert!(!decision.error_reflexion);
    assert!(!decision.status_recap);
}

#[test]
fn lightweight_repo_inspection_suppresses_fractal_and_status_recap() {
    let decision = decide_interventions(InterventionGateInput {
        token_usage_total: None,
        token_budget: None,
        cpu_usage: 10.0,
        mem_pressure: 10.0,
        enable_reflexion: true,
        quality_error_detected: false,
        complexity_score: 0.98,
        predicted_output_tokens: 1200,
        is_parallelizable: false,
        current_step: 2,
        estimated_steps: 4,
        total_chars: 7000,
        is_local_provider: true,
        is_sub_agent: false,
        is_specialist_worker: false,
        simple_media_understanding: false,
        lightweight_repo_inspection: true,
        compound_realtime_followup_execution: false,
        status_recap_threshold_steps: 2,
        status_recap_threshold_chars: 2000,
    });

    assert!(!decision.status_recap);
}

#[test]
fn specialist_workers_skip_orchestration_interventions() {
    let decision = decide_interventions(InterventionGateInput {
        token_usage_total: None,
        token_budget: None,
        cpu_usage: 10.0,
        mem_pressure: 10.0,
        enable_reflexion: true,
        quality_error_detected: false,
        complexity_score: 0.98,
        predicted_output_tokens: 2600,
        is_parallelizable: true,
        current_step: 6,
        estimated_steps: 12,
        total_chars: 5000,
        is_local_provider: true,
        is_sub_agent: true,
        is_specialist_worker: true,
        simple_media_understanding: false,
        lightweight_repo_inspection: false,
        compound_realtime_followup_execution: false,
        status_recap_threshold_steps: 5,
        status_recap_threshold_chars: 2000,
    });

    assert!(!decision.status_recap);
}

#[test]
fn compound_realtime_followup_execution_suppresses_orchestration_noise() {
    let decision = decide_interventions(InterventionGateInput {
        token_usage_total: None,
        token_budget: None,
        cpu_usage: 10.0,
        mem_pressure: 10.0,
        enable_reflexion: true,
        quality_error_detected: false,
        complexity_score: 0.98,
        predicted_output_tokens: 2600,
        is_parallelizable: true,
        current_step: 7,
        estimated_steps: 12,
        total_chars: 7000,
        is_local_provider: true,
        is_sub_agent: false,
        is_specialist_worker: false,
        simple_media_understanding: false,
        lightweight_repo_inspection: false,
        compound_realtime_followup_execution: true,
        status_recap_threshold_steps: 5,
        status_recap_threshold_chars: 2000,
    });

    assert!(!decision.status_recap);
}

#[test]
fn strips_parsed_media_injection_from_frontstage_text() {
    let cleaned = strip_frontstage_media_injection(
        "请描述这张图片\n[Parsed image Attachment via local_sensory_vlm]\nsource: file:///tmp/demo.png\n一只橙色猫坐在窗台上。\nparser_mode: visual",
    );
    assert_eq!(cleaned, "请描述这张图片");
}

#[test]
fn detects_simple_media_understanding_turn() {
    let snapshot = MessageSnapshot {
        text: "请描述这张图片里有什么".to_string(),
        media: vec![MediaKind::Image],
    };
    let complexity = ComplexityScore {
        score: 0.91,
        reason: "MULTI_MODAL_INPUT(1)".to_string(),
        predicted_output_tokens: 800,
        is_parallelizable: false,
        level: 1,
        metadata: serde_json::json!({ "media_types": ["image"] }),
    };

    assert!(is_simple_media_understanding_turn(&snapshot, &complexity));
    assert!(is_frontstage_single_image_turn(
        &snapshot,
        &complexity,
        1,
        200
    ));
}

#[test]
fn media_input_prefers_react_for_initial_strategy() {
    let strategy = crate::decide_initial_reasoning_strategy(InitialReasoningStrategyInput {
        force_react_due_to_resource_pressure: false,
        throttled_by_metabolic_guard: false,
        reflexion_enabled: true,
        explicit_image_generation_turn: false,
        light_frontstage_turn: false,
        has_media_input: true,
    });

    assert_eq!(strategy, InitialReasoningStrategy::ReAct);
}

#[test]
fn lightweight_repo_summary_does_not_trigger_extended_preflight() {
    let level = classify_extended_pre_flight_level(
        "请帮我查看当前项目里和 hardness 相关的文档，给我一个简短中文总结。",
        PreFlightRouteClass::Complex,
        false,
        false,
    );

    assert_eq!(level, crate::ExtendedPreFlightLevel::None);
}

#[test]
fn repo_modification_request_still_keeps_complex_preflight() {
    let level = classify_extended_pre_flight_level(
        "请帮我修改当前项目里的 hardness 模块并提交补丁。",
        PreFlightRouteClass::Complex,
        false,
        false,
    );

    assert_eq!(level, crate::ExtendedPreFlightLevel::ComplexTask);
}

#[test]
fn tool_first_recovery_requires_remaining_steps_and_visible_tools() {
    assert!(crate::decide_tool_first_recovery(ToolFirstRecoveryInput {
        current_step: 1,
        max_steps: 4,
        available_tool_count: 2,
        has_recent_tool_execution_required_prompt: false,
        simple_media_understanding: false,
    }));

    assert!(!crate::decide_tool_first_recovery(ToolFirstRecoveryInput {
        current_step: 4,
        max_steps: 4,
        available_tool_count: 2,
        has_recent_tool_execution_required_prompt: false,
        simple_media_understanding: false,
    }));

    assert!(!crate::decide_tool_first_recovery(ToolFirstRecoveryInput {
        current_step: 1,
        max_steps: 4,
        available_tool_count: 0,
        has_recent_tool_execution_required_prompt: false,
        simple_media_understanding: false,
    }));

    assert!(!crate::decide_tool_first_recovery(ToolFirstRecoveryInput {
        current_step: 1,
        max_steps: 4,
        available_tool_count: 2,
        has_recent_tool_execution_required_prompt: true,
        simple_media_understanding: false,
    }));

    assert!(!crate::decide_tool_first_recovery(ToolFirstRecoveryInput {
        current_step: 1,
        max_steps: 4,
        available_tool_count: 2,
        has_recent_tool_execution_required_prompt: false,
        simple_media_understanding: true,
    }));
}

#[test]
fn lookup_evidence_recovery_prefers_observation_for_weak_evidence() {
    let action = crate::decide_lookup_evidence_recovery(crate::LookupEvidenceRecoveryInput {
        current_step: 2,
        max_steps: 8,
        evidence_quality: crate::EvidenceQuality::MissingConcreteSource,
        has_search_tool: true,
        search_attempts: 1,
        has_observation_tool: true,
        observation_already_attempted: false,
        has_delegate_tool: true,
        specialist_already_attempted: true,
        required_persistence: true,
    });

    assert_eq!(action, crate::RecoveryAction::SwitchObservationSurface);
}

#[test]
fn lookup_evidence_recovery_stops_after_observation_and_specialist() {
    let action = crate::decide_lookup_evidence_recovery(crate::LookupEvidenceRecoveryInput {
        current_step: 6,
        max_steps: 8,
        evidence_quality: crate::EvidenceQuality::LowInformation,
        has_search_tool: true,
        search_attempts: 3,
        has_observation_tool: true,
        observation_already_attempted: true,
        has_delegate_tool: true,
        specialist_already_attempted: true,
        required_persistence: true,
    });

    assert_eq!(action, crate::RecoveryAction::EmitBlocker);
}

#[test]
fn lookup_evidence_recovery_finalizes_sufficient_evidence() {
    let action = crate::decide_lookup_evidence_recovery(crate::LookupEvidenceRecoveryInput {
        current_step: 8,
        max_steps: 8,
        evidence_quality: crate::EvidenceQuality::Sufficient,
        has_search_tool: false,
        search_attempts: 0,
        has_observation_tool: false,
        observation_already_attempted: false,
        has_delegate_tool: false,
        specialist_already_attempted: false,
        required_persistence: false,
    });

    assert_eq!(action, crate::RecoveryAction::FinalizeFromEvidence);
}

#[test]
fn completion_gate_maps_missing_effects_to_failures() {
    assert_eq!(
        crate::decide_completion_gate(crate::CompletionGateSignal::Complete),
        crate::CompletionGateDecision::Pass
    );
    assert_eq!(
        crate::decide_completion_gate(crate::CompletionGateSignal::MissingRequiredEffect),
        crate::CompletionGateDecision::Fail
    );
    assert_eq!(
        crate::decide_completion_gate(crate::CompletionGateSignal::RuntimeBlocker),
        crate::CompletionGateDecision::Blocked
    );
}

#[test]
fn simple_multimodal_document_turn_does_not_require_tool_reply() {
    assert!(!decide_execution_tool_reply_requirement(
        ExecutionToolReplyRequirementInput {
            has_media_input: true,
            normalized_text_is_empty: false,
            document_understanding_turn: true,
            capability_route_requires_real_tool_call: true,
        }
    ));

    assert!(decide_execution_tool_reply_requirement(
        ExecutionToolReplyRequirementInput {
            has_media_input: false,
            normalized_text_is_empty: false,
            document_understanding_turn: true,
            capability_route_requires_real_tool_call: true,
        }
    ));
}

#[test]
fn reflexion_critique_reason_extraction_is_normalized() {
    assert_eq!(
        extract_reflexion_critique_reason("[CRITIQUE] Missing the final answer section."),
        Some("Missing the final answer section.".to_string())
    );
    assert_eq!(
        extract_reflexion_critique_reason("[CRITIQUE]"),
        Some("unspecified critique".to_string())
    );
    assert_eq!(extract_reflexion_critique_reason("[PASSED]"), None);
    assert_eq!(
        extract_reflexion_critique_reason(
            "The response is accurate and sufficient. It could be more verbose, but it is acceptable. [PASSED]"
        ),
        None
    );
    assert_eq!(
        extract_reflexion_critique_reason("[critique] Missing one concrete next step."),
        Some("Missing one concrete next step.".to_string())
    );
    assert_eq!(
        extract_reflexion_critique_reason(
            "[CRITIQUE] The response is accurate and appropriate. There are no missing steps or factual errors."
        ),
        None
    );
    assert_eq!(
        extract_reflexion_critique_reason(
            "[CRITIQUE] The response correctly states availability without unnecessary elaboration."
        ),
        None
    );
    assert_eq!(
        extract_reflexion_critique_reason("[CRITIQUE] The response lacks the requested source."),
        Some("The response lacks the requested source.".to_string())
    );
}

#[test]
fn failure_analysis_requires_manager_tool_and_error() {
    assert!(should_enqueue_failure_analysis(FailureAnalysisInput {
        evolution_manager_available: true,
        tool_name_is_empty: false,
        normalized_error_is_empty: false,
    }));

    assert!(!should_enqueue_failure_analysis(FailureAnalysisInput {
        evolution_manager_available: false,
        tool_name_is_empty: false,
        normalized_error_is_empty: false,
    }));

    assert!(!should_enqueue_failure_analysis(FailureAnalysisInput {
        evolution_manager_available: true,
        tool_name_is_empty: true,
        normalized_error_is_empty: false,
    }));

    assert!(!should_enqueue_failure_analysis(FailureAnalysisInput {
        evolution_manager_available: true,
        tool_name_is_empty: false,
        normalized_error_is_empty: true,
    }));
}

#[test]
fn explicit_image_generation_first_attempt_requires_clean_turn() {
    assert!(is_explicit_image_generation_first_attempt(false, 0, true));
    assert!(!is_explicit_image_generation_first_attempt(true, 0, true));
    assert!(!is_explicit_image_generation_first_attempt(false, 1, true));
    assert!(!is_explicit_image_generation_first_attempt(false, 0, false));
}

#[test]
fn reflexion_recovery_prompt_only_applies_to_non_execution_failures() {
    assert!(should_append_reflexion_recovery_prompt(
        true,
        false,
        FailureClass::Quality
    ));
    assert!(!should_append_reflexion_recovery_prompt(
        false,
        false,
        FailureClass::Quality
    ));
    assert!(!should_append_reflexion_recovery_prompt(
        true,
        true,
        FailureClass::Quality
    ));
    assert!(!should_append_reflexion_recovery_prompt(
        true,
        false,
        FailureClass::Execution
    ));
}

#[test]
fn retry_only_allows_reflexion_for_quality_failures() {
    assert!(retry_allows_reflexion_upgrade(1, 3, FailureClass::Quality));
    assert!(!retry_allows_reflexion_upgrade(
        1,
        3,
        FailureClass::Transport
    ));
    assert!(!retry_allows_reflexion_upgrade(
        1,
        3,
        FailureClass::Resource
    ));
}

#[test]
fn failure_classification_distinguishes_quality_from_transport() {
    assert_eq!(
        classify_failure("No response from LLM after tool execution"),
        FailureClass::Quality
    );
    assert_eq!(
        classify_failure("Provider API timeout while waiting for upstream"),
        FailureClass::Transport
    );
    assert_eq!(
        classify_failure("file does not exist or cannot be opened"),
        FailureClass::Execution
    );
}

#[test]
fn task_complexity_sanitization_clamps_and_defaults_fields() {
    let sanitized = sanitize_task_complexity(TaskComplexity {
        estimated_steps: 0,
        risk_score: 4.2,
        rationale: "test".to_string(),
        max_steps_override: Some(999),
        intent: "".to_string(),
    });

    assert_eq!(sanitized.estimated_steps, 1);
    assert_eq!(sanitized.risk_score, 1.0);
    assert_eq!(sanitized.max_steps_override, Some(200));
    assert_eq!(sanitized.intent, "general_query");
}

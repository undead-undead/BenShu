use crate::{
    classify_query_verification_plan_with_request, CapabilityRouteRequest, QueryVerificationPlan,
    SourcePosture, TruthStatus, VerificationDomain, VerificationFollowupPlan, VerificationMode,
    VerificationOutcome, VerificationRequirement, VerificationResultEnvelope, VerificationSource,
    WebVerificationOrchestrator, WebVerificationTermination,
};
use std::collections::BTreeMap;

const TRUTH_VERIFICATION_GUIDANCE_PROMPT: &str = concat!(
    "### TRUTH AND VERIFICATION CONTRACT\n",
    "Treat truthfulness as a hard runtime constraint, not a stylistic preference.\n",
    "- Never present unverified claims as confirmed facts.\n",
    "- Never present a plan, guess, or likely state as an observed result.\n",
    "- For latest/current/external factual claims, tool availability, execution results, runtime readiness, and high-risk advice, prefer verification before confirmation.\n",
    "- If only search results were observed, explicitly state that source pages still need to be fetched before treating the answer as confirmed.\n",
    "- If the answer is based only on local context, mark it as unverified or inferred instead of pretending it was verified.\n",
    "- If sources or execution evidence are missing, say so plainly and keep the answer conservative.\n"
);

const EXPLICIT_SOURCE_REQUEST_MARKERS: &[&str] = &[
    "给来源",
    "给个来源",
    "给我来源",
    "给链接",
    "给个链接",
    "附来源",
    "附上来源",
    "带来源",
    "带上来源",
    "引用来源",
    "出处",
    "source",
    "sources",
    "citation",
    "citations",
    "cite",
    "cited",
    "link",
    "links",
    "with source",
    "with sources",
    "show source",
    "show sources",
    "show me the source",
    "give me the source",
    "give me sources",
    "give me links",
];

#[derive(Debug, Clone, Copy, Default)]
pub struct TruthVerificationPolicyEngine {
    request: CapabilityRouteRequest,
}

impl TruthVerificationPolicyEngine {
    pub fn new(request: CapabilityRouteRequest) -> Self {
        Self { request }
    }

    pub fn classify_query(&self, query: &str) -> Option<QueryVerificationPlan> {
        classify_query_verification_plan_with_request(query, self.request)
    }

    pub fn query_requests_explicit_sources(&self, query: &str) -> bool {
        EXPLICIT_SOURCE_REQUEST_MARKERS
            .iter()
            .any(|marker| query.contains(marker))
    }

    pub fn guidance_prompt(&self) -> &'static str {
        TRUTH_VERIFICATION_GUIDANCE_PROMPT
    }

    pub fn should_include_guidance_for_query(&self, query: &str) -> bool {
        if self.query_requests_explicit_sources(query) {
            return true;
        }

        self.classify_query(query).is_some_and(|plan| {
            plan.requirement == VerificationRequirement::Required
                || !matches!(
                    plan.mode,
                    VerificationMode::None | VerificationMode::LocalContextOnly
                )
        })
    }

    pub fn build_local_context_only_notice(&self, source_required: bool) -> String {
        if source_required {
            "Verification notice: this answer is based on local context only, has not been independently verified, and the requested source or link is still missing. Treat the answer below as tentative until you verify it and attach a source.".to_string()
        } else {
            "Verification notice: this answer is based on local context only and has not been independently verified. Treat the answer below as tentative unless you explicitly verify it.".to_string()
        }
    }

    pub fn build_requested_source_missing_notice(&self) -> String {
        "Verification notice: the user explicitly requested a source or link, but a source-backed answer has not been fully established yet. Treat the answer below as tentative until you fetch or attach a supporting source.".to_string()
    }

    pub fn build_evidence_attachment_appendix(
        &self,
        latest: &BTreeMap<String, String>,
        followup: Option<&BTreeMap<String, String>>,
        sources: &[VerificationSource],
        execution_evidence: &[String],
        state_evidence: &[String],
    ) -> Option<String> {
        let cite_required = followup
            .and_then(|fields| fields.get("cite_required"))
            .is_some_and(|value| value == "true");
        let orchestrator = WebVerificationOrchestrator::new();
        let followup_plan = followup.map(followup_plan_from_fields);
        let decision = orchestrator.decide(
            None,
            Some(&verification_result_from_latest(
                latest,
                sources,
                execution_evidence,
                state_evidence,
            )),
            followup_plan.as_ref(),
        );

        let mut sections = Vec::new();

        if matches!(
            decision.termination,
            WebVerificationTermination::FinalizeWithSources
        ) && cite_required
        {
            let lines = sources
                .iter()
                .filter_map(|source| {
                    let title = source.title.trim();
                    let uri = source.uri.trim();
                    if title.is_empty() || uri.is_empty() {
                        return None;
                    }
                    Some(format!("- {title}: {uri}"))
                })
                .collect::<Vec<_>>();
            if !lines.is_empty() {
                sections.push(format!("Sources:\n{}", lines.join("\n")));
            }
        }

        if matches!(
            decision.termination,
            WebVerificationTermination::FinalizeWithExecutionEvidence
        ) && !execution_evidence.is_empty()
        {
            sections.push(format!(
                "Execution Evidence:\n{}",
                execution_evidence
                    .iter()
                    .map(|item| format!("- {}", item.trim()))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        if matches!(
            decision.termination,
            WebVerificationTermination::FinalizeWithStateEvidence
        ) && !state_evidence.is_empty()
        {
            sections.push(format!(
                "State Evidence:\n{}",
                state_evidence
                    .iter()
                    .map(|item| format!("- {}", item.trim()))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        if sections.is_empty() {
            return None;
        }

        Some(sections.join("\n\n"))
    }

    pub fn explicit_source_request_still_missing(
        &self,
        user_input: Option<&str>,
        latest: &BTreeMap<String, String>,
        followup: Option<&BTreeMap<String, String>>,
    ) -> bool {
        if !user_input.is_some_and(|query| self.query_requests_explicit_sources(query)) {
            return false;
        }

        if latest.get("domain").map(String::as_str) != Some("KnowledgeFact") {
            return false;
        }

        let result = verification_result_from_latest(latest, &[], &[], &[]);
        let followup_plan = followup.map(followup_plan_from_fields);
        let decision =
            WebVerificationOrchestrator::new().decide(None, Some(&result), followup_plan.as_ref());

        matches!(
            decision.termination,
            WebVerificationTermination::TentativeOnly | WebVerificationTermination::NotReady
        )
    }

    pub fn build_downgrade_notice(
        &self,
        latest: &BTreeMap<String, String>,
        followup: Option<&BTreeMap<String, String>>,
    ) -> Option<String> {
        self.build_downgrade_notice_for_language(latest, followup, false)
    }

    pub fn build_downgrade_notice_for_language(
        &self,
        latest: &BTreeMap<String, String>,
        followup: Option<&BTreeMap<String, String>>,
        prefers_chinese: bool,
    ) -> Option<String> {
        let truth_status = latest.get("truth_status").map(String::as_str);
        let source_posture = latest.get("source_posture").map(String::as_str);
        let outcome = latest.get("outcome").map(String::as_str);

        let mut reasons = Vec::new();

        match (prefers_chinese, truth_status) {
            (true, Some("Unverified")) => reasons.push("答案仍未完成验证"),
            (true, Some("Inferred")) => reasons.push("部分内容是推断结果，并非直接验证结果"),
            (true, Some("Uncertain")) => reasons.push("答案仍存在不确定性"),
            (true, Some("ClarificationRequired")) => {
                reasons.push("请求还需要进一步澄清后才能作为事实回答")
            }
            (false, Some("Unverified")) => reasons.push("the answer is still unverified"),
            (false, Some("Inferred")) => {
                reasons.push("part of the answer is inferred rather than directly verified")
            }
            (false, Some("Uncertain")) => reasons.push("the answer remains uncertain"),
            (false, Some("ClarificationRequired")) => reasons
                .push("the request still needs clarification before it can be stated as fact"),
            _ => {}
        }

        match (prefers_chinese, source_posture) {
            (true, Some("SourcesRequiredButMissing")) => reasons.push("缺少必要的支持来源"),
            (true, Some("SourcesReferencedButNotAttached")) => {
                reasons.push("提到了来源，但没有实际附上可验证来源")
            }
            (_, Some("ExecutionEvidenceAttached") | Some("StateEvidenceAttached")) => {}
            (false, Some("SourcesRequiredButMissing")) => {
                reasons.push("required supporting sources are missing")
            }
            (false, Some("SourcesReferencedButNotAttached")) => {
                reasons.push("sources were referenced but not actually attached")
            }
            _ => {}
        }

        match (prefers_chinese, outcome) {
            (true, Some("VerificationExecutionMissing")) => {
                reasons.push("运行时只规划了验证步骤，但没有观察到完成结果")
            }
            (true, Some("VerificationStateMissing")) => {
                reasons.push("没有实际观察到所需运行时状态")
            }
            (true, Some("VerificationToolUnavailable")) => reasons.push("所需验证工具当前不可用"),
            (true, Some("VerificationFetchFailed")) => reasons.push("验证抓取在收集证据前失败"),
            (true, Some("VerificationSourceInsufficient")) => {
                reasons.push("已收集来源不足以支撑高置信回答")
            }
            (true, Some("VerificationSkippedByPolicyGap")) => {
                reasons.push("运行时缺少必要验证路径，验证被跳过")
            }
            (false, Some("VerificationExecutionMissing")) => reasons
                .push("the runtime only planned execution and did not observe a completed result"),
            (false, Some("VerificationStateMissing")) => {
                reasons.push("the required runtime state was not actually observed")
            }
            (false, Some("VerificationToolUnavailable")) => {
                reasons.push("the required verification tool was not available")
            }
            (false, Some("VerificationFetchFailed")) => {
                reasons.push("verification fetch failed before evidence was collected")
            }
            (false, Some("VerificationSourceInsufficient")) => reasons
                .push("the collected sources were insufficient to support a confident answer"),
            (false, Some("VerificationSkippedByPolicyGap")) => reasons.push(
                "verification was skipped because the runtime lacks a required verification path",
            ),
            _ => {}
        }

        if let Some(followup) = followup {
            let followup_plan = followup_plan_from_fields(followup);
            let decision = WebVerificationOrchestrator::new().decide(
                None,
                Some(&verification_result_from_latest(latest, &[], &[], &[])),
                Some(&followup_plan),
            );
            if decision.requires_followup {
                reasons.push(if prefers_chinese {
                    "仍有后续验证步骤未完成"
                } else {
                    "verification follow-up is still pending"
                });
            }
            if matches!(
                decision.termination,
                WebVerificationTermination::TentativeOnly
            ) && followup
                .get("answer_readiness")
                .is_some_and(|value| value == "search_results_only")
            {
                reasons.push(if prefers_chinese {
                    "只观察到了搜索结果，还没有抓取来源页面"
                } else {
                    "search results were observed, but source pages were not fetched yet"
                });
            }
            if decision.cite_required && !matches!(source_posture, Some("SourcesAttached")) {
                reasons.push(if prefers_chinese {
                    "仍需要引用来源，才能把答案视为已确认"
                } else {
                    "citation is still required before the answer can be treated as confirmed"
                });
            }
        }

        if reasons.is_empty() {
            return None;
        }

        if prefers_chinese {
            Some(format!(
                "验证提示：{}。在完成验证前，请把下面的回答视为暂定结果。",
                reasons.join("；")
            ))
        } else {
            Some(format!(
                "Verification notice: {}. Treat the answer below as tentative until verification is completed.",
                reasons.join("; ")
            ))
        }
    }
}

fn followup_plan_from_fields(fields: &BTreeMap<String, String>) -> VerificationFollowupPlan {
    VerificationFollowupPlan {
        answer_readiness: fields
            .get("answer_readiness")
            .cloned()
            .unwrap_or_else(|| "verification_pending".to_string()),
        next_tools: fields
            .get("next_tools")
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        cite_required: fields
            .get("cite_required")
            .is_some_and(|value| value == "true"),
        note: fields.get("note").cloned().unwrap_or_default(),
    }
}

fn verification_result_from_latest(
    latest: &BTreeMap<String, String>,
    sources: &[VerificationSource],
    execution_evidence: &[String],
    state_evidence: &[String],
) -> VerificationResultEnvelope {
    fn parse_domain(value: Option<&String>) -> VerificationDomain {
        match value.map(String::as_str) {
            Some("ToolFact") => VerificationDomain::ToolFact,
            Some("ExecutionFact") => VerificationDomain::ExecutionFact,
            Some("StateFact") => VerificationDomain::StateFact,
            _ => VerificationDomain::KnowledgeFact,
        }
    }

    fn parse_requirement(value: Option<&String>) -> VerificationRequirement {
        match value.map(String::as_str) {
            Some("Recommended") => VerificationRequirement::Recommended,
            Some("LocalContextAllowed") => VerificationRequirement::LocalContextAllowed,
            _ => VerificationRequirement::Required,
        }
    }

    fn parse_mode(value: Option<&String>) -> VerificationMode {
        match value.map(String::as_str) {
            Some("LocalContextOnly") => VerificationMode::LocalContextOnly,
            Some("ToolInventoryCheck") => VerificationMode::ToolInventoryCheck,
            Some("RuntimeStateCheck") => VerificationMode::RuntimeStateCheck,
            Some("ExecutionResultCheck") => VerificationMode::ExecutionResultCheck,
            Some("ToolLookup") => VerificationMode::ToolLookup,
            Some("BrowserValidation") => VerificationMode::BrowserValidation,
            Some("RealtimeLookup") => VerificationMode::RealtimeLookup,
            Some("None") => VerificationMode::None,
            _ => VerificationMode::WebSearchFetch,
        }
    }

    fn parse_outcome(value: Option<&String>) -> VerificationOutcome {
        match value.map(String::as_str) {
            Some("VerificationNotRequired") => VerificationOutcome::VerificationNotRequired,
            Some("VerificationToolUnavailable") => VerificationOutcome::VerificationToolUnavailable,
            Some("VerificationFetchFailed") => VerificationOutcome::VerificationFetchFailed,
            Some("VerificationSourceInsufficient") => {
                VerificationOutcome::VerificationSourceInsufficient
            }
            Some("VerificationExecutionMissing") => {
                VerificationOutcome::VerificationExecutionMissing
            }
            Some("VerificationStateMissing") => VerificationOutcome::VerificationStateMissing,
            Some("VerificationSkippedByPolicyGap") => {
                VerificationOutcome::VerificationSkippedByPolicyGap
            }
            _ => VerificationOutcome::VerificationSucceeded,
        }
    }

    fn parse_truth_status(value: Option<&String>) -> TruthStatus {
        match value.map(String::as_str) {
            Some("Unverified") => TruthStatus::Unverified,
            Some("Inferred") => TruthStatus::Inferred,
            Some("Uncertain") => TruthStatus::Uncertain,
            Some("ClarificationRequired") => TruthStatus::ClarificationRequired,
            _ => TruthStatus::Verified,
        }
    }

    fn parse_source_posture(value: Option<&String>) -> SourcePosture {
        match value.map(String::as_str) {
            Some("SourcesReferencedButNotAttached") => {
                SourcePosture::SourcesReferencedButNotAttached
            }
            Some("ExecutionEvidenceAttached") => SourcePosture::ExecutionEvidenceAttached,
            Some("StateEvidenceAttached") => SourcePosture::StateEvidenceAttached,
            Some("NoSourcesRequired") => SourcePosture::NoSourcesRequired,
            Some("SourcesRequiredButMissing") => SourcePosture::SourcesRequiredButMissing,
            _ => SourcePosture::SourcesAttached,
        }
    }

    VerificationResultEnvelope {
        domain: parse_domain(latest.get("domain")),
        requirement: parse_requirement(latest.get("requirement")),
        mode: parse_mode(latest.get("mode")),
        outcome: parse_outcome(latest.get("outcome")),
        truth_status: parse_truth_status(latest.get("truth_status")),
        source_posture: parse_source_posture(latest.get("source_posture")),
        sources: sources.to_vec(),
        execution_evidence: execution_evidence.to_vec(),
        state_evidence: state_evidence.to_vec(),
        notes: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_engine_marks_requested_source_missing_for_search_results_only() {
        let engine = TruthVerificationPolicyEngine::default();
        let latest = BTreeMap::from([
            ("domain".to_string(), "KnowledgeFact".to_string()),
            ("source_posture".to_string(), "SourcesAttached".to_string()),
        ]);
        let followup = BTreeMap::from([(
            "answer_readiness".to_string(),
            "search_results_only".to_string(),
        )]);

        assert!(engine.explicit_source_request_still_missing(
            Some("给我来源和链接"),
            &latest,
            Some(&followup),
        ));
    }

    #[test]
    fn policy_engine_accepts_source_observed_answers() {
        let engine = TruthVerificationPolicyEngine::default();
        let latest = BTreeMap::from([
            ("domain".to_string(), "KnowledgeFact".to_string()),
            ("source_posture".to_string(), "SourcesAttached".to_string()),
        ]);
        let followup = BTreeMap::from([(
            "answer_readiness".to_string(),
            "source_content_observed".to_string(),
        )]);

        assert!(!engine.explicit_source_request_still_missing(
            Some("给我来源和链接"),
            &latest,
            Some(&followup),
        ));
    }

    #[test]
    fn policy_engine_builds_source_attachment_appendix() {
        let engine = TruthVerificationPolicyEngine::default();
        let latest =
            BTreeMap::from([("source_posture".to_string(), "SourcesAttached".to_string())]);
        let followup = BTreeMap::from([
            (
                "answer_readiness".to_string(),
                "source_content_observed".to_string(),
            ),
            ("cite_required".to_string(), "true".to_string()),
        ]);
        let sources = vec![VerificationSource {
            kind: "web_page".to_string(),
            title: "OpenAI Pricing".to_string(),
            uri: "https://openai.com/api/pricing".to_string(),
            observed_at: None,
        }];

        let appendix = engine
            .build_evidence_attachment_appendix(&latest, Some(&followup), &sources, &[], &[])
            .expect("source appendix should be emitted");

        assert!(appendix.contains("Sources:"));
        assert!(appendix.contains("OpenAI Pricing"));
        assert!(appendix.contains("https://openai.com/api/pricing"));
    }

    #[test]
    fn policy_engine_builds_execution_state_evidence_appendix() {
        let engine = TruthVerificationPolicyEngine::default();
        let followup = BTreeMap::from([(
            "answer_readiness".to_string(),
            "execution_or_state_observed".to_string(),
        )]);

        let execution_appendix = engine
            .build_evidence_attachment_appendix(
                &BTreeMap::from([(
                    "source_posture".to_string(),
                    "ExecutionEvidenceAttached".to_string(),
                )]),
                Some(&followup),
                &[],
                &["command=git status exit=0".to_string()],
                &[],
            )
            .expect("execution evidence appendix should be emitted");
        assert!(execution_appendix.contains("Execution Evidence:"));
        assert!(execution_appendix.contains("command=git status exit=0"));

        let state_appendix = engine
            .build_evidence_attachment_appendix(
                &BTreeMap::from([(
                    "source_posture".to_string(),
                    "StateEvidenceAttached".to_string(),
                )]),
                Some(&followup),
                &[],
                &[],
                &["runtime=quickjs available=true".to_string()],
            )
            .expect("state evidence appendix should be emitted");
        assert!(state_appendix.contains("State Evidence:"));
        assert!(state_appendix.contains("runtime=quickjs available=true"));
    }

    #[test]
    fn policy_engine_skips_guidance_for_local_only_explanatory_queries() {
        let engine = TruthVerificationPolicyEngine::default();

        assert!(!engine
            .should_include_guidance_for_query("解释一下 background envelope 和 history 的区别"));
    }

    #[test]
    fn policy_engine_enables_guidance_for_sources_and_runtime_checks() {
        let engine = TruthVerificationPolicyEngine::default();

        assert!(engine.should_include_guidance_for_query("给我来源和链接"));
        assert!(engine.should_include_guidance_for_query("检查 powershell 现在能不能用"));
    }
}

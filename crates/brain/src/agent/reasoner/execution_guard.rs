use std::time::Duration;

use crate::agent::message::Message;
use crate::agent::protocol::{AgentEventData, AgentLiaison, SafetyLevel};
use crate::agent::provider::Provider;
use crate::agent::tactical::{ProposedAction, TacticalVerdict};
use crate::error::{Error, Result};
use tracing::{debug, error, info, warn};

use super::{reasoner_constants, Reasoner};

pub(super) enum GuardDecision {
    Proceed,
    ContinueLoop,
}

impl<P: Provider> Reasoner<P> {
    pub(super) async fn run_tactical_precheck(
        &self,
        bridge: &dyn AgentLiaison,
        messages: &mut Vec<Message>,
        tool_calls: &[(String, String, serde_json::Value)],
    ) -> Result<GuardDecision> {
        if tool_calls.is_empty() {
            return Ok(GuardDecision::Proceed);
        }

        if self.tactical_orchestrator.is_active() {
            bridge.emit(AgentEventData::Thought {
                content: "TACTICAL ORCHESTRATOR: Engaging SLM for System 2 reflection..."
                    .to_string(),
            });
        }

        let actions: Vec<ProposedAction> = tool_calls
            .iter()
            .map(|(id, name, args)| ProposedAction {
                id: id.clone(),
                name: name.clone(),
                args: args.clone(),
            })
            .collect();

        match self
            .tactical_orchestrator
            .derive_tactics(messages, &actions)
            .await?
        {
            TacticalVerdict::Proceed => {
                debug!("TacticalOrchestrator: Plan approved.");
                Ok(GuardDecision::Proceed)
            }
            TacticalVerdict::Pivot(advice) => {
                info!("TacticalOrchestrator: PIVOT suggested: {}", advice);
                bridge.emit(AgentEventData::Thought {
                    content: format!("TACTICAL PIVOT: {}", advice),
                });
                messages.push(Message::system(format!(
                    "{}\n\
                     Your current plan has been analyzed by a tactical sub-processor. It suggests a PIVOT:\n\n\
                     {}\n\n\
                     Adjust your strategy accordingly.",
                    reasoner_constants::MARKER_TACTICAL_PIVOT,
                    advice
                )));
                Ok(GuardDecision::ContinueLoop)
            }
            TacticalVerdict::Halt(reason) => {
                warn!("TacticalOrchestrator: HALT triggered: {}", reason);
                bridge.emit(AgentEventData::Thought {
                    content: format!("TACTICAL HALT: {}", reason),
                });
                bridge.emit(AgentEventData::Error {
                    message: format!("Tactical Halt: {}", reason),
                });
                Err(Error::agent_config(format!(
                    "Tactical Orchestrator halted task: {}",
                    reason
                )))
            }
        }
    }

    pub(super) async fn run_red_team_audit_if_needed(
        &self,
        bridge: &dyn AgentLiaison,
        messages: &mut Vec<Message>,
        thoughts_snapshot: &[String],
        tool_calls: &[(String, String, serde_json::Value)],
        risk_score: f32,
        max_steps: usize,
        audit_rejections: &mut usize,
    ) -> Result<GuardDecision> {
        if tool_calls.is_empty() {
            return Ok(GuardDecision::Proceed);
        }

        let risk_level = self.calculate_plan_risk(risk_score, tool_calls).await;
        let has_red_tool = self.has_red_safety_tool(tool_calls).await;

        if risk_level < crate::agent::protocol::constants::HIGH_RISK_THRESHOLD && !has_red_tool {
            return Ok(GuardDecision::Proceed);
        }

        bridge.emit(AgentEventData::Thought {
            content: "RED TEAM AUDIT: Plan risk evaluated - launching verification flow..."
                .to_string(),
        });

        let mut audit_result = None;
        let mut audit_attempts = 0;

        while audit_attempts < reasoner_constants::AUDIT_MAX_RETRIES {
            match self.audit_plan(thoughts_snapshot, tool_calls).await {
                Ok(res) => {
                    audit_result = res;
                    break;
                }
                Err(e) => {
                    audit_attempts += 1;
                    warn!("Audit attempt {} failed: {}", audit_attempts, e);
                    if audit_attempts >= reasoner_constants::AUDIT_MAX_RETRIES {
                        warn!(
                            "Audit failed after max retries - proceeding with caution as fallback."
                        );
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(
                        reasoner_constants::AUDIT_RETRY_BACKOFF_MS,
                    ))
                    .await;
                }
            }
        }

        let Some(rejection) = audit_result else {
            debug!("Red Team Audit: PASSED or fallback - proceeding to execution");
            return Ok(GuardDecision::Proceed);
        };

        warn!("Red Team Audit: PLAN REJECTED. Reason: {}", rejection);
        bridge.emit(AgentEventData::Thought {
            content: format!("AUDIT REJECTED: {}", rejection),
        });

        *audit_rejections += 1;
        let max_retries = (max_steps as f32 * reasoner_constants::MAX_AUDIT_RETRY_RATIO) as usize;
        if *audit_rejections > max_retries {
            error!(
                "Audit rejection limit reached ({} calls). Halting task to prevent infinite loop.",
                *audit_rejections
            );
            return Err(Error::agent_config(format!(
                "Too many security audit rejections ({}). Plan is fundamentally unsafe.",
                *audit_rejections
            )));
        }

        messages.push(Message::system(format!(
            "{}\n\
             Your plan was REJECTED by the security auditor for these reasons:\n\n\
             {}\n\n\
             Re-evaluate and propose a safer path that addresses these concerns.",
            reasoner_constants::MARKER_SECURITY_REJECTION,
            rejection
        )));
        Ok(GuardDecision::ContinueLoop)
    }

    async fn calculate_plan_risk(
        &self,
        base_risk_score: f32,
        tool_calls: &[(String, String, serde_json::Value)],
    ) -> f32 {
        let mut total_risk = base_risk_score.max(0.1);
        for (_, name, _) in tool_calls {
            if let Some(tool) = self.tools.get(name) {
                total_risk += match tool.definition().await.safety_level {
                    SafetyLevel::Red => 0.4,
                    SafetyLevel::Yellow => 0.15,
                    SafetyLevel::Green => 0.02,
                };
            }
        }
        total_risk.min(1.0)
    }

    async fn has_red_safety_tool(
        &self,
        tool_calls: &[(String, String, serde_json::Value)],
    ) -> bool {
        for (_, name, _) in tool_calls {
            if let Some(tool) = self.tools.get(name) {
                if tool.definition().await.safety_level == SafetyLevel::Red {
                    return true;
                }
            }
        }
        false
    }

    async fn audit_plan(
        &self,
        thoughts: &[String],
        tool_calls: &[(String, String, serde_json::Value)],
    ) -> Result<Option<String>> {
        let mut plan = String::from("### PLAN TO AUDIT\n\n");
        for thought in thoughts.iter().take(4) {
            plan.push_str(&format!("- Thought: {}\n", thought.trim()));
        }
        for (id, name, args) in tool_calls {
            plan.push_str(&format!(
                "- ID: {}\n Tool: {}\n Args: {}\n",
                id,
                name,
                self.sanitize_arguments(name, args)
            ));
        }

        let request = crate::agent::provider::ChatRequest {
            model: self.config.model.clone(),
            system_prompt: Some(
                "You are a strict security auditor. Respond [REJECTED] <reason> or [APPROVED]."
                    .to_string(),
            ),
            messages: vec![Message::user(format!("Audit this plan:\n{}", plan))],
            temperature: Some(0.0),
            max_tokens: Some(256),
            session_id: self.auxiliary_session_id("audit"),
            ..Default::default()
        };

        let stream = tokio::time::timeout(
            Duration::from_secs(10),
            self.provider.stream_completion(request),
        )
        .await
        .map_err(|_| Error::Internal("Audit timeout".to_string()))??;

        let res = stream.collect_text().await.unwrap_or_default();
        let res_upper = res.to_uppercase();

        if res_upper.contains("[REJECTED]") {
            Ok(Some(res.replace("[REJECTED]", "").trim().to_string()))
        } else if res_upper.contains("REJECTED") {
            Ok(Some(res.replace("REJECTED", "").trim().to_string()))
        } else if res_upper.contains("REJECT") && !res_upper.contains("APPROVE") {
            Ok(Some(res.replace("REJECT", "").trim().to_string()))
        } else if res_upper.contains("[APPROVED]")
            || res_upper.contains("APPROVED")
            || res_upper.contains("APPROVE")
        {
            Ok(None)
        } else {
            warn!("Reasoner: Audit result unparseable: '{}'", res);
            Ok(None)
        }
    }

    fn sanitize_arguments(&self, tool_name: &str, args: &serde_json::Value) -> String {
        let sensitive = ["secret", "password", "token", "key", "credential"];
        let mut sanitized = args.clone();
        if let serde_json::Value::Object(ref mut map) = sanitized {
            for key in sensitive {
                if map.contains_key(key) {
                    map.insert(
                        key.to_string(),
                        serde_json::Value::String("[REDACTED]".to_string()),
                    );
                }
            }
        }
        if ["ssh_exec", "vault_manager"].contains(&tool_name) {
            return "[HIGH RISK TOOL SANITIZED]".to_string();
        }
        let rendered = sanitized.to_string();
        if rendered.len() > 150 {
            format!("{}... [TRUNCATED]", &rendered[0..150])
        } else {
            rendered
        }
    }
}

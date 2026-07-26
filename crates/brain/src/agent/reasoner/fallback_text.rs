use benshu_hardness::FinalizationFallbackKind;

use crate::skills::tool::CapabilityRouteHint;

pub(crate) fn tool_failure_delivery_text(
    tool_name: &str,
    compact_error: &str,
    prefers_chinese: bool,
    query_requests_knowledge_persistence: bool,
) -> String {
    if prefers_chinese {
        let persistence_note = if query_requests_knowledge_persistence {
            "\n\n因为没有拿到可靠、可验证的来源内容，我没有把这次失败结果写入知识库。"
        } else {
            ""
        };
        format!(
            "这次任务没有稳定完成：工具 `{}` 执行失败，系统已停止继续空转。\n\n当前具体卡点：{}{}",
            tool_name, compact_error, persistence_note
        )
    } else {
        let persistence_note = if query_requests_knowledge_persistence {
            "\n\nBecause no reliable, verifiable source content was obtained, I did not save this failed result into the knowledge base."
        } else {
            ""
        };
        format!(
            "This task did not complete reliably: tool `{}` failed, so the system stopped instead of continuing to loop.\n\nCurrent blocker: {}{}",
            tool_name, compact_error, persistence_note
        )
    }
}

pub(crate) fn routing_judgment_fallback_text(route: Option<CapabilityRouteHint>) -> String {
    match route.unwrap_or(CapabilityRouteHint::General) {
        CapabilityRouteHint::RuntimeSurface | CapabilityRouteHint::ExternalCliTools => {
            "如果需要执行 Windows 命令，我会先把任务交给 terminal / runtime-surface specialist，由 BenShu 保持前台协调，不会自己直接执行。".to_string()
        }
        CapabilityRouteHint::FileOps => {
            "如果这轮主要是文件操作，我会优先交给 file-ops / repo specialist，由 BenShu 负责前台协调和结果整合。".to_string()
        }
        CapabilityRouteHint::Coding => {
            "如果这是编码实现类请求，我会优先交给 coder specialist，由 BenShu 负责前台协调和结果整合。".to_string()
        }
        CapabilityRouteHint::Writing => {
            "如果这是写作或长文产物请求，我会优先交给 writer specialist，由 BenShu 负责前台协调、连续性约束和结果整合。".to_string()
        }
        CapabilityRouteHint::DocumentUnderstanding => {
            "如果这是文档理解类请求，我会优先交给 document / pdf / ocr specialist，由 BenShu 保持前台协调。".to_string()
        }
        CapabilityRouteHint::VisualUnderstanding => {
            "如果这是图片理解类请求，我会优先使用现有多模态承接；如果本地多模态不可用，再降级到 document / ocr / image specialist，由 BenShu 保持前台协调。".to_string()
        }
        CapabilityRouteHint::VoiceUnderstanding => {
            "如果这是语音理解类请求，我会优先交给 voice-understanding specialist，由 BenShu 负责前台协调。".to_string()
        }
        CapabilityRouteHint::Communication => {
            "如果这是对外沟通类请求，我会优先交给 communication specialist，由 BenShu 负责前台协调和结果整合。".to_string()
        }
        CapabilityRouteHint::Memory => {
            "如果这轮主要是记忆查询或知识回溯，我会优先走 memory / knowledge 路径，由 BenShu 负责前台整理。".to_string()
        }
        CapabilityRouteHint::RealtimeLookup(_) => {
            "如果这轮需要最新外部信息，我会先走 realtime lookup 路径，再由 BenShu 整合结果。".to_string()
        }
        CapabilityRouteHint::CapabilityGap | CapabilityRouteHint::General => {
            "如果暂时没有明显的单一执行面，我会先保持 BenShu 的前台协调姿态，再选择最窄的 specialist 或执行路径。".to_string()
        }
    }
}

pub(crate) fn image_generation_unavailable_fallback_text(query: &str) -> String {
    let prefers_chinese = query.chars().any(|ch| {
        ('\u{4E00}'..='\u{9FFF}').contains(&ch)
            || ('\u{3400}'..='\u{4DBF}').contains(&ch)
            || ('\u{F900}'..='\u{FAFF}').contains(&ch)
    });

    if prefers_chinese {
        "当前没有可用的图片生成模型支持，所以这轮我不能直接帮你生成图片。我现在只能做图像理解；如果要启用画图，请先配置并接通真实的 image generation backend（例如 `sensory.image_gen_model`）。".to_string()
    } else {
        "Image generation is unavailable in the current runtime, so I cannot create an image in this turn. I can handle image understanding, but to generate images you need to configure and expose a real image generation backend (for example `sensory.image_gen_model`).".to_string()
    }
}

pub(crate) fn classified_finalization_fallback_text(
    kind: FinalizationFallbackKind,
    prefers_chinese: bool,
) -> String {
    match kind {
        FinalizationFallbackKind::MediaUnderstandingRetryHint => {
            if prefers_chinese {
                "我看到了图片内容，但这轮的多模态交付没有稳定落成自然语言答案，请再试一次。"
                    .to_string()
            } else {
                "I could see the image content, but this multimodal turn did not settle into a stable natural-language answer. Please try again.".to_string()
            }
        }
        FinalizationFallbackKind::QualityNoAnswer => {
            if prefers_chinese {
                "这轮已经进入最终交付阶段，但模型没有稳定产出可直接展示的答案。我可以继续重试，或把已观察到的工具结果直接整理给你。".to_string()
            } else {
                "This turn reached final delivery, but the model did not produce a stable user-facing answer. I can retry, or I can summarize the tool results that were already observed.".to_string()
            }
        }
        FinalizationFallbackKind::TransportUnavailable => {
            if prefers_chinese {
                "这轮没有稳定完成交付，主要像是模型服务连接或超时问题，不是你的请求本身有问题。可以稍后重试。".to_string()
            } else {
                "This turn did not finish cleanly because of a provider connectivity or timeout issue, not because your request was invalid. Please retry shortly.".to_string()
            }
        }
        FinalizationFallbackKind::ResourcePressure => {
            if prefers_chinese {
                "这轮没有稳定完成交付，主要像是本地资源压力过高，例如显存或内存不足。可以降低负载后再试。".to_string()
            } else {
                "This turn did not finish cleanly because local resources were under pressure, such as VRAM or memory exhaustion. Please reduce load and try again.".to_string()
            }
        }
        FinalizationFallbackKind::ExecutionFailure => {
            if prefers_chinese {
                "这轮没有稳定完成交付，主要是执行面失败，例如文件、工具或运行环境不可用。不是因为需要进一步 Reflexion。".to_string()
            } else {
                "This turn did not finish cleanly because the execution layer failed, such as a file, tool, or runtime availability issue. This is not primarily a reflexion problem.".to_string()
            }
        }
        FinalizationFallbackKind::UnknownFailure => {
            if prefers_chinese {
                "这轮没有稳定完成交付，但当前还不能把失败原因精确分类。我建议直接重试，或查看运行时日志以获取更具体原因。".to_string()
            } else {
                "This turn did not finish cleanly, but the failure could not yet be classified precisely. Please retry, or inspect runtime logs for a more specific cause.".to_string()
            }
        }
    }
}

pub(crate) fn is_pseudo_tool_call_leak(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.starts_with("<|tool_call>")
        || trimmed.contains("<|tool_call>")
        || trimmed.contains("</tool_call>")
        || trimmed.contains("[Assistant tool request]")
        || trimmed.contains("[assistant tool request]")
}

pub(crate) fn is_multimodal_procedural_placeholder(text: &str) -> bool {
    let lowered = text.trim().to_lowercase();
    (lowered.contains("i will delegate")
        || lowered.contains("invoke the `document_understanding` tool")
        || lowered.contains("invoke the document_understanding tool")
        || lowered.contains("appropriate specialist"))
        && (lowered.contains("image") || lowered.contains("document_understanding"))
}

use crate::{
    capability_route_debug_label, capability_route_hint_label,
    capability_route_preferred_tool_names, capability_route_requires_real_tool_call,
    capability_route_requires_source_fetch, CapabilityClarificationHint, CapabilityRouteHint,
    CapabilityRouteRequest, QueryVerificationPlan, RealtimeLookupKind, VerificationDomain,
    VerificationMode, VerificationRequirement,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CapabilityRouter {
    request: CapabilityRouteRequest,
}

impl CapabilityRouter {
    pub fn new(request: CapabilityRouteRequest) -> Self {
        Self { request }
    }

    pub fn request(&self) -> CapabilityRouteRequest {
        self.request
    }

    pub fn classify_query_route(&self, query: &str) -> Option<CapabilityRouteHint> {
        if self.request.approved_forge_request {
            return None;
        }

        if self.request.has_media_input {
            return Some(CapabilityRouteHint::DocumentUnderstanding);
        }

        if self.request.force_document_understanding {
            return Some(CapabilityRouteHint::DocumentUnderstanding);
        }

        if self.request.runtime_surface_bias {
            return Some(CapabilityRouteHint::RuntimeSurface);
        }

        match classify_query_capability_route(query) {
            Some(CapabilityRouteHint::DocumentUnderstanding)
                if self.request.suppress_document_understanding =>
            {
                None
            }
            Some(CapabilityRouteHint::RealtimeLookup(_))
                if self.request.suppress_realtime_lookup =>
            {
                None
            }
            Some(
                route @ (CapabilityRouteHint::DocumentUnderstanding
                | CapabilityRouteHint::FileOps
                | CapabilityRouteHint::RealtimeLookup(_)
                | CapabilityRouteHint::RuntimeSurface
                | CapabilityRouteHint::ExternalCliTools
                | CapabilityRouteHint::Coding
                | CapabilityRouteHint::Communication
                | CapabilityRouteHint::Memory
                | CapabilityRouteHint::CapabilityGap),
            ) => Some(route),
            _ => None,
        }
    }

    pub fn preferred_capability_domain(&self, query: &str) -> Option<&'static str> {
        self.classify_query_route(query)
            .and_then(preferred_capability_domain_for_route)
    }

    pub fn clarification_hint(&self, query: &str) -> Option<CapabilityClarificationHint> {
        match self.classify_query_route(query) {
            Some(CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::PriceLookup)) => {
                if query_has_specific_price_target(query) {
                    None
                } else {
                    Some(CapabilityClarificationHint::MissingPriceTarget)
                }
            }
            Some(CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::FxLookup)) => {
                if count_currency_mentions_for_lookup(query) >= 2 {
                    None
                } else {
                    Some(CapabilityClarificationHint::MissingFxPair)
                }
            }
            Some(CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::WeatherLookup)) => {
                if query_has_weather_location_hint(query) {
                    None
                } else {
                    Some(CapabilityClarificationHint::MissingWeatherLocation)
                }
            }
            _ => None,
        }
    }

    pub fn route_label(&self, route: CapabilityRouteHint) -> &'static str {
        capability_route_hint_label(route)
    }

    pub fn route_debug_label(&self, route: CapabilityRouteHint) -> &'static str {
        capability_route_debug_label(route)
    }

    pub fn route_requires_real_tool_call(&self, route: CapabilityRouteHint) -> bool {
        capability_route_requires_real_tool_call(route)
    }

    pub fn preferred_tool_names(&self, route: CapabilityRouteHint) -> &'static [&'static str] {
        capability_route_preferred_tool_names(route)
    }

    pub fn route_requires_source_fetch(&self, route: CapabilityRouteHint) -> bool {
        capability_route_requires_source_fetch(route)
    }
}

pub fn classify_query_capability_domain(query: &str) -> Option<String> {
    let normalized_query = query.trim().to_lowercase();
    if normalized_query.is_empty() {
        return None;
    }
    let tokens = tokenize_query(&normalized_query);
    infer_query_capability_domain(&normalized_query, &tokens)
}

pub fn classify_query_capability_route(query: &str) -> Option<CapabilityRouteHint> {
    classify_query_capability_domain(query)
        .as_deref()
        .map(capability_domain_to_route_hint)
}

pub fn query_requests_routing_judgment_only(query: &str) -> bool {
    let normalized = query.trim().to_lowercase();
    if normalized.is_empty() {
        return false;
    }

    let routing_terms = [
        "路由",
        "调度",
        "交给谁",
        "给谁做",
        "谁来处理",
        "谁来执行",
        "route",
        "routing",
        "dispatch",
        "delegate",
        "who should handle",
        "who should do",
    ];
    let execution_suppression_terms = [
        "不要直接执行",
        "不要执行",
        "不要调用工具",
        "只说路由",
        "只做路由判断",
        "只做调度判断",
        "只做判断",
        "先不要执行",
        "do not execute",
        "don't execute",
        "no execution",
        "route only",
        "routing only",
        "just route",
        "only decide routing",
    ];

    routing_terms.iter().any(|term| normalized.contains(term))
        && execution_suppression_terms
            .iter()
            .any(|term| normalized.contains(term))
}

pub fn classify_query_verification_plan(query: &str) -> Option<QueryVerificationPlan> {
    classify_query_verification_plan_with_request(query, CapabilityRouteRequest::default())
}

pub fn classify_query_verification_plan_with_request(
    query: &str,
    request: CapabilityRouteRequest,
) -> Option<QueryVerificationPlan> {
    let normalized_query = query.trim().to_lowercase();
    if normalized_query.is_empty() {
        return None;
    }

    let route = resolve_capability_route(&normalized_query, request);
    if let Some(route) = route {
        let (domain, requirement, mode) = match route {
            CapabilityRouteHint::DocumentUnderstanding
            | CapabilityRouteHint::VisualUnderstanding
            | CapabilityRouteHint::VoiceUnderstanding => (
                VerificationDomain::KnowledgeFact,
                VerificationRequirement::Required,
                VerificationMode::ToolLookup,
            ),
            CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::WebSearch) => (
                VerificationDomain::KnowledgeFact,
                VerificationRequirement::Required,
                VerificationMode::WebSearchFetch,
            ),
            CapabilityRouteHint::RealtimeLookup(_) => (
                VerificationDomain::KnowledgeFact,
                VerificationRequirement::Required,
                VerificationMode::RealtimeLookup,
            ),
            CapabilityRouteHint::RuntimeSurface => {
                if query_requests_state_fact_verification(&normalized_query) {
                    (
                        VerificationDomain::StateFact,
                        VerificationRequirement::Required,
                        VerificationMode::RuntimeStateCheck,
                    )
                } else if query_requests_tool_fact_verification(&normalized_query) {
                    (
                        VerificationDomain::ToolFact,
                        VerificationRequirement::Required,
                        VerificationMode::ToolInventoryCheck,
                    )
                } else {
                    (
                        VerificationDomain::ExecutionFact,
                        VerificationRequirement::Required,
                        VerificationMode::ExecutionResultCheck,
                    )
                }
            }
            CapabilityRouteHint::Writing => (
                VerificationDomain::ExecutionFact,
                VerificationRequirement::Recommended,
                VerificationMode::ExecutionResultCheck,
            ),
            CapabilityRouteHint::ExternalCliTools => {
                if query_requests_execution_fact_verification(&normalized_query) {
                    (
                        VerificationDomain::ExecutionFact,
                        VerificationRequirement::Required,
                        VerificationMode::ExecutionResultCheck,
                    )
                } else if query_requests_tool_fact_verification(&normalized_query) {
                    (
                        VerificationDomain::ToolFact,
                        VerificationRequirement::Required,
                        VerificationMode::ToolInventoryCheck,
                    )
                } else if query_requests_state_fact_verification(&normalized_query) {
                    (
                        VerificationDomain::StateFact,
                        VerificationRequirement::Required,
                        VerificationMode::RuntimeStateCheck,
                    )
                } else {
                    (
                        VerificationDomain::ExecutionFact,
                        VerificationRequirement::Required,
                        VerificationMode::ExecutionResultCheck,
                    )
                }
            }
            _ => (
                VerificationDomain::KnowledgeFact,
                VerificationRequirement::Recommended,
                VerificationMode::LocalContextOnly,
            ),
        };

        return Some(QueryVerificationPlan {
            domain,
            requirement,
            mode,
            route_hint: Some(route),
        });
    }

    if query_requests_tool_fact_verification(&normalized_query) {
        return Some(QueryVerificationPlan {
            domain: VerificationDomain::ToolFact,
            requirement: VerificationRequirement::Recommended,
            mode: VerificationMode::ToolInventoryCheck,
            route_hint: None,
        });
    }

    if query_requests_state_fact_verification(&normalized_query) {
        return Some(QueryVerificationPlan {
            domain: VerificationDomain::StateFact,
            requirement: VerificationRequirement::Recommended,
            mode: VerificationMode::RuntimeStateCheck,
            route_hint: None,
        });
    }

    if query_requests_execution_fact_verification(&normalized_query) {
        return Some(QueryVerificationPlan {
            domain: VerificationDomain::ExecutionFact,
            requirement: VerificationRequirement::Recommended,
            mode: VerificationMode::ExecutionResultCheck,
            route_hint: None,
        });
    }

    if query_requests_high_risk_verification(&normalized_query) {
        return Some(QueryVerificationPlan {
            domain: VerificationDomain::KnowledgeFact,
            requirement: VerificationRequirement::Required,
            mode: VerificationMode::WebSearchFetch,
            route_hint: None,
        });
    }

    if looks_like_explanatory_query(&normalized_query) {
        return Some(QueryVerificationPlan {
            domain: VerificationDomain::KnowledgeFact,
            requirement: VerificationRequirement::LocalContextAllowed,
            mode: VerificationMode::LocalContextOnly,
            route_hint: None,
        });
    }

    None
}

pub fn resolve_capability_route(
    query: &str,
    request: CapabilityRouteRequest,
) -> Option<CapabilityRouteHint> {
    CapabilityRouter::new(request).classify_query_route(query)
}

pub fn preferred_capability_domain_for_route(route: CapabilityRouteHint) -> Option<&'static str> {
    match route {
        CapabilityRouteHint::DocumentUnderstanding => Some("document_understanding"),
        CapabilityRouteHint::VisualUnderstanding => Some("document_understanding"),
        CapabilityRouteHint::VoiceUnderstanding => Some("voice_understanding"),
        CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::WebSearch) => {
            Some("realtime_lookup.web")
        }
        CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::PriceLookup) => {
            Some("realtime_lookup.price")
        }
        CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::FxLookup) => {
            Some("realtime_lookup.fx")
        }
        CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::WeatherLookup) => {
            Some("realtime_lookup.weather")
        }
        CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::LatestInfoLookup) => {
            Some("realtime_lookup.latest_info")
        }
        CapabilityRouteHint::RuntimeSurface => Some("runtime_surface"),
        CapabilityRouteHint::ExternalCliTools => Some("external_cli_tools"),
        CapabilityRouteHint::FileOps => Some("file_ops"),
        CapabilityRouteHint::Writing => Some("writing"),
        CapabilityRouteHint::Coding => Some("coding"),
        CapabilityRouteHint::Communication => Some("communication"),
        CapabilityRouteHint::Memory => Some("memory"),
        CapabilityRouteHint::CapabilityGap => Some("capability_gap"),
        CapabilityRouteHint::General => None,
    }
}

fn capability_domain_to_route_hint(domain: &str) -> CapabilityRouteHint {
    match domain {
        "document_understanding" => CapabilityRouteHint::DocumentUnderstanding,
        "visual_understanding" => CapabilityRouteHint::DocumentUnderstanding,
        "voice_understanding" => CapabilityRouteHint::VoiceUnderstanding,
        "image_generation" => CapabilityRouteHint::CapabilityGap,
        "realtime_lookup.web" => CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::WebSearch),
        "realtime_lookup.price" => {
            CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::PriceLookup)
        }
        "realtime_lookup.fx" => CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::FxLookup),
        "realtime_lookup.weather" => {
            CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::WeatherLookup)
        }
        "realtime_lookup.latest_info" => {
            CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::LatestInfoLookup)
        }
        "runtime_surface" => CapabilityRouteHint::RuntimeSurface,
        "external_cli_tools" => CapabilityRouteHint::ExternalCliTools,
        "file_ops" => CapabilityRouteHint::FileOps,
        "writing" => CapabilityRouteHint::Writing,
        "coding" => CapabilityRouteHint::Coding,
        "communication" => CapabilityRouteHint::Communication,
        "memory" => CapabilityRouteHint::Memory,
        "capability_gap" => CapabilityRouteHint::CapabilityGap,
        _ => CapabilityRouteHint::General,
    }
}

fn tokenize_query(query: &str) -> Vec<String> {
    query
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .filter_map(|token| {
            let token = token.trim();
            if token.is_empty() {
                None
            } else {
                Some(token.to_lowercase())
            }
        })
        .collect()
}

fn known_currency_markers_for_lookup() -> &'static [(&'static str, &'static str)] {
    &[
        ("usd", "USD"),
        ("美元", "USD"),
        ("美金", "USD"),
        ("us dollar", "USD"),
        ("cny", "CNY"),
        ("rmb", "CNY"),
        ("人民币", "CNY"),
        ("yuan", "CNY"),
        ("eur", "EUR"),
        ("欧元", "EUR"),
        ("euro", "EUR"),
        ("jpy", "JPY"),
        ("日元", "JPY"),
        ("yen", "JPY"),
        ("hkd", "HKD"),
        ("港币", "HKD"),
        ("港元", "HKD"),
        ("gbp", "GBP"),
        ("英镑", "GBP"),
        ("pound", "GBP"),
        ("aud", "AUD"),
        ("澳元", "AUD"),
        ("cad", "CAD"),
        ("加元", "CAD"),
        ("sgd", "SGD"),
        ("新加坡元", "SGD"),
        ("krw", "KRW"),
        ("韩元", "KRW"),
        ("twd", "TWD"),
        ("台币", "TWD"),
        ("新台币", "TWD"),
    ]
}

fn extract_known_currency_codes_for_lookup(query: &str) -> Vec<&'static str> {
    let lowered = query.to_lowercase();
    let mut positions: Vec<(usize, &'static str)> = Vec::new();
    for (marker, code) in known_currency_markers_for_lookup() {
        let position = lowered.find(marker).or_else(|| query.find(marker));
        if let Some(idx) = position {
            positions.push((idx, *code));
        }
    }
    positions.sort_by_key(|(idx, _)| *idx);
    let mut seen = std::collections::BTreeSet::new();
    let mut ordered = Vec::new();
    for (_, code) in positions {
        if seen.insert(code) {
            ordered.push(code);
        }
    }
    ordered
}

fn count_currency_mentions_for_lookup(query: &str) -> usize {
    extract_known_currency_codes_for_lookup(query).len()
}

fn query_has_specific_price_target(query: &str) -> bool {
    let lowered = query.to_lowercase();
    let explicit_markers = [
        "btc",
        "bitcoin",
        "比特币",
        "eth",
        "ethereum",
        "以太坊",
        "sol",
        "solana",
        "doge",
        "xrp",
        "ada",
        "gold",
        "silver",
        "原油",
        "黄金",
        "白银",
        "纳指",
        "标普",
        "道琼斯",
        "上证",
        "深证",
        "恒生",
        "aapl",
        "tsla",
        "nvda",
        "amd",
        "msft",
        "goog",
        "amzn",
        "meta",
        "苹果",
        "特斯拉",
        "英伟达",
        "微软",
        "谷歌",
        "亚马逊",
    ];
    explicit_markers
        .iter()
        .any(|marker| lowered.contains(marker) || query.contains(marker))
}

fn query_has_weather_location_hint(query: &str) -> bool {
    let lowered = query.to_lowercase();
    if lowered.contains("weather in ") || lowered.contains("forecast for ") {
        return true;
    }

    let common_places = [
        "北京",
        "上海",
        "广州",
        "深圳",
        "杭州",
        "成都",
        "重庆",
        "武汉",
        "西安",
        "南京",
        "苏州",
        "天津",
        "长沙",
        "郑州",
        "合肥",
        "济南",
        "青岛",
        "厦门",
        "福州",
        "香港",
        "澳门",
        "台北",
        "new york",
        "san francisco",
        "london",
        "tokyo",
        "singapore",
        "sydney",
        "paris",
        "berlin",
        "los angeles",
        "beijing",
        "shanghai",
        "guangzhou",
        "shenzhen",
        "hangzhou",
        "chengdu",
    ];
    if common_places
        .iter()
        .any(|marker| lowered.contains(marker) || query.contains(marker))
    {
        return true;
    }

    ["省", "市", "县", "区", "镇", "乡", "州"]
        .iter()
        .any(|suffix| query.contains(suffix))
}

fn looks_like_explanatory_query(query: &str) -> bool {
    let explain_markers = [
        "什么是",
        "是什么",
        "什么意思",
        "有啥用",
        "有什么用",
        "介绍一下",
        "解释一下",
        "怎么理解",
        "区别是什么",
        "是什么东西",
        "what is",
        "what's",
        "meaning of",
        "explain",
        "introduce",
        "tell me about",
        "what does",
    ];

    explain_markers.iter().any(|marker| query.contains(marker))
}

fn looks_like_execution_request(query: &str, tokens: &[String]) -> bool {
    let has_token = |needle: &str| tokens.iter().any(|token| token == needle);
    let has_any_token = |needles: &[&str]| needles.iter().any(|needle| has_token(needle));
    let contains_any = |needles: &[&str]| needles.iter().any(|needle| query.contains(needle));

    contains_any(&[
        "用", "执行", "运行", "调用", "打开", "列出", "查看", "转换", "编译", "安装", "启动",
        "停止", "导出", "抓取", "检查", "run ", "use ", "execute", "invoke", "open ", "list ",
        "show ", "convert", "build", "install", "launch", "start ", "stop ",
    ]) || has_any_token(&[
        "run", "use", "execute", "invoke", "open", "list", "show", "convert", "build", "install",
        "launch", "start", "stop",
    ])
}

fn query_requests_verification(query: &str) -> bool {
    let markers = [
        "确认",
        "核实",
        "验证",
        "检查",
        "看看有没有",
        "有没有",
        "在不在",
        "存在吗",
        "是否存在",
        "可用吗",
        "是否可用",
        "装了吗",
        "安装了吗",
        "成功了吗",
        "完成了吗",
        "有没有成功",
        "有没有执行",
        "exists",
        "exist",
        "available",
        "installed",
        "ready",
        "verify",
        "confirm",
        "check whether",
    ];

    markers.iter().any(|marker| query.contains(marker))
}

fn looks_like_fact_check_request(query: &str) -> bool {
    let markers = [
        "帮我看",
        "帮我查",
        "查一下",
        "看一下",
        "看下",
        "看看",
        "当前",
        "现在",
        "是否",
        "有没",
        "有没有",
        "可不可用",
        "能不能用",
        "is there",
        "current",
        "right now",
    ];

    query_requests_verification(query) || markers.iter().any(|marker| query.contains(marker))
}

fn query_requests_tool_fact_verification(query: &str) -> bool {
    let tool_markers = [
        "工具",
        "命令",
        "cli",
        "程序",
        "插件",
        "adapter",
        "tool",
        "command",
        "binary",
        "安装",
        "git",
        "ffmpeg",
        "docker",
        "playwright",
    ];

    looks_like_fact_check_request(query) && tool_markers.iter().any(|marker| query.contains(marker))
}

fn query_requests_state_fact_verification(query: &str) -> bool {
    let state_markers = [
        "状态",
        "就绪",
        "连接",
        "在线",
        "可用",
        "host",
        "runtime",
        "模型是否",
        "模型有没有",
        "是否启动",
        "是否已启动",
        "ready",
        "status",
        "connected",
        "running",
        "host_runtime",
    ];

    looks_like_fact_check_request(query)
        && state_markers.iter().any(|marker| query.contains(marker))
}

fn query_requests_execution_fact_verification(query: &str) -> bool {
    let execution_markers = [
        "执行结果",
        "执行成功",
        "未提交改动",
        "git status",
        "shows changes",
        "show changes",
        "改了没",
        "改了没有",
        "文件是否改了",
        "文件改了吗",
        "生成了吗",
        "输出是什么",
        "结果是什么",
        "跑完了吗",
        "有没有执行",
        "有没有成功",
        "是否完成",
        "did it run",
        "did it finish",
        "was it created",
        "execution result",
        "command output",
        "finished",
        "completed",
    ];

    looks_like_fact_check_request(query)
        && execution_markers
            .iter()
            .any(|marker| query.contains(marker))
}

pub fn query_requests_high_risk_verification(query: &str) -> bool {
    let risk_domain_markers = [
        "法律",
        "律师",
        "诉讼",
        "合同",
        "违法",
        "合规",
        "医疗",
        "医生",
        "症状",
        "药",
        "吃药",
        "处方",
        "胸口疼",
        "发烧",
        "金融",
        "投资",
        "理财",
        "股票",
        "基金",
        "报税",
        "税务",
        "保险",
        "medical",
        "doctor",
        "legal",
        "lawyer",
        "financial",
        "invest",
        "tax",
        "insurance",
    ];
    let advice_markers = [
        "怎么办",
        "要不要",
        "该不该",
        "应该",
        "能不能",
        "是否应该",
        "建议",
        "how should",
        "should i",
        "what should i do",
    ];

    risk_domain_markers
        .iter()
        .any(|marker| query.contains(marker))
        && advice_markers.iter().any(|marker| query.contains(marker))
}

pub fn query_requests_document_understanding(query: &str) -> bool {
    let lowered = query.to_lowercase();
    let explicit_artifact_markers = [
        "pdf",
        "附件",
        "图片",
        "图像",
        "截图",
        "识图",
        "ocr",
        "音频",
        "语音",
        "录音",
        "视频",
        "总结这个pdf",
        "read this pdf",
        "analyze this image",
        "transcribe this audio",
        "summarize this document",
        "extract text",
    ];
    let action_markers = ["帮我看", "帮我读", "帮我解析", "帮我提取"];
    let contextual_artifact_markers = [
        "这个文件",
        "这份文件",
        "上传的文件",
        "该文件",
        "这个文档",
        "这份文档",
        "上传的文档",
        "该文档",
    ];
    let has_explicit_artifact_marker = explicit_artifact_markers
        .iter()
        .any(|marker| lowered.contains(marker) || query.contains(marker));
    let has_contextual_artifact_marker = contextual_artifact_markers
        .iter()
        .any(|marker| lowered.contains(marker) || query.contains(marker));
    let has_action_marker = action_markers.iter().any(|marker| query.contains(marker));

    has_explicit_artifact_marker
        || has_contextual_artifact_marker
        || (has_action_marker
            && ["这个", "这份", "该", "上传", "附件", "图片", "截图", "pdf"]
                .iter()
                .any(|marker| query.contains(marker) || lowered.contains(marker)))
}

pub fn query_requests_image_generation(query: &str) -> bool {
    let lowered = query.to_lowercase();
    let understanding_marker_hit = [
        "图片理解",
        "图像理解",
        "识图",
        "看图",
        "读图",
        "分析图片",
        "理解图片",
        "理解图像",
        "image understanding",
        "analyze this image",
        "describe this image",
        "read this image",
        "ocr",
    ]
    .iter()
    .any(|marker| lowered.contains(marker) || query.contains(marker));
    if understanding_marker_hit {
        return false;
    }

    let direct_marker_hit = [
        "generate image",
        "create image",
        "draw image",
        "make image",
        "image generation",
        "text-to-image",
        "draw me",
        "illustration",
        "poster",
        "logo design",
        "画图",
        "生成图片",
        "做图",
        "文生图",
        "画一张",
        "海报",
        "插画",
        "logo",
        "生成一张图",
    ]
    .iter()
    .any(|marker| lowered.contains(marker) || query.contains(marker));

    if direct_marker_hit {
        return true;
    }

    let has_generation_verb = ["生成", "画", "做", "帮我生成", "帮我画", "请生成", "请画"]
        .iter()
        .any(|marker| query.contains(marker) || lowered.contains(marker));

    let has_image_object = [
        "图片",
        "图像",
        "配图",
        "海报",
        "插画",
        "封面",
        "壁纸",
        "logo",
        "image",
        "picture",
        "poster",
        "illustration",
        "cover",
        "wallpaper",
    ]
    .iter()
    .any(|marker| query.contains(marker) || lowered.contains(marker));

    has_generation_verb && has_image_object
}

pub fn query_prefers_session_continuity_answer(query: &str) -> bool {
    let lowered = query.trim().to_lowercase();
    if lowered.is_empty() {
        return false;
    }

    let immediacy_markers = [
        "上一条",
        "上一句",
        "上条",
        "前面那句",
        "前面那条",
        "刚才",
        "刚刚",
        "你刚才",
        "我刚才",
        "我们刚才",
        "上一个回复",
        "上一轮",
        "这轮刚才",
        "当前会话",
        "同会话",
        "这轮会话",
        "本轮会话",
        "只根据当前会话",
        "临时暗号",
        "last message",
        "last reply",
        "previous message",
        "previous reply",
        "earlier in this chat",
        "in this session",
        "current session",
        "same session",
        "what did you just",
        "what did i just",
    ];
    let recall_markers = [
        "是什么",
        "是哪句",
        "哪句话",
        "哪一句",
        "说了什么",
        "聊到哪",
        "讲到哪",
        "聊过",
        "聊了",
        "连续聊过",
        "话题",
        "关键词",
        "暗号",
        "让我记住",
        "记住的那句话",
        "记住的那句",
        "what was",
        "which sentence",
        "what did you say",
        "what was that",
    ];

    immediacy_markers
        .iter()
        .any(|marker| lowered.contains(&marker.to_lowercase()))
        && recall_markers
            .iter()
            .any(|marker| lowered.contains(&marker.to_lowercase()))
}

fn infer_query_capability_domain(query: &str, tokens: &[String]) -> Option<String> {
    let has_token = |needle: &str| tokens.iter().any(|token| token == needle);
    let query_has_any = |needles: &[&str]| needles.iter().any(|needle| query.contains(needle));
    let has_url = query.contains("http://") || query.contains("https://");
    let has_current_marker = query_has_any(&[
        "当前", "最新", "现任", "current", "latest", "recent", "today",
    ]);
    let has_web_lookup_action = query_has_any(&[
        "查找", "寻找", "检索", "查询", "搜索", "搜", "找", "下载", "lookup", "search", "find",
        "download",
    ]) || tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "lookup" | "search" | "find" | "download" | "web" | "google"
        )
    });
    let has_web_scope_marker = has_url
        || query_has_any(&[
            "公网",
            "网上",
            "网络",
            "网页",
            "网站",
            "站点",
            "链接",
            "公开",
            "互联网",
            "web",
            "online",
            "internet",
            "website",
            "site",
            "url",
        ]);
    let has_policy_marker =
        query_has_any(&["政策", "规则", "法规", "policy", "rule", "regulation"]);
    let mentions_currency_name = query_has_any(&[
        "美元",
        "人民币",
        "欧元",
        "日元",
        "港币",
        "英镑",
        "澳元",
        "加元",
        "新加坡元",
        "韩元",
        "台币",
    ]);
    let has_quantity_question =
        query_has_any(&["多少", "几多", "是多少", "几", "what is", "how much"]);
    let has_market_value_marker = query_has_any(&[
        "价格",
        "币价",
        "股价",
        "报价",
        "行情",
        "点数",
        "指数",
        "股票",
        "基金",
        "期货",
        "加密货币",
        "虚拟货币",
    ]) || tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "price"
                | "quote"
                | "btc"
                | "eth"
                | "stock"
                | "stocks"
                | "equity"
                | "ticker"
                | "crypto"
                | "coin"
                | "token"
                | "index"
                | "indices"
                | "points"
        )
    });
    let has_crypto_quantity_target =
        has_quantity_question && query.contains('币') && !mentions_currency_name;

    if tokens
        .iter()
        .any(|token| matches!(token.as_str(), "weather" | "forecast" | "气温"))
        || query_has_any(&["天气", "预报"])
    {
        return Some("realtime_lookup.weather".to_string());
    }

    if has_token("fx")
        || query.contains("汇率")
        || query.contains("exchange rate")
        || query.contains("currency pair")
        || (tokens.iter().any(|token| {
            matches!(
                token.as_str(),
                "汇率" | "rate" | "usd" | "cny" | "eur" | "jpy" | "hkd" | "gbp"
            )
        }) && (query.contains("exchange rate")
            || query.contains("currency pair")
            || query.contains("汇率")
            || query.contains("兑")
            || query.contains("to ")
            || mentions_currency_name
            || tokens.len() >= 2 && has_token("rate")))
        || ((query.contains("汇率") || query.contains("兑")) && mentions_currency_name)
    {
        return Some("realtime_lookup.fx".to_string());
    }

    if has_market_value_marker
        || (has_quantity_question && query_has_specific_price_target(query))
        || has_crypto_quantity_target
        || (has_quantity_question
            && tokens.iter().any(|token| {
                matches!(
                    token.as_str(),
                    "nasdaq" | "dow" | "sp500" | "s&p" | "nikkei" | "hang" | "seng"
                )
            }))
    {
        return Some("realtime_lookup.price".to_string());
    }

    if query_requests_writing_continuation(query, &tokens) {
        return Some("writing".to_string());
    }

    if query_requests_writing(query, tokens)
        && !(has_web_lookup_action
            || has_web_scope_marker
            || has_url
            || (has_current_marker && has_policy_marker)
            || query_has_any(&["最近", "最新", "新闻", "current policy", "latest"]))
    {
        return Some("writing".to_string());
    }

    if has_web_lookup_action
        && (has_web_scope_marker
            || query_has_any(&["搜索", "检索", "搜一下", "搜索一下"])
            || tokens
                .iter()
                .any(|token| matches!(token.as_str(), "search" | "web" | "google")))
    {
        return Some("realtime_lookup.web".to_string());
    }

    if tokens
        .iter()
        .any(|token| matches!(token.as_str(), "latest" | "news" | "today" | "incumbent"))
        || (has_current_marker && has_policy_marker)
        || query_has_any(&[
            "最近",
            "最新",
            "新闻",
            "现任",
            "当前政策",
            "最新政策",
            "current ceo",
            "current president",
            "current policy",
            "current version",
            "release version",
        ])
    {
        return Some("realtime_lookup.latest_info".to_string());
    }

    if has_url
        && (query_has_any(&[
            "读取", "打开", "浏览", "抓取", "页面", "网页", "标题", "摘要", "read", "open",
            "browse", "fetch", "page", "title", "summary",
        ]) || tokens
            .iter()
            .any(|token| matches!(token.as_str(), "read" | "open" | "browse" | "fetch")))
    {
        return Some("realtime_lookup.web".to_string());
    }

    if (has_web_lookup_action && has_web_scope_marker)
        || tokens
            .iter()
            .any(|token| matches!(token.as_str(), "search" | "web" | "google"))
        || query_has_any(&["网页", "搜索"])
    {
        return Some("realtime_lookup.web".to_string());
    }

    if query_requests_image_generation(query)
        || tokens.iter().any(|token| {
            matches!(
                token.as_str(),
                "draw" | "drawing" | "illustration" | "poster" | "logo" | "render"
            )
        })
        || query_has_any(&["画图", "生成图片", "做图", "文生图", "海报", "插画"])
    {
        return Some("image_generation".to_string());
    }

    if query_requests_document_understanding(query)
        || tokens
            .iter()
            .any(|token| matches!(token.as_str(), "pdf" | "document" | "ocr" | "extract"))
    {
        return Some("document_understanding".to_string());
    }

    if tokens
        .iter()
        .any(|token| matches!(token.as_str(), "image" | "vision" | "visual"))
        || query_has_any(&["图像", "截图"])
    {
        return Some("document_understanding".to_string());
    }

    if tokens
        .iter()
        .any(|token| matches!(token.as_str(), "voice" | "audio" | "speech"))
        || query_has_any(&["语音", "音频"])
    {
        return Some("voice_understanding".to_string());
    }

    if query_requests_tool_fact_verification(query)
        && tokens.iter().any(|token| {
            matches!(
                token.as_str(),
                "git"
                    | "ffmpeg"
                    | "docker"
                    | "npm"
                    | "pnpm"
                    | "yarn"
                    | "cargo"
                    | "chrome"
                    | "chromium"
                    | "playwright"
                    | "adb"
                    | "sqlite3"
                    | "ffprobe"
                    | "cli"
            )
        })
    {
        return Some("external_cli_tools".to_string());
    }

    if (query_requests_tool_fact_verification(query)
        || query_requests_state_fact_verification(query))
        && tokens.iter().any(|token| {
            matches!(
                token.as_str(),
                "bash"
                    | "powershell"
                    | "pwsh"
                    | "cmd"
                    | "shell"
                    | "terminal"
                    | "uv"
                    | "pixi"
                    | "bun"
                    | "gcc"
                    | "python"
                    | "node"
                    | "quickjs"
            )
        })
    {
        return Some("runtime_surface".to_string());
    }

    if query_requests_state_fact_verification(query)
        && query_has_any(&[
            "系统状态",
            "当前系统状态",
            "宿主状态",
            "运行时状态",
            "环境状态",
        ])
    {
        return Some("runtime_surface".to_string());
    }

    if query_requests_execution_fact_verification(query)
        && (tokens.iter().any(|token| {
            matches!(
                token.as_str(),
                "git" | "repo" | "repository" | "branch" | "status"
            )
        }) || query_has_any(&["未提交改动", "工作区", "仓库改动", "git status"]))
    {
        return Some("external_cli_tools".to_string());
    }

    if query_requests_execution_fact_verification(query)
        && query_has_any(&[
            "文件是否改了",
            "文件改了吗",
            "改了没有",
            "改了没",
            "生成了吗",
            "输出是什么",
            "结果是什么",
        ])
    {
        return Some("runtime_surface".to_string());
    }

    if !looks_like_explanatory_query(query)
        && looks_like_execution_request(query, tokens)
        && tokens.iter().any(|token| {
            matches!(
                token.as_str(),
                "git"
                    | "ffmpeg"
                    | "docker"
                    | "npm"
                    | "pnpm"
                    | "yarn"
                    | "cargo"
                    | "chrome"
                    | "chromium"
                    | "playwright"
                    | "adb"
                    | "sqlite3"
                    | "ffprobe"
                    | "cli"
            )
        })
        || (!looks_like_explanatory_query(query)
            && looks_like_execution_request(query, tokens)
            && query_has_any(&["分支", "程序自带cli", "程序自带命令"]))
    {
        return Some("external_cli_tools".to_string());
    }

    if !looks_like_explanatory_query(query)
        && looks_like_execution_request(query, tokens)
        && tokens.iter().any(|token| {
            matches!(
                token.as_str(),
                "bash"
                    | "powershell"
                    | "pwsh"
                    | "cmd"
                    | "shell"
                    | "terminal"
                    | "uv"
                    | "pixi"
                    | "bun"
                    | "gcc"
                    | "python"
                    | "node"
                    | "quickjs"
            )
        })
        || (!looks_like_explanatory_query(query)
            && looks_like_execution_request(query, tokens)
            && query_has_any(&["命令行", "终端", "脚本运行时"]))
    {
        return Some("runtime_surface".to_string());
    }

    if query_requests_file_ops(query, tokens) {
        return Some("file_ops".to_string());
    }

    if query_requests_capability_gap(query, tokens) {
        return Some("capability_gap".to_string());
    }

    if query_requests_memory(query, tokens) {
        return Some("memory".to_string());
    }

    if query_requests_coding(query, tokens) {
        return Some("coding".to_string());
    }

    if query_requests_communication(query, tokens) {
        return Some("communication".to_string());
    }

    None
}

fn query_requests_writing(query: &str, tokens: &[String]) -> bool {
    let lowered = query.to_lowercase();
    let contains_any = |needles: &[&str]| needles.iter().any(|needle| query.contains(needle));
    let lowered_contains_any = |needles: &[&str]| {
        needles
            .iter()
            .any(|needle| lowered.contains(&needle.to_lowercase()))
    };
    let has_token = |needles: &[&str]| {
        tokens
            .iter()
            .any(|token| needles.iter().any(|needle| token == needle))
    };

    let writing_verbs = [
        "写",
        "续写",
        "继续写",
        "修订",
        "修改",
        "修正",
        "改写",
        "润色",
        "补全",
        "完善",
        "整理",
        "更新",
        "校订",
        "编辑",
        "创作",
        "撰写",
        "起草",
        "草拟",
        "成文",
        "正文",
        "draft",
        "write",
        "compose",
        "continue",
        "author",
        "revise",
        "revision",
        "edit",
        "update",
        "rewrite",
        "polish",
        "complete",
        "expand",
        "refine",
    ];
    let writing_artifacts = [
        "小说",
        "故事",
        "章节",
        "章",
        "文章",
        "论文",
        "作文",
        "报告",
        "文稿",
        "长文",
        "稿件",
        "草稿",
        "正文",
        "摘要",
        "大纲",
        "设定",
        "连续性",
        "novel",
        "story",
        "chapter",
        "article",
        "paper",
        "essay",
        "report",
        "manuscript",
        "document",
        "draft",
        "outline",
        "summary",
        "continuity",
    ];
    let coding_markers = [
        "代码",
        "脚本",
        "程序",
        "仓库",
        "bug",
        "rust",
        "python",
        "typescript",
        "javascript",
        "code",
        "script",
        "repo",
        "repository",
    ];

    let has_writing_verb = contains_any(&writing_verbs)
        || lowered_contains_any(&writing_verbs)
        || has_token(&["write", "draft", "compose", "continue", "author"]);
    let has_writing_artifact = contains_any(&writing_artifacts)
        || lowered_contains_any(&writing_artifacts)
        || has_token(&[
            "novel",
            "story",
            "chapter",
            "article",
            "paper",
            "essay",
            "report",
            "manuscript",
            "document",
        ]);
    let looks_coding = contains_any(&coding_markers)
        || lowered_contains_any(&coding_markers)
        || has_token(&[
            "code",
            "script",
            "repo",
            "repository",
            "rust",
            "python",
            "typescript",
            "javascript",
        ]);

    has_writing_verb && has_writing_artifact && !looks_coding
}

fn query_requests_writing_continuation(query: &str, tokens: &[String]) -> bool {
    query.contains("继续写")
        || query.contains("续写")
        || (query.contains("继续") && (query.contains("章节") || query.contains("正文")))
        || (query.contains("修订") && (query.contains("章节") || query.contains("章")))
        || (query.contains("补全") && (query.contains("章节") || query.contains("章")))
        || (query.contains("润色") && (query.contains("章节") || query.contains("章")))
        || tokens
            .iter()
            .any(|token| matches!(token.as_str(), "continue" | "chapter" | "revise"))
}

fn query_requests_coding(query: &str, tokens: &[String]) -> bool {
    let lowered = query.to_lowercase();
    let has_token = |needles: &[&str]| {
        tokens
            .iter()
            .any(|token| needles.iter().any(|needle| token == needle))
    };
    let contains_any = |needles: &[&str]| needles.iter().any(|needle| query.contains(needle));
    let lowered_contains_any = |needles: &[&str]| {
        needles
            .iter()
            .any(|needle| lowered.contains(&needle.to_lowercase()))
    };

    let coding_markers = [
        "代码",
        "仓库",
        "repo",
        "repository",
        "bug",
        "commit",
        "patch",
        "feature",
        "pull request",
        "branch",
        "编译",
        "build",
        "cargo",
        "cargo test",
        "pytest",
        "单元测试",
        "集成测试",
        "rust",
        "python",
        "typescript",
        "javascript",
    ];

    let coding_verbs = [
        "写",
        "改",
        "修",
        "实现",
        "开发",
        "重构",
        "加上",
        "补上",
        "测试",
        "提交",
        "优化",
        "排查",
        "write",
        "fix",
        "implement",
        "build",
        "refactor",
        "test",
        "patch",
        "debug",
        "review",
    ];

    (contains_any(&coding_markers)
        || lowered_contains_any(&coding_markers)
        || has_token(&[
            "code",
            "repo",
            "repository",
            "bug",
            "fix",
            "implement",
            "refactor",
            "patch",
            "build",
            "commit",
        ]))
        && (contains_any(&coding_verbs)
            || lowered_contains_any(&coding_verbs)
            || has_token(&[
                "write",
                "fix",
                "implement",
                "build",
                "refactor",
                "test",
                "patch",
                "debug",
                "review",
            ]))
}

fn query_requests_communication(query: &str, tokens: &[String]) -> bool {
    let lowered = query.to_lowercase();
    let contains_any = |needles: &[&str]| needles.iter().any(|needle| query.contains(needle));
    let lowered_contains_any = |needles: &[&str]| {
        needles
            .iter()
            .any(|needle| lowered.contains(&needle.to_lowercase()))
    };
    let has_token = |needles: &[&str]| {
        tokens
            .iter()
            .any(|token| needles.iter().any(|needle| token == needle))
    };

    let channels = [
        "邮件",
        "邮箱",
        "email",
        "mail",
        "slack",
        "discord",
        "telegram",
        "通知",
        "提醒",
        "消息",
        "notification",
        "message",
    ];
    let verbs = [
        "发送", "发给", "通知", "提醒", "回复", "草拟", "draft", "send", "notify", "reply",
        "message",
    ];

    (contains_any(&channels)
        || lowered_contains_any(&channels)
        || has_token(&[
            "email",
            "mail",
            "slack",
            "discord",
            "telegram",
            "notify",
            "notification",
            "message",
        ]))
        && (contains_any(&verbs)
            || lowered_contains_any(&verbs)
            || has_token(&["send", "notify", "reply", "draft", "message"]))
}

fn query_requests_memory(query: &str, tokens: &[String]) -> bool {
    let lowered = query.to_lowercase();
    let blocks_durable_memory = [
        "不要保存为长期记忆",
        "不要写入长期记忆",
        "不要保存到记忆",
        "不要记住",
        "do not save",
        "don't save",
        "do not remember",
        "don't remember",
    ]
    .iter()
    .any(|needle| lowered.contains(&needle.to_lowercase()));
    if query_prefers_session_continuity_answer(query) && blocks_durable_memory {
        return false;
    }

    let explicit_memory_marker = [
        "记住",
        "记忆",
        "memory",
        "remember",
        "recall",
        "让你记住",
        "我让你记住",
    ]
    .iter()
    .any(|needle| lowered.contains(&needle.to_lowercase()));
    if query_prefers_session_continuity_answer(query) && !explicit_memory_marker {
        return false;
    }

    let contains_any = |needles: &[&str]| needles.iter().any(|needle| query.contains(needle));
    let lowered_contains_any = |needles: &[&str]| {
        needles
            .iter()
            .any(|needle| lowered.contains(&needle.to_lowercase()))
    };
    let has_token = |needles: &[&str]| {
        tokens
            .iter()
            .any(|token| needles.iter().any(|needle| token == needle))
    };

    contains_any(&[
        "记住",
        "记下来",
        "还记得",
        "记得",
        "回忆",
        "想起来",
        "上次",
        "刚才",
        "之前",
        "以前",
        "历史里",
        "知识库",
        "记忆",
        "之前说过",
        "我让你记住",
        "让你记住",
    ]) || lowered_contains_any(&[
        "remember",
        "recall",
        "memory",
        "history",
        "last time",
        "earlier",
        "previous",
        "previously",
        "knowledge base",
        "previously said",
    ]) || has_token(&["remember", "recall", "memory", "history", "knowledge"])
}

pub fn query_requests_memory_write(query: &str) -> bool {
    let lowered = query.to_lowercase();
    [
        "记住",
        "记下来",
        "保存到记忆",
        "保存为记忆",
        "写入记忆",
        "remember this",
        "save this",
        "store this",
        "save to memory",
    ]
    .iter()
    .any(|needle| lowered.contains(&needle.to_lowercase()))
}

pub fn query_requests_fact_management(query: &str) -> bool {
    let lowered = query.to_lowercase();
    let is_recall_or_check = [
        "查", "找回", "回忆", "读取", "再查", "recall", "retrieve", "look up",
    ]
    .iter()
    .any(|needle| lowered.contains(&needle.to_lowercase()));
    let is_conditional_delete_mention = ["如果", "是否", "已经删除", "删掉了吗", "if", "whether"]
        .iter()
        .any(|needle| lowered.contains(&needle.to_lowercase()));
    if is_recall_or_check && is_conditional_delete_mention {
        return false;
    }

    let memory_mutation = [
        "删除", "忘记", "更新", "修改", "改成", "delete", "forget", "update", "change",
    ]
    .iter()
    .any(|needle| lowered.contains(&needle.to_lowercase()))
        && [
            "记忆",
            "记住",
            "记得",
            "验证码",
            "刚才那个",
            "刚才的",
            "那个",
            "memory",
            "remembered",
        ]
        .iter()
        .any(|needle| lowered.contains(&needle.to_lowercase()));
    if memory_mutation {
        return true;
    }

    let mentions_fact_store = [
        "核心事实",
        "事实",
        "core memory",
        "core fact",
        "fact",
        "facts",
        "manage_facts",
    ]
    .iter()
    .any(|needle| lowered.contains(&needle.to_lowercase()));
    if !mentions_fact_store {
        return false;
    }

    [
        "列出",
        "列表",
        "删除",
        "更新",
        "修改",
        "置顶",
        "保护",
        "取消保护",
        "重要性",
        "list",
        "delete",
        "update",
        "pin",
        "protect",
        "importance",
        "manage",
    ]
    .iter()
    .any(|needle| lowered.contains(&needle.to_lowercase()))
}

pub fn query_prefers_knowledge_base_retrieval(query: &str) -> bool {
    let lowered = query.to_lowercase();
    let mentions_knowledge_base = ["知识库", "资料库", "knowledge base", "knowledge-base"]
        .iter()
        .any(|needle| lowered.contains(&needle.to_lowercase()));

    if !mentions_knowledge_base {
        return false;
    }

    let retrieval_markers = [
        "读出",
        "读回",
        "查出",
        "查回",
        "取出",
        "取回",
        "找出",
        "找回",
        "告诉我",
        "给我",
        "列出",
        "摘要",
        "标题",
        "内容",
        "详情",
        "from the knowledge base",
        "read back",
        "read from",
        "look up",
        "lookup",
        "retrieve",
        "tell me",
        "show me",
        "summary",
        "title",
        "contents",
        "details",
    ];

    let mutation_markers = [
        "记住",
        "保存到知识库",
        "存入知识库",
        "写入知识库",
        "加入知识库",
        "更新这条事实",
        "删除这条事实",
        "保护这条事实",
        "置顶这条事实",
        "pin this fact",
        "protect this fact",
        "update this fact",
        "delete this fact",
        "save this to the knowledge base",
        "store this in the knowledge base",
        "write this into the knowledge base",
        "remember this",
    ];

    retrieval_markers
        .iter()
        .any(|needle| lowered.contains(&needle.to_lowercase()))
        && !mutation_markers
            .iter()
            .any(|needle| lowered.contains(&needle.to_lowercase()))
}

fn query_requests_capability_gap(query: &str, tokens: &[String]) -> bool {
    let lowered = query.to_lowercase();
    let contains_any = |needles: &[&str]| needles.iter().any(|needle| query.contains(needle));
    let lowered_contains_any = |needles: &[&str]| {
        needles
            .iter()
            .any(|needle| lowered.contains(&needle.to_lowercase()))
    };
    let has_token = |needles: &[&str]| {
        tokens
            .iter()
            .any(|token| needles.iter().any(|needle| token == needle))
    };

    let artifact_markers = [
        "工具",
        "插件",
        "skill",
        "worker",
        "能力",
        "脚本",
        "自动化",
        "plugin",
        "tool",
        "script",
        "automation",
        "agent",
    ];
    let build_verbs = [
        "造",
        "做",
        "创建",
        "生成",
        "安装",
        "接入",
        "添加",
        "配置",
        "装上",
        "启用",
        "编写",
        "开发",
        "实现",
        "搭一个",
        "写一个",
        "build",
        "create",
        "generate",
        "install",
        "setup",
        "set up",
        "add",
        "enable",
        "configure",
        "make",
        "implement",
        "develop",
        "write",
    ];

    (contains_any(&artifact_markers)
        || lowered_contains_any(&artifact_markers)
        || has_token(&[
            "skill",
            "worker",
            "tool",
            "plugin",
            "script",
            "automation",
            "agent",
        ]))
        && (contains_any(&build_verbs)
            || lowered_contains_any(&build_verbs)
            || has_token(&[
                "build",
                "create",
                "generate",
                "install",
                "setup",
                "add",
                "enable",
                "configure",
                "make",
                "implement",
                "develop",
                "write",
            ]))
}

fn query_requests_file_ops(query: &str, tokens: &[String]) -> bool {
    let lowered = query.to_lowercase();
    let query_has_any = |needles: &[&str]| needles.iter().any(|needle| query.contains(needle));
    let lowered_has_any = |needles: &[&str]| {
        needles
            .iter()
            .any(|needle| lowered.contains(&needle.to_lowercase()))
    };
    let has_token = |needles: &[&str]| {
        tokens
            .iter()
            .any(|token| needles.iter().any(|n| token == n))
    };

    let has_path_like_target = query_contains_filesystem_path(query)
        || query_has_any(&[
            "文件",
            "文件夹",
            "目录",
            "路径",
            "工作区",
            "workspace",
            "path",
            "folder",
            "directory",
        ])
        || has_token(&[
            "file",
            "files",
            "folder",
            "directory",
            "path",
            "workspace",
            "readme",
            "md",
            "json",
            "yaml",
            "toml",
            "txt",
            "log",
            "csv",
            "rs",
            "py",
            "js",
            "ts",
        ]);

    let has_file_op_verb = query_has_any(&[
        "读取",
        "读出",
        "打开",
        "查看",
        "看下",
        "显示",
        "列出",
        "罗列",
        "打印",
        "写入",
        "写到",
        "保存到",
        "修改",
        "编辑",
        "创建文件",
        "读取文件",
        "读取目录",
    ]) || lowered_has_any(&[
        "read ", "open ", "show ", "view ", "list ", "ls ", "cat ", "write ", "save ", "edit ",
    ]) || has_token(&[
        "read", "open", "show", "view", "list", "ls", "cat", "write", "save", "edit",
    ]);

    let has_file_output_marker =
        query_has_any(&[
            "前一行",
            "前两行",
            "前三行",
            "前几行",
            "内容",
            "全文",
            "第一行",
            "第二行",
            "第三行",
            "列一下",
        ]) || lowered_has_any(&["first line", "first lines", "top lines", "contents"]);

    (has_path_like_target && has_file_op_verb)
        || (query_contains_filesystem_path(query) && has_file_output_marker)
}

fn query_contains_filesystem_path(query: &str) -> bool {
    let trimmed = query.trim();
    let tokens = trimmed
        .split_whitespace()
        .map(|token| token.trim_matches(|c: char| matches!(c, '"' | '\'' | '，' | '。' | ',')))
        .collect::<Vec<_>>();

    tokens.iter().any(|token| {
        token.starts_with('/')
            || token.starts_with("./")
            || token.starts_with("../")
            || token.starts_with("~/")
            || (token.len() > 3
                && token.as_bytes().get(1) == Some(&b':')
                && matches!(token.as_bytes().get(2), Some(b'\\' | b'/')))
            || [
                ".md", ".txt", ".json", ".yaml", ".yml", ".toml", ".rs", ".py", ".js", ".ts",
                ".csv", ".log",
            ]
            .iter()
            .any(|ext| token.ends_with(ext))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_capability_route_detects_shared_intents() {
        assert_eq!(
            classify_query_capability_route("帮我查 BTC 现在价格"),
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::PriceLookup
            ))
        );
        assert_eq!(
            classify_query_capability_route("纳斯达克点数多少？"),
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::PriceLookup
            ))
        );
        assert_eq!(
            classify_query_capability_route("比特币现在多少钱？"),
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::PriceLookup
            ))
        );
        assert_eq!(
            classify_query_capability_route("AAPL 股票现在多少钱？"),
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::PriceLookup
            ))
        );
        assert_eq!(
            classify_query_capability_route("请读取 https://example.com 的页面标题"),
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::WebSearch
            ))
        );
        assert_eq!(
            classify_query_capability_route("请在公网查找热门免费资料并保存成txt文档"),
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::WebSearch
            ))
        );
        assert_eq!(
            classify_query_capability_route(
                "search the public market for downloadable free fiction"
            ),
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::WebSearch
            ))
        );
        assert_eq!(
            classify_query_capability_route(
                "Search for popular, downloadable, and free fantasy (玄幻/奇幻) novels available on the public web. Find up to 10 novels and their content."
            ),
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::WebSearch
            ))
        );
        assert_eq!(
            classify_query_capability_route("在网上寻找可下载的数据集，之后写入知识库"),
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::WebSearch
            ))
        );
        assert_ne!(
            classify_query_capability_route("帮我写一个txt文档"),
            Some(CapabilityRouteHint::DocumentUnderstanding)
        );
        assert_eq!(
            classify_query_capability_route("请从零开始写一部玄幻小说，保持人物名字和设定不漂移"),
            Some(CapabilityRouteHint::Writing)
        );
        assert_eq!(
            classify_query_capability_route(
                "继续写第三章，回顾当前主角、力量规则和未解决伏笔，检查有没有前后矛盾"
            ),
            Some(CapabilityRouteHint::Writing)
        );
        assert_eq!(
            classify_query_capability_route(
                "请继续处理第二章，按照刚才的检查结果修订它，补全摘要、关键事实和连续性更新"
            ),
            Some(CapabilityRouteHint::Writing)
        );
        assert_eq!(
            classify_query_capability_route("查找最近的医学论文并写一篇报告"),
            Some(CapabilityRouteHint::RealtimeLookup(
                RealtimeLookupKind::LatestInfoLookup
            ))
        );
        assert_eq!(
            capability_route_preferred_tool_names(CapabilityRouteHint::Writing)
                .first()
                .copied(),
            Some("novel_studio")
        );
        assert_eq!(
            classify_query_capability_route("帮我总结这个 PDF"),
            Some(CapabilityRouteHint::DocumentUnderstanding)
        );
        assert_eq!(
            classify_query_capability_route("用 powershell 列出当前目录"),
            Some(CapabilityRouteHint::RuntimeSurface)
        );
        assert_eq!(
            classify_query_capability_route("你好，用一句中文回复：现在可以开始测试。"),
            None
        );
        assert_eq!(classify_query_capability_route("现在可以开始测试。"), None);
        assert_eq!(
            classify_query_capability_route("帮我测试这个 Rust 仓库"),
            Some(CapabilityRouteHint::Coding)
        );
        assert_eq!(classify_query_capability_route("什么是 git"), None);
    }

    #[test]
    fn routing_judgment_only_queries_are_detected() {
        assert!(query_requests_routing_judgment_only(
            "只做路由判断，不要执行：这件事应该交给谁？"
        ));
        assert!(query_requests_routing_judgment_only(
            "route only: who should handle this task?"
        ));
        assert!(!query_requests_routing_judgment_only(
            "帮我路由到 researcher 并执行"
        ));
    }

    #[test]
    fn router_applies_request_biases_and_suppression() {
        assert_eq!(
            resolve_capability_route(
                "普通一句话",
                CapabilityRouteRequest {
                    has_media_input: true,
                    ..Default::default()
                },
            ),
            Some(CapabilityRouteHint::DocumentUnderstanding)
        );
        assert_eq!(
            resolve_capability_route(
                "帮我做一个搜索 btc 价格的工具",
                CapabilityRouteRequest {
                    suppress_document_understanding: true,
                    suppress_realtime_lookup: true,
                    ..Default::default()
                },
            ),
            None
        );
        assert_eq!(
            resolve_capability_route(
                "帮我做一个图片理解工具",
                CapabilityRouteRequest {
                    suppress_document_understanding: true,
                    ..Default::default()
                },
            ),
            None
        );
    }
}

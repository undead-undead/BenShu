use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtendedPreFlightLevel {
    None,
    ComplexTask,
    HighRiskTask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PreFlightRouteClass {
    None,
    Complex,
    HighRisk,
}

pub fn is_lightweight_repo_inspection_request(user_request: &str) -> bool {
    let normalized = user_request.trim().to_lowercase();
    if normalized.is_empty() {
        return false;
    }

    let mentions_repo_scope = [
        "当前项目",
        "这个项目",
        "代码库",
        "仓库",
        "repo",
        "repository",
        "project",
        "workspace",
        "docs",
        "文档",
        "文件",
    ]
    .iter()
    .any(|needle| normalized.contains(needle));

    if !mentions_repo_scope {
        return false;
    }

    let inspection_intent = [
        "查看",
        "看看",
        "看一下",
        "帮我看",
        "总结",
        "概括",
        "简短总结",
        "brief summary",
        "summary",
        "summarize",
        "inspect",
        "review",
        "read",
    ]
    .iter()
    .any(|needle| normalized.contains(needle));

    if !inspection_intent {
        return false;
    }

    let mutating_or_runtime_intent = [
        "修改",
        "修复",
        "重构",
        "实现",
        "创建",
        "新增",
        "删除",
        "运行",
        "执行",
        "安装",
        "部署",
        "提交",
        "push",
        "commit",
        "fix",
        "refactor",
        "implement",
        "create",
        "delete",
        "run",
        "execute",
        "install",
        "deploy",
    ]
    .iter()
    .any(|needle| normalized.contains(needle));

    !mutating_or_runtime_intent
}

pub fn classify_extended_pre_flight_level(
    user_request: &str,
    route_class: PreFlightRouteClass,
    has_media_input: bool,
    requires_truth_or_freshness_verification: bool,
) -> ExtendedPreFlightLevel {
    if has_media_input {
        return ExtendedPreFlightLevel::ComplexTask;
    }

    let trimmed = user_request.trim();
    if trimmed.is_empty() {
        return ExtendedPreFlightLevel::None;
    }

    if is_lightweight_repo_inspection_request(trimmed) {
        return ExtendedPreFlightLevel::None;
    }

    if requires_truth_or_freshness_verification {
        return ExtendedPreFlightLevel::HighRiskTask;
    }

    match route_class {
        PreFlightRouteClass::HighRisk => return ExtendedPreFlightLevel::HighRiskTask,
        PreFlightRouteClass::Complex => return ExtendedPreFlightLevel::ComplexTask,
        PreFlightRouteClass::None => {}
    }

    if trimmed.chars().count() > 220
        || trimmed.contains('\n')
        || trimmed.contains("```")
        || trimmed.contains("http://")
        || trimmed.contains("https://")
    {
        return ExtendedPreFlightLevel::ComplexTask;
    }

    ExtendedPreFlightLevel::None
}

pub fn should_run_extended_pre_flight_for_turn(level: ExtendedPreFlightLevel) -> bool {
    !matches!(level, ExtendedPreFlightLevel::None)
}

pub fn extended_pre_flight_runs_complexity_estimator(level: ExtendedPreFlightLevel) -> bool {
    !matches!(level, ExtendedPreFlightLevel::None)
}

pub fn extended_pre_flight_runs_jit_distillation(level: ExtendedPreFlightLevel) -> bool {
    !matches!(level, ExtendedPreFlightLevel::None)
}

pub fn extended_pre_flight_allows_auto_stepdown(level: ExtendedPreFlightLevel) -> bool {
    matches!(level, ExtendedPreFlightLevel::ComplexTask)
}

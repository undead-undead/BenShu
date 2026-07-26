use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RealtimeLookupKind {
    WebSearch,
    PriceLookup,
    FxLookup,
    WeatherLookup,
    LatestInfoLookup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapabilityRouteHint {
    DocumentUnderstanding,
    VisualUnderstanding,
    VoiceUnderstanding,
    RealtimeLookup(RealtimeLookupKind),
    RuntimeSurface,
    ExternalCliTools,
    FileOps,
    Writing,
    Coding,
    Communication,
    Memory,
    CapabilityGap,
    General,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CapabilityRouteRequest {
    pub approved_forge_request: bool,
    pub has_media_input: bool,
    pub force_document_understanding: bool,
    pub runtime_surface_bias: bool,
    pub suppress_document_understanding: bool,
    pub suppress_realtime_lookup: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapabilityClarificationHint {
    MissingPriceTarget,
    MissingFxPair,
    MissingWeatherLocation,
}

pub fn capability_route_hint_label(route: CapabilityRouteHint) -> &'static str {
    match route {
        CapabilityRouteHint::DocumentUnderstanding => "document_understanding",
        CapabilityRouteHint::VisualUnderstanding => "document_understanding",
        CapabilityRouteHint::VoiceUnderstanding => "voice_understanding",
        CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::WebSearch) => "realtime_lookup.web",
        CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::PriceLookup) => {
            "realtime_lookup.price"
        }
        CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::FxLookup) => "realtime_lookup.fx",
        CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::WeatherLookup) => {
            "realtime_lookup.weather"
        }
        CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::LatestInfoLookup) => {
            "realtime_lookup.latest_info"
        }
        CapabilityRouteHint::RuntimeSurface => "runtime_surface",
        CapabilityRouteHint::ExternalCliTools => "external_cli_tools",
        CapabilityRouteHint::FileOps => "file_ops",
        CapabilityRouteHint::Writing => "writing",
        CapabilityRouteHint::Coding => "coding",
        CapabilityRouteHint::Communication => "communication",
        CapabilityRouteHint::Memory => "memory",
        CapabilityRouteHint::CapabilityGap => "capability_gap",
        CapabilityRouteHint::General => "general",
    }
}

pub fn capability_route_debug_label(route: CapabilityRouteHint) -> &'static str {
    match route {
        CapabilityRouteHint::DocumentUnderstanding => "document hard route",
        CapabilityRouteHint::FileOps => "file_ops hard route",
        CapabilityRouteHint::RealtimeLookup(_) => "realtime lookup hard route",
        CapabilityRouteHint::RuntimeSurface => "runtime_surface hard route",
        CapabilityRouteHint::ExternalCliTools => "external_cli_tools hard route",
        _ => "shared capability route",
    }
}

pub fn capability_route_requires_real_tool_call(route: CapabilityRouteHint) -> bool {
    matches!(
        route,
        CapabilityRouteHint::DocumentUnderstanding
            | CapabilityRouteHint::FileOps
            | CapabilityRouteHint::Writing
            | CapabilityRouteHint::RealtimeLookup(_)
            | CapabilityRouteHint::RuntimeSurface
            | CapabilityRouteHint::ExternalCliTools
            | CapabilityRouteHint::Coding
            | CapabilityRouteHint::Communication
            | CapabilityRouteHint::Memory
            | CapabilityRouteHint::CapabilityGap
    )
}

pub fn capability_route_prefers_direct_tool_surface(route: CapabilityRouteHint) -> bool {
    matches!(
        route,
        CapabilityRouteHint::RealtimeLookup(_) | CapabilityRouteHint::Writing
    )
}

pub fn capability_route_requires_source_fetch(route: CapabilityRouteHint) -> bool {
    matches!(
        route,
        CapabilityRouteHint::RealtimeLookup(
            RealtimeLookupKind::PriceLookup
                | RealtimeLookupKind::FxLookup
                | RealtimeLookupKind::WeatherLookup
                | RealtimeLookupKind::LatestInfoLookup
        )
    )
}

pub fn capability_route_should_inject_system_message(route: CapabilityRouteHint) -> bool {
    matches!(
        route,
        CapabilityRouteHint::RealtimeLookup(_)
            | CapabilityRouteHint::Coding
            | CapabilityRouteHint::Writing
            | CapabilityRouteHint::Communication
            | CapabilityRouteHint::Memory
            | CapabilityRouteHint::CapabilityGap
    )
}

pub fn capability_route_preferred_tool_names(
    route: CapabilityRouteHint,
) -> &'static [&'static str] {
    match route {
        CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::PriceLookup) => &[
            "price_lookup",
            "web_search",
            "web_fetch",
            "browser_browse",
            "tool_search",
        ],
        CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::FxLookup) => &[
            "fx_lookup",
            "web_search",
            "web_fetch",
            "browser_browse",
            "tool_search",
        ],
        CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::WeatherLookup) => &[
            "weather_lookup",
            "web_search",
            "web_fetch",
            "browser_browse",
            "tool_search",
        ],
        CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::LatestInfoLookup) => &[
            "latest_info_lookup",
            "web_search",
            "web_fetch",
            "browser_browse",
            "tool_search",
        ],
        CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::WebSearch) => {
            &["web_search", "web_fetch", "browser_browse", "tool_search"]
        }
        CapabilityRouteHint::DocumentUnderstanding => &[
            "delegate",
            "shared_board",
            "tool_search",
            "pdf_parse",
            "text_extract",
            "document_understand",
        ],
        CapabilityRouteHint::FileOps => &[
            "read_file",
            "list_dir",
            "edit_file",
            "write_file",
            "tool_search",
        ],
        CapabilityRouteHint::Writing => &[
            "novel_studio",
            "writing_studio",
            "write_file",
            "delegate",
            "shared_board",
            "tool_search",
        ],
        CapabilityRouteHint::RuntimeSurface => &[
            "delegate",
            "shared_board",
            "runtime_surface",
            "command_exec",
            "tool_search",
        ],
        CapabilityRouteHint::ExternalCliTools => {
            &["delegate", "shared_board", "tool_search", "command_exec"]
        }
        CapabilityRouteHint::Coding => &["delegate", "shared_board", "handover", "tool_search"],
        CapabilityRouteHint::Communication => &[
            "delegate",
            "mailer",
            "notifier",
            "shared_board",
            "tool_search",
        ],
        CapabilityRouteHint::Memory => &[
            "delegate",
            "shared_board",
            "search_history",
            "knowledge_search",
            "remember_this",
            "tiered_search",
            "manage_facts",
            "tool_search",
        ],
        CapabilityRouteHint::CapabilityGap => &["delegate", "shared_board", "tool_search"],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_labels_stay_stable() {
        assert_eq!(
            capability_route_hint_label(CapabilityRouteHint::RuntimeSurface),
            "runtime_surface"
        );
        assert_eq!(
            capability_route_debug_label(CapabilityRouteHint::DocumentUnderstanding),
            "document hard route"
        );
    }

    #[test]
    fn route_execution_requirements_are_stable() {
        assert!(capability_route_requires_real_tool_call(
            CapabilityRouteHint::DocumentUnderstanding
        ));
        assert!(!capability_route_requires_real_tool_call(
            CapabilityRouteHint::General
        ));
        assert!(capability_route_prefers_direct_tool_surface(
            CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::LatestInfoLookup)
        ));
        assert!(capability_route_requires_source_fetch(
            CapabilityRouteHint::RealtimeLookup(RealtimeLookupKind::WeatherLookup)
        ));
    }

    #[test]
    fn route_prompt_and_tool_preferences_are_stable() {
        assert!(capability_route_should_inject_system_message(
            CapabilityRouteHint::Memory
        ));
        assert!(capability_route_should_inject_system_message(
            CapabilityRouteHint::Writing
        ));
        assert!(!capability_route_should_inject_system_message(
            CapabilityRouteHint::DocumentUnderstanding
        ));
        assert_eq!(
            capability_route_preferred_tool_names(CapabilityRouteHint::Coding),
            &["delegate", "shared_board", "handover", "tool_search"]
        );
        assert!(capability_route_preferred_tool_names(CapabilityRouteHint::General).is_empty());
    }
}

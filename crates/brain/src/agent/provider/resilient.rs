use async_trait::async_trait;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::agent::provider::{ChatRequest, Provider, ProviderMetadata};
use crate::agent::streaming::StreamingResponse;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRouteDecision {
    pub route: String,
    pub state: String,
    pub provider: String,
    pub fallback_provider: Option<String>,
    pub reason: String,
}

type RouteObserver = Arc<dyn Fn(ProviderRouteDecision) + Send + Sync>;

/// Configuration for the Circuit Breaker
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Failure threshold before opening the circuit
    pub failure_threshold: u32,
    /// Duration to wait before attempting recovery (Half-Open)
    pub reset_timeout: Duration,
    /// Maximum request duration before considering it a failure (Timeout)
    pub request_timeout: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 3,
            reset_timeout: Duration::from_secs(60),
            request_timeout: Duration::from_secs(30),
        }
    }
}

/// State of the Circuit Breaker
#[derive(Debug, Clone, PartialEq)]
enum CircuitState {
    Closed,   // Normal operation
    Open,     // Failing, use fallback
    HalfOpen, // Recovering, test primary
}

/// A provider that wraps a primary and a fallback provider with circuit breaker logic
pub struct ResilientProvider<P: Provider, F: Provider> {
    primary: Arc<P>,
    fallback: Arc<F>,
    config: CircuitBreakerConfig,
    state: Arc<Mutex<CircuitStateInternal>>,
    observer: Option<RouteObserver>,
}

struct CircuitStateInternal {
    state: CircuitState,
    failures: u32,
    last_failure_time: Option<Instant>,
}

impl<P: Provider, F: Provider> ResilientProvider<P, F> {
    pub fn new(primary: P, fallback: F, config: CircuitBreakerConfig) -> Self {
        Self {
            primary: Arc::new(primary),
            fallback: Arc::new(fallback),
            config,
            state: Arc::new(Mutex::new(CircuitStateInternal {
                state: CircuitState::Closed,
                failures: 0,
                last_failure_time: None,
            })),
            observer: None,
        }
    }

    pub fn with_observer(mut self, observer: RouteObserver) -> Self {
        self.observer = Some(observer);
        self
    }

    fn emit_route_decision(
        &self,
        route: impl Into<String>,
        state: impl Into<String>,
        provider: impl Into<String>,
        reason: impl Into<String>,
    ) {
        if let Some(observer) = &self.observer {
            observer(ProviderRouteDecision {
                route: route.into(),
                state: state.into(),
                provider: provider.into(),
                fallback_provider: Some(self.fallback.name().to_string()),
                reason: reason.into(),
            });
        }
    }

    async fn check_state(&self) -> CircuitState {
        let mut router = self.state.lock().await;

        match router.state {
            CircuitState::Open => {
                if let Some(last_failure) = router.last_failure_time {
                    if last_failure.elapsed() > self.config.reset_timeout {
                        info!("Circuit Breaker: Reset timeout passed, switching to Half-Open");
                        router.state = CircuitState::HalfOpen;
                        return CircuitState::HalfOpen;
                    }
                }
                CircuitState::Open
            }
            _ => router.state.clone(),
        }
    }

    async fn report_success(&self) {
        let mut router = self.state.lock().await;
        if router.state == CircuitState::HalfOpen {
            info!("Circuit Breaker: Half-Open success, closing circuit (Back to Normal)");
            router.state = CircuitState::Closed;
            router.failures = 0;
            router.last_failure_time = None;
        } else if router.state == CircuitState::Closed {
            router.failures = 0;
        }
    }

    async fn report_failure(&self) {
        let mut router = self.state.lock().await;
        router.failures += 1;
        router.last_failure_time = Some(Instant::now());

        if router.state == CircuitState::Closed && router.failures >= self.config.failure_threshold
        {
            warn!("Circuit Breaker: Failure threshold reached, OPENING circuit (Switching to Fallback)");
            router.state = CircuitState::Open;
        } else if router.state == CircuitState::HalfOpen {
            warn!("Circuit Breaker: Half-Open failure, re-opening circuit");
            router.state = CircuitState::Open;
        }
    }
}

#[async_trait]
impl<P: Provider, F: Provider> Provider for ResilientProvider<P, F> {
    fn name(&self) -> &str {
        "resilient-provider"
    }

    async fn stream_completion(
        &self,
        request: ChatRequest,
    ) -> benshu_infra::error::Result<StreamingResponse> {
        let state = self.check_state().await;

        // Decide which provider to use
        let use_primary = match state {
            CircuitState::Closed => true,
            CircuitState::HalfOpen => true, // Try one request
            CircuitState::Open => false,
        };

        if use_primary {
            self.emit_route_decision(
                "primary",
                format!("{state:?}").to_lowercase(),
                self.primary.name(),
                "primary path selected",
            );
            // Attempt Primary with Timeout
            match tokio::time::timeout(
                self.config.request_timeout,
                self.primary.stream_completion(request.clone()),
            )
            .await
            {
                Ok(Ok(response)) => {
                    self.report_success().await;
                    return Ok(response);
                }
                Ok(Err(e)) => {
                    warn!("Primary provider failed: {}", e);
                    self.emit_route_decision(
                        "fallback",
                        format!("{state:?}").to_lowercase(),
                        self.primary.name(),
                        format!("primary error: {}", e),
                    );
                    self.report_failure().await;
                    // Fallthrough to fallback
                }
                Err(_) => {
                    warn!(
                        "Primary provider timed out (> {:?})",
                        self.config.request_timeout
                    );
                    self.emit_route_decision(
                        "fallback",
                        format!("{state:?}").to_lowercase(),
                        self.primary.name(),
                        format!("primary timeout > {:?}", self.config.request_timeout),
                    );
                    self.report_failure().await;
                    // Fallthrough to fallback
                }
            }
        }

        // Fallback Logic
        info!("Using Fallback Provider: {}", self.fallback.name());
        self.emit_route_decision(
            "fallback",
            format!("{state:?}").to_lowercase(),
            self.fallback.name(),
            "fallback path activated",
        );
        self.fallback.stream_completion(request).await
    }

    fn metadata() -> ProviderMetadata
    where
        Self: Sized,
    {
        let mut meta = P::metadata();
        meta.capabilities
            .push("runtime:fallback-enabled".to_string());
        meta.capabilities
            .push("runtime:resilient-provider".to_string());
        meta
    }

    async fn get_dynamic_metadata(&self) -> benshu_infra::error::Result<ProviderMetadata> {
        // Prefer primary dynamic metadata if not open
        let state = self.check_state().await;
        if state != CircuitState::Open {
            if let Ok(meta) = self.primary.get_dynamic_metadata().await {
                let mut meta = meta;
                meta.capabilities
                    .push("runtime:fallback-enabled".to_string());
                meta.capabilities.push(format!(
                    "runtime:fallback-provider:{}",
                    self.fallback.name()
                ));
                meta.capabilities
                    .push("runtime:resilient-provider".to_string());
                return Ok(meta);
            }
        }
        let mut meta = self.fallback.get_dynamic_metadata().await?;
        meta.capabilities
            .push("runtime:fallback-enabled".to_string());
        meta.capabilities.push(format!(
            "runtime:fallback-provider:{}",
            self.fallback.name()
        ));
        meta.capabilities
            .push("runtime:resilient-provider".to_string());
        Ok(meta)
    }

    fn get_context_window(&self, model: &str) -> usize {
        self.primary.get_context_window(model)
    }

    fn trim_messages(
        &self,
        messages: Vec<crate::agent::message::Message>,
        model: &str,
    ) -> Vec<crate::agent::message::Message> {
        self.primary.trim_messages(messages, model)
    }

    fn is_local(&self) -> bool {
        self.primary.is_local()
    }

    fn tool_contract_mode(&self) -> &'static str {
        self.primary.tool_contract_mode()
    }

    fn mainline_stability(&self) -> &'static str {
        self.primary.mainline_stability()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::provider::{ChatRequest, Provider, ProviderMetadata};
    use crate::agent::streaming::MockStreamBuilder;
    use std::sync::Mutex as StdMutex;

    #[derive(Clone)]
    struct MockProvider {
        name: &'static str,
        fail: bool,
    }

    #[async_trait]
    impl Provider for MockProvider {
        async fn stream_completion(
            &self,
            _request: ChatRequest,
        ) -> benshu_infra::error::Result<StreamingResponse> {
            if self.fail {
                Err(benshu_infra::error::Error::Agent(format!(
                    "{} failed",
                    self.name
                )))
            } else {
                Ok(MockStreamBuilder::new().message("ok").done().build())
            }
        }

        fn name(&self) -> &str {
            self.name
        }

        fn metadata() -> ProviderMetadata
        where
            Self: Sized,
        {
            ProviderMetadata {
                id: "mock".to_string(),
                name: "mock".to_string(),
                description: "mock".to_string(),
                icon: "m".to_string(),
                fields: vec![],
                capabilities: vec![],
                preferred_models: vec![],
            }
        }
    }

    #[tokio::test]
    async fn resilient_provider_emits_failover_decision() {
        let decisions = Arc::new(StdMutex::new(Vec::<ProviderRouteDecision>::new()));
        let sink = decisions.clone();
        let provider = ResilientProvider::new(
            MockProvider {
                name: "primary",
                fail: true,
            },
            MockProvider {
                name: "fallback",
                fail: false,
            },
            CircuitBreakerConfig::default(),
        )
        .with_observer(Arc::new(move |decision| {
            sink.lock().expect("decision log").push(decision);
        }));

        let _ = provider
            .stream_completion(ChatRequest {
                model: "mock".to_string(),
                ..Default::default()
            })
            .await
            .expect("fallback should succeed");

        let logged = decisions.lock().expect("decision log");
        assert!(logged.iter().any(|item| item.route == "fallback"));
        assert!(logged
            .iter()
            .any(|item| item.reason.contains("primary error")));
    }
}

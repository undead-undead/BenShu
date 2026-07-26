use crate::agent::context::ContextInjector;
use crate::error::Result;
use crate::skills::tool::Tool;
use std::sync::Arc;

/// Phase 5.4: Component Registry for decoupled Agent assembly
pub trait AgentComponent: Send + Sync {
    /// The unique name of the component (e.g., "security", "memory", "evolution")
    fn name(&self) -> &'static str;

    /// Stage 1: Registration - The component adds its required tools/injectors to the builder's pool.
    fn register(&self, context: &mut ComponentContext) -> Result<()>;

    /// Stage 3: Linking - The component wires itself and its dependencies after the Agent is instantiated.
    fn link(
        &self,
        _tools: &crate::skills::tool::ToolSet,
        _memory: Option<&Arc<dyn crate::agent::memory::Memory>>,
    ) -> Result<()> {
        Ok(())
    }
}

/// A context for a component to register its contributions.
pub struct ComponentContext {
    pub tools: Vec<Box<dyn Tool>>,
    pub injectors: Vec<Arc<dyn ContextInjector>>,
}

impl ComponentContext {
    pub fn new() -> Self {
        Self {
            tools: Vec::new(),
            injectors: Vec::new(),
        }
    }
}

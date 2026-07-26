/// Declares which families of memory behavior a concrete backend can serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MemoryCapabilities {
    pub episodic_messages: bool,
    pub sessions: bool,
    pub facts: bool,
    pub search: bool,
    pub knowledge_store: bool,
    pub experiences: bool,
    pub metadata: bool,
}

impl MemoryCapabilities {
    pub const fn episodic_only() -> Self {
        Self {
            episodic_messages: true,
            sessions: true,
            facts: true,
            search: false,
            knowledge_store: false,
            experiences: false,
            metadata: true,
        }
    }

    pub const fn full() -> Self {
        Self {
            episodic_messages: true,
            sessions: true,
            facts: true,
            search: true,
            knowledge_store: true,
            experiences: true,
            metadata: true,
        }
    }

    pub const fn read_only(inner: Self) -> Self {
        inner
    }
}

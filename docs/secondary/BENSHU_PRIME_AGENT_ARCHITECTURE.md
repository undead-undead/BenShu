# BenShu Prime Agent Architecture

> Platform Positioning: `Windows Native` is BenShu's formal product path and primary host platform; `WSL / WSL2 / Linux` routes are development/testing lanes for fast iteration and must not be presented as the default product deployment path.

## 1. Purpose

This document defines the core architectural stance of BenShu as it evolves toward a personal "Jarvis" system.

The goal is to remove ambiguity around one question:

- Is BenShu a collection of equal agents?
- Or is BenShu a single user-facing intelligence that orchestrates many specialist agents?

This document makes the answer explicit.

---

## 2. One-Line Position

`BenShu is the single user-facing prime agent that orchestrates a distributed A2A specialist network behind one coherent identity.`

In plain terms:

- The user talks to `BenShu`
- `BenShu` owns the task
- `BenShu` decides when to delegate
- Specialist agents operate behind the scenes through `A2A`
- Memory, governance, approvals, and final responsibility remain with `BenShu`

---

## 3. What BenShu Is

BenShu is not merely:

- a chat UI
- a model router
- a multi-agent playground
- a tool execution shell

BenShu is intended to be:

- a persistent personal AI executive
- a unified runtime for reasoning, memory, action, and recovery
- a secure local-first operating layer for personal AI assistance
- a prime coordinator for specialist agent labor

The user should experience one coherent assistant, not a swarm of competing personalities.

---

## 4. Frontstage vs Backstage

### 4.1 Frontstage: Single Persona

For the user, the system should appear as one consistent entity:

- one name: `BenShu`
- one conversation surface
- one task ownership model
- one memory identity
- one approval authority

The user should not need to understand which sub-agent handled a step unless that detail is useful for trust, debugging, or governance.

### 4.2 Backstage: Multi-Agent Execution

Internally, the system should be explicitly multi-agent:

- planner / dispatcher agents
- coding agents
- research agents
- retrieval agents
- multimodal specialists
- OCR / PDF / document agents
- system operation agents
- memory maintenance agents
- communication / swarm agents

These are implementation components, not primary user identities.

---

## 5. Role of A2A

`A2A` is an internal coordination bus, not the primary user interface.

Its purpose is to support:

- task delegation
- capability handoff
- specialist coordination
- message passing
- telemetry
- distributed execution

This means:

- users should not be required to manually choose target agents in normal operation
- specialist-to-specialist collaboration should be routed through `A2A`
- prime-agent delegation should be explicit, observable, and governable

`A2A` is therefore a core internal substrate, not the product's front-facing mental model.

---

## 6. Ownership Model

The most important design rule is ownership clarity.

### 6.1 Task Ownership

All user-originated tasks belong to `BenShu`.

Even when work is delegated:

- the prime agent retains responsibility
- specialists do not become independent task owners
- final synthesis and delivery return to the prime agent

### 6.2 Memory Ownership

Personal memory belongs to `BenShu`, not to specialists.

This includes:

- user profile
- user preferences
- long-term goals
- trust boundaries
- work habits
- important documents and recurring contexts
- task history and outcomes

Specialists may have temporary working memory or scoped operational memory, but the persistent personal memory graph belongs to the prime agent.

### 6.3 Governance Ownership

Approvals, safety posture, and trust boundaries belong to `BenShu`.

Examples:

- tool approval policy
- trusted workspaces
- network permissions
- escalation thresholds
- privacy boundaries
- action auditing

Specialists may inherit a restricted governance envelope, but they should not silently redefine the user's safety model.

### 6.4 Inbox Ownership

All obligations should eventually roll up into a prime-agent inbox.

That includes:

- pending tasks
- blocked tasks
- failed tasks
- waiting-for-approval tasks
- scheduled follow-ups
- reminders
- partially completed missions

This inbox is a user-facing responsibility surface, not just an internal queue.

---

## 7. Prime Agent Responsibilities

The prime agent should be responsible for:

1. Understanding user intent
2. Converting user intent into a task structure
3. Choosing whether to answer directly or delegate
4. Selecting specialists based on capability and risk
5. Allocating budget, priority, and time
6. Merging results back into a coherent response
7. Updating long-term memory when appropriate
8. Requesting approval when actions cross trust boundaries
9. Owning failure recovery and next-step communication

If a task fails, the user should feel that `BenShu` failed and recovered, not that some invisible background actor disappeared.

---

## 8. Specialist Agent Responsibilities

Specialist agents should be narrow, role-based execution components.

They should typically own:

- local reasoning within a bounded domain
- tool usage for their specialty
- short-lived execution memory
- structured outputs for the prime agent

They should typically not own:

- the user relationship
- long-term personal memory
- final approval authority
- the final voice of the system

This keeps the system coherent and prevents personality fragmentation.

---

## 9. Why This Architecture Fits BenShu

This architecture aligns with the strengths BenShu already has:

- `brain` already supports delegation, governance, memory, and runtime control
- `comm` already points toward distributed specialist coordination
- `engram` already points toward durable long-term memory
- `runtimes` and `security` already provide execution-grade substrate
- `panel + gateway` already point toward a unified control surface

In other words:

- BenShu already has many of the right building blocks
- what is needed now is architectural consolidation, not a complete conceptual reset

---

## 10. Product Implications

If this document is accepted, several product decisions become clear.

### 10.1 The UI Should Reflect Prime-Agent Ownership

The UI should emphasize:

- one main conversation
- one task inbox
- one memory and history surface
- one approval center
- one system status surface

It may expose specialist activity, but only as supporting detail.

### 10.2 README and Docs Should Use Prime-Agent Language

The project should describe itself less as:

- an agent ecosystem
- a swarm of equal agents
- a collection of modules

And more as:

- a single personal AI executive
- backed by a governed A2A specialist network
- running on a secure local-first runtime

### 10.3 Feature Priorities Change

Under this architecture, the most important follow-up work becomes:

- unified capability routing
- task inbox and obligation tracking
- memory ownership and memory surfaces
- approval and governance surfaces
- specialist profiles and delegation rules
- A2A closure for specialist collaboration

This also means some capabilities should be deprioritized as primary marketing claims:

- raw swarm complexity by itself
- hyperbolic navigation as a front-page promise
- autopilot features that are not yet fully productized

---

## 11. Near-Term Architecture Consequences

The following follow-up documents and workstreams should align to this stance:

- `BRAIN_CAPABILITY_PRIORITY_AND_INTEGRATION_AUDIT.md`
  - should treat `brain` as the runtime core of the prime agent, not merely a single-agent library
- `comm`
  - should be framed as the internal A2A substrate for specialist collaboration
- multimodal routing
  - should become a prime-agent capability-routing concern, not a loose collection of tools
- memory integration
  - should center on prime-agent memory ownership with scoped specialist access
- panel
  - should evolve from control console toward prime-agent workspace

---

## 12. Non-Goals

This architecture does not imply:

- that every internal component must be anthropomorphized as an agent
- that every action requires delegation
- that users should micromanage the swarm
- that all capabilities must be implemented through `comm`

Many subsystems should remain plain runtimes or services:

- OCR backends
- PDF parsers
- embedding engines
- execution sandboxes
- telemetry pipelines

The system should use agent abstraction where it improves reasoning, delegation, and governance, not everywhere by default.

---

## 13. Final Principle

The core design principle is:

`One visible intelligence, many invisible workers, one accountable owner.`

That owner is `BenShu`.

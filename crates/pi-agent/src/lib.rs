//! `pi-agent` — Agent runtime with tool calling.
//!
//! Rust port of `@earendil-works/pi-agent-core`. Provides:
//! - [`AgentTool`] / [`AgentToolResult`] for defining tools
//! - [`AgentConfig`] for configuring a run, plus a [`PermissionPolicy`] hook
//! - [`run_agent`] / [`run_agent_with_history`] — the agent loop
//! - Builtin tools under [`tools`]

pub mod agent_loop;
pub mod agent_session;
pub mod error;
pub mod tools;
pub mod types;

pub use agent_loop::{run_agent, run_agent_with_history, AgentRun};
pub use agent_session::AgentSession;
pub use error::{AgentError, Result};
pub use types::{
    tool_def, AgentConfig, AgentEvent, AgentSessionState, AgentTool, AgentToolResult,
    AllowAllPolicy, BeforeToolCall, BeforeToolCallResult, PermissionDecision, PermissionPolicy,
    QueueMode, RuntimeLimits, SessionPhase, ToolCallHook,
};

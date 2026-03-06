#![allow(private_interfaces)]
//! Uniform response builders for all Claude Code hook events.
//!
//! Each event type has its own response struct exposing only valid
//! combinations. The compiler rejects invalid usage — no runtime checks.
//!
//! All types implement HookOutput::to_json() which serializes to the
//! correct Anthropic wire format for that event.
//!
//! Our language:          Anthropic wire format:
//!   Allow                  permissionDecision: "allow" (PreToolUse)
//!                          omit decision field (PostToolUse, Stop, etc.)
//!   Deny                   permissionDecision: "deny" (PreToolUse)
//!                          decision: "block" (PostToolUse, Stop, etc.)
//!                          decision.behavior: "deny" (PermissionRequest)
//!   Ask                    permissionDecision: "ask" (PreToolUse only)
//!   context                additionalContext (location varies by event)
//!   reason                 permissionDecisionReason / reason (varies)
//!   updated_input          updatedInput (PreToolUse, PermissionRequest)

use serde_json::Value;

/// Trait implemented by all event response types.
pub trait HookOutput {
    fn to_json(&self) -> String;
}

// ── Universal fields ──────────────────────────────────────────────
// Available on all events. Applied as top-level fields in the JSON.

pub(crate) struct Universal {
    suppress_output: bool,
    system_message: Option<String>,
    stop_session: bool,
    stop_reason: Option<String>,
}

impl Default for Universal {
    fn default() -> Self {
        Self {
            suppress_output: false,
            system_message: None,
            stop_session: false,
            stop_reason: None,
        }
    }
}

impl Universal {
    fn apply(&self, obj: &mut serde_json::Map<String, Value>) {
        if self.suppress_output {
            obj.insert("suppressOutput".into(), Value::Bool(true));
        }
        if let Some(ref msg) = self.system_message {
            obj.insert("systemMessage".into(), Value::String(msg.clone()));
        }
        if self.stop_session {
            obj.insert("continue".into(), Value::Bool(false));
            if let Some(ref reason) = self.stop_reason {
                obj.insert("stopReason".into(), Value::String(reason.clone()));
            }
        }
    }
}

// ── Shared builder trait for universal fields ─────────────────────

/// Methods available on all response builders for universal fields.
#[allow(private_interfaces)]
pub trait WithUniversal: Sized {
    #[doc(hidden)]
    fn universal_mut(&mut self) -> &mut Universal;

    fn suppress_output(mut self) -> Self {
        self.universal_mut().suppress_output = true;
        self
    }

    fn system_message(mut self, msg: impl Into<String>) -> Self {
        self.universal_mut().system_message = Some(msg.into());
        self
    }

    fn stop_session(mut self, reason: impl Into<String>) -> Self {
        self.universal_mut().stop_session = true;
        self.universal_mut().stop_reason = Some(reason.into());
        self
    }
}

// ═════════════════════════════════════════════════════════════════
// PreToolUse
//   Decision: Allow | Deny | Ask  (inside hookSpecificOutput)
//   Fields:   context, updated_input, reason
// ═════════════════════════════════════════════════════════════════

enum PreToolUseDecision {
    Allow,
    Deny,
    Ask,
}

pub struct PreToolUseResponse {
    universal: Universal,
    decision: PreToolUseDecision,
    reason: Option<String>,
    context: Option<String>,
    updated_input: Option<Value>,
}

impl PreToolUseResponse {
    pub fn allow() -> Self {
        Self {
            universal: Universal::default(),
            decision: PreToolUseDecision::Allow,
            reason: None,
            context: None,
            updated_input: None,
        }
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            universal: Universal::default(),
            decision: PreToolUseDecision::Deny,
            reason: Some(reason.into()),
            context: None,
            updated_input: None,
        }
    }

    pub fn ask(reason: impl Into<String>) -> Self {
        Self {
            universal: Universal::default(),
            decision: PreToolUseDecision::Ask,
            reason: Some(reason.into()),
            context: None,
            updated_input: None,
        }
    }

    pub fn with_context(mut self, ctx: impl Into<String>) -> Self {
        self.context = Some(ctx.into());
        self
    }

    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    pub fn with_updated_input(mut self, input: Value) -> Self {
        self.updated_input = Some(input);
        self
    }
}

impl WithUniversal for PreToolUseResponse {
    fn universal_mut(&mut self) -> &mut Universal { &mut self.universal }
}

impl HookOutput for PreToolUseResponse {
    fn to_json(&self) -> String {
        let mut root = serde_json::Map::new();
        self.universal.apply(&mut root);

        let mut hso = serde_json::Map::new();
        hso.insert("hookEventName".into(), Value::String("PreToolUse".into()));

        let decision_str = match self.decision {
            PreToolUseDecision::Allow => "allow",
            PreToolUseDecision::Deny => "deny",
            PreToolUseDecision::Ask => "ask",
        };
        hso.insert("permissionDecision".into(), Value::String(decision_str.into()));

        if let Some(ref reason) = self.reason {
            hso.insert("permissionDecisionReason".into(), Value::String(reason.clone()));
        }
        if let Some(ref ctx) = self.context {
            hso.insert("additionalContext".into(), Value::String(ctx.clone()));
        }
        if let Some(ref input) = self.updated_input {
            hso.insert("updatedInput".into(), input.clone());
        }

        root.insert("hookSpecificOutput".into(), Value::Object(hso));
        serde_json::to_string(&root).unwrap_or_default()
    }
}

// ═════════════════════════════════════════════════════════════════
// PermissionRequest
//   Decision: Allow | Deny  (inside hookSpecificOutput.decision)
//   Fields:   updated_input (allow), message+interrupt (deny)
// ═════════════════════════════════════════════════════════════════

enum PermissionDecisionKind {
    Allow {
        updated_input: Option<Value>,
        updated_permissions: Option<Value>,
    },
    Deny {
        message: Option<String>,
        interrupt: bool,
    },
}

pub struct PermissionRequestResponse {
    universal: Universal,
    decision: PermissionDecisionKind,
}

impl PermissionRequestResponse {
    pub fn allow() -> Self {
        Self {
            universal: Universal::default(),
            decision: PermissionDecisionKind::Allow {
                updated_input: None,
                updated_permissions: None,
            },
        }
    }

    pub fn deny() -> Self {
        Self {
            universal: Universal::default(),
            decision: PermissionDecisionKind::Deny {
                message: None,
                interrupt: false,
            },
        }
    }

    pub fn with_updated_input(mut self, input: Value) -> Self {
        if let PermissionDecisionKind::Allow { ref mut updated_input, .. } = self.decision {
            *updated_input = Some(input);
        }
        self
    }

    pub fn with_updated_permissions(mut self, perms: Value) -> Self {
        if let PermissionDecisionKind::Allow { ref mut updated_permissions, .. } = self.decision {
            *updated_permissions = Some(perms);
        }
        self
    }

    pub fn with_message(mut self, msg: impl Into<String>) -> Self {
        if let PermissionDecisionKind::Deny { ref mut message, .. } = self.decision {
            *message = Some(msg.into());
        }
        self
    }

    pub fn with_interrupt(mut self) -> Self {
        if let PermissionDecisionKind::Deny { ref mut interrupt, .. } = self.decision {
            *interrupt = true;
        }
        self
    }
}

impl WithUniversal for PermissionRequestResponse {
    fn universal_mut(&mut self) -> &mut Universal { &mut self.universal }
}

impl HookOutput for PermissionRequestResponse {
    fn to_json(&self) -> String {
        let mut root = serde_json::Map::new();
        self.universal.apply(&mut root);

        let mut hso = serde_json::Map::new();
        hso.insert("hookEventName".into(), Value::String("PermissionRequest".into()));

        let mut decision_obj = serde_json::Map::new();
        match &self.decision {
            PermissionDecisionKind::Allow { updated_input, updated_permissions } => {
                decision_obj.insert("behavior".into(), Value::String("allow".into()));
                if let Some(ref input) = updated_input {
                    decision_obj.insert("updatedInput".into(), input.clone());
                }
                if let Some(ref perms) = updated_permissions {
                    decision_obj.insert("updatedPermissions".into(), perms.clone());
                }
            }
            PermissionDecisionKind::Deny { message, interrupt } => {
                decision_obj.insert("behavior".into(), Value::String("deny".into()));
                if let Some(ref msg) = message {
                    decision_obj.insert("message".into(), Value::String(msg.clone()));
                }
                if *interrupt {
                    decision_obj.insert("interrupt".into(), Value::Bool(true));
                }
            }
        }
        hso.insert("decision".into(), Value::Object(decision_obj));

        root.insert("hookSpecificOutput".into(), Value::Object(hso));
        serde_json::to_string(&root).unwrap_or_default()
    }
}

// ═════════════════════════════════════════════════════════════════
// PostToolUse
//   Decision: Allow | Deny(block)  (top-level decision)
//   Fields:   context, reason
// ═════════════════════════════════════════════════════════════════

pub struct PostToolUseResponse {
    universal: Universal,
    deny: bool,
    reason: Option<String>,
    context: Option<String>,
}

impl PostToolUseResponse {
    pub fn allow() -> Self {
        Self {
            universal: Universal::default(),
            deny: false,
            reason: None,
            context: None,
        }
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            universal: Universal::default(),
            deny: true,
            reason: Some(reason.into()),
            context: None,
        }
    }

    pub fn with_context(mut self, ctx: impl Into<String>) -> Self {
        self.context = Some(ctx.into());
        self
    }
}

impl WithUniversal for PostToolUseResponse {
    fn universal_mut(&mut self) -> &mut Universal { &mut self.universal }
}

impl HookOutput for PostToolUseResponse {
    fn to_json(&self) -> String {
        let mut root = serde_json::Map::new();
        self.universal.apply(&mut root);

        if self.deny {
            root.insert("decision".into(), Value::String("block".into()));
            if let Some(ref reason) = self.reason {
                root.insert("reason".into(), Value::String(reason.clone()));
            }
        }

        if let Some(ref ctx) = self.context {
            let mut hso = serde_json::Map::new();
            hso.insert("hookEventName".into(), Value::String("PostToolUse".into()));
            hso.insert("additionalContext".into(), Value::String(ctx.clone()));
            root.insert("hookSpecificOutput".into(), Value::Object(hso));
        }

        serde_json::to_string(&root).unwrap_or_default()
    }
}

// ═════════════════════════════════════════════════════════════════
// PostToolUseFailure
//   Decision: none (tool already failed)
//   Fields:   context
// ═════════════════════════════════════════════════════════════════

pub struct PostToolUseFailureResponse {
    universal: Universal,
    context: Option<String>,
}

impl PostToolUseFailureResponse {
    pub fn allow() -> Self {
        Self { universal: Universal::default(), context: None }
    }

    pub fn with_context(mut self, ctx: impl Into<String>) -> Self {
        self.context = Some(ctx.into());
        self
    }
}

impl WithUniversal for PostToolUseFailureResponse {
    fn universal_mut(&mut self) -> &mut Universal { &mut self.universal }
}

impl HookOutput for PostToolUseFailureResponse {
    fn to_json(&self) -> String {
        let mut root = serde_json::Map::new();
        self.universal.apply(&mut root);

        if let Some(ref ctx) = self.context {
            let mut hso = serde_json::Map::new();
            hso.insert("hookEventName".into(), Value::String("PostToolUseFailure".into()));
            hso.insert("additionalContext".into(), Value::String(ctx.clone()));
            root.insert("hookSpecificOutput".into(), Value::Object(hso));
        }

        serde_json::to_string(&root).unwrap_or_default()
    }
}

// ═════════════════════════════════════════════════════════════════
// UserPromptSubmit
//   Decision: Allow | Deny(block)  (top-level decision)
//   Fields:   context, reason
// ═════════════════════════════════════════════════════════════════

pub struct UserPromptSubmitResponse {
    universal: Universal,
    deny: bool,
    reason: Option<String>,
    context: Option<String>,
}

impl UserPromptSubmitResponse {
    pub fn allow() -> Self {
        Self {
            universal: Universal::default(),
            deny: false,
            reason: None,
            context: None,
        }
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            universal: Universal::default(),
            deny: true,
            reason: Some(reason.into()),
            context: None,
        }
    }

    pub fn with_context(mut self, ctx: impl Into<String>) -> Self {
        self.context = Some(ctx.into());
        self
    }
}

impl WithUniversal for UserPromptSubmitResponse {
    fn universal_mut(&mut self) -> &mut Universal { &mut self.universal }
}

impl HookOutput for UserPromptSubmitResponse {
    fn to_json(&self) -> String {
        let mut root = serde_json::Map::new();
        self.universal.apply(&mut root);

        if self.deny {
            root.insert("decision".into(), Value::String("block".into()));
            if let Some(ref reason) = self.reason {
                root.insert("reason".into(), Value::String(reason.clone()));
            }
        }

        if let Some(ref ctx) = self.context {
            let mut hso = serde_json::Map::new();
            hso.insert("hookEventName".into(), Value::String("UserPromptSubmit".into()));
            hso.insert("additionalContext".into(), Value::String(ctx.clone()));
            root.insert("hookSpecificOutput".into(), Value::Object(hso));
        }

        serde_json::to_string(&root).unwrap_or_default()
    }
}

// ═════════════════════════════════════════════════════════════════
// Stop
//   Decision: Allow | Deny(block)  (top-level decision)
//   Fields:   reason only
// ═════════════════════════════════════════════════════════════════

pub struct StopResponse {
    universal: Universal,
    deny: bool,
    reason: Option<String>,
}

impl StopResponse {
    pub fn allow() -> Self {
        Self { universal: Universal::default(), deny: false, reason: None }
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            universal: Universal::default(),
            deny: true,
            reason: Some(reason.into()),
        }
    }
}

impl WithUniversal for StopResponse {
    fn universal_mut(&mut self) -> &mut Universal { &mut self.universal }
}

impl HookOutput for StopResponse {
    fn to_json(&self) -> String {
        let mut root = serde_json::Map::new();
        self.universal.apply(&mut root);

        if self.deny {
            root.insert("decision".into(), Value::String("block".into()));
            if let Some(ref reason) = self.reason {
                root.insert("reason".into(), Value::String(reason.clone()));
            }
        }

        serde_json::to_string(&root).unwrap_or_default()
    }
}

// ═════════════════════════════════════════════════════════════════
// SubagentStop — same contract as Stop
// ═════════════════════════════════════════════════════════════════

pub struct SubagentStopResponse {
    universal: Universal,
    deny: bool,
    reason: Option<String>,
}

impl SubagentStopResponse {
    pub fn allow() -> Self {
        Self { universal: Universal::default(), deny: false, reason: None }
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            universal: Universal::default(),
            deny: true,
            reason: Some(reason.into()),
        }
    }
}

impl WithUniversal for SubagentStopResponse {
    fn universal_mut(&mut self) -> &mut Universal { &mut self.universal }
}

impl HookOutput for SubagentStopResponse {
    fn to_json(&self) -> String {
        let mut root = serde_json::Map::new();
        self.universal.apply(&mut root);

        if self.deny {
            root.insert("decision".into(), Value::String("block".into()));
            if let Some(ref reason) = self.reason {
                root.insert("reason".into(), Value::String(reason.clone()));
            }
        }

        serde_json::to_string(&root).unwrap_or_default()
    }
}

// ═════════════════════════════════════════════════════════════════
// ConfigChange
//   Decision: Allow | Deny(block)  (top-level decision)
//   Fields:   reason only
// ═════════════════════════════════════════════════════════════════

pub struct ConfigChangeResponse {
    universal: Universal,
    deny: bool,
    reason: Option<String>,
}

impl ConfigChangeResponse {
    pub fn allow() -> Self {
        Self { universal: Universal::default(), deny: false, reason: None }
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            universal: Universal::default(),
            deny: true,
            reason: Some(reason.into()),
        }
    }
}

impl WithUniversal for ConfigChangeResponse {
    fn universal_mut(&mut self) -> &mut Universal { &mut self.universal }
}

impl HookOutput for ConfigChangeResponse {
    fn to_json(&self) -> String {
        let mut root = serde_json::Map::new();
        self.universal.apply(&mut root);

        if self.deny {
            root.insert("decision".into(), Value::String("block".into()));
            if let Some(ref reason) = self.reason {
                root.insert("reason".into(), Value::String(reason.clone()));
            }
        }

        serde_json::to_string(&root).unwrap_or_default()
    }
}

// ═════════════════════════════════════════════════════════════════
// SessionStart — context injection only
// ═════════════════════════════════════════════════════════════════

pub struct SessionStartResponse {
    universal: Universal,
    context: Option<String>,
}

impl SessionStartResponse {
    pub fn allow() -> Self {
        Self { universal: Universal::default(), context: None }
    }

    pub fn with_context(mut self, ctx: impl Into<String>) -> Self {
        self.context = Some(ctx.into());
        self
    }
}

impl WithUniversal for SessionStartResponse {
    fn universal_mut(&mut self) -> &mut Universal { &mut self.universal }
}

impl HookOutput for SessionStartResponse {
    fn to_json(&self) -> String {
        let mut root = serde_json::Map::new();
        self.universal.apply(&mut root);

        if let Some(ref ctx) = self.context {
            let mut hso = serde_json::Map::new();
            hso.insert("hookEventName".into(), Value::String("SessionStart".into()));
            hso.insert("additionalContext".into(), Value::String(ctx.clone()));
            root.insert("hookSpecificOutput".into(), Value::Object(hso));
        }

        serde_json::to_string(&root).unwrap_or_default()
    }
}

// ═════════════════════════════════════════════════════════════════
// SubagentStart — context injection only
// ═════════════════════════════════════════════════════════════════

pub struct SubagentStartResponse {
    universal: Universal,
    context: Option<String>,
}

impl SubagentStartResponse {
    pub fn allow() -> Self {
        Self { universal: Universal::default(), context: None }
    }

    pub fn with_context(mut self, ctx: impl Into<String>) -> Self {
        self.context = Some(ctx.into());
        self
    }
}

impl WithUniversal for SubagentStartResponse {
    fn universal_mut(&mut self) -> &mut Universal { &mut self.universal }
}

impl HookOutput for SubagentStartResponse {
    fn to_json(&self) -> String {
        let mut root = serde_json::Map::new();
        self.universal.apply(&mut root);

        if let Some(ref ctx) = self.context {
            let mut hso = serde_json::Map::new();
            hso.insert("hookEventName".into(), Value::String("SubagentStart".into()));
            hso.insert("additionalContext".into(), Value::String(ctx.clone()));
            root.insert("hookSpecificOutput".into(), Value::Object(hso));
        }

        serde_json::to_string(&root).unwrap_or_default()
    }
}

// ═════════════════════════════════════════════════════════════════
// Notification — context injection only
// ═════════════════════════════════════════════════════════════════

pub struct NotificationResponse {
    universal: Universal,
    context: Option<String>,
}

impl NotificationResponse {
    pub fn allow() -> Self {
        Self { universal: Universal::default(), context: None }
    }

    pub fn with_context(mut self, ctx: impl Into<String>) -> Self {
        self.context = Some(ctx.into());
        self
    }
}

impl WithUniversal for NotificationResponse {
    fn universal_mut(&mut self) -> &mut Universal { &mut self.universal }
}

impl HookOutput for NotificationResponse {
    fn to_json(&self) -> String {
        let mut root = serde_json::Map::new();
        self.universal.apply(&mut root);

        if let Some(ref ctx) = self.context {
            let mut hso = serde_json::Map::new();
            hso.insert("hookEventName".into(), Value::String("Notification".into()));
            hso.insert("additionalContext".into(), Value::String(ctx.clone()));
            root.insert("hookSpecificOutput".into(), Value::Object(hso));
        }

        serde_json::to_string(&root).unwrap_or_default()
    }
}

// ═════════════════════════════════════════════════════════════════
// Side-effect-only events — no decision, no context
//   PreCompact, SessionEnd, InstructionsLoaded
// ═════════════════════════════════════════════════════════════════

pub struct PreCompactResponse {
    universal: Universal,
}

impl PreCompactResponse {
    pub fn allow() -> Self {
        Self { universal: Universal::default() }
    }
}

impl WithUniversal for PreCompactResponse {
    fn universal_mut(&mut self) -> &mut Universal { &mut self.universal }
}

impl HookOutput for PreCompactResponse {
    fn to_json(&self) -> String {
        let mut root = serde_json::Map::new();
        self.universal.apply(&mut root);
        if root.is_empty() { return String::new(); }
        serde_json::to_string(&root).unwrap_or_default()
    }
}

pub struct SessionEndResponse {
    universal: Universal,
}

impl SessionEndResponse {
    pub fn allow() -> Self {
        Self { universal: Universal::default() }
    }
}

impl WithUniversal for SessionEndResponse {
    fn universal_mut(&mut self) -> &mut Universal { &mut self.universal }
}

impl HookOutput for SessionEndResponse {
    fn to_json(&self) -> String {
        let mut root = serde_json::Map::new();
        self.universal.apply(&mut root);
        if root.is_empty() { return String::new(); }
        serde_json::to_string(&root).unwrap_or_default()
    }
}

pub struct InstructionsLoadedResponse {
    universal: Universal,
}

impl InstructionsLoadedResponse {
    pub fn allow() -> Self {
        Self { universal: Universal::default() }
    }
}

impl WithUniversal for InstructionsLoadedResponse {
    fn universal_mut(&mut self) -> &mut Universal { &mut self.universal }
}

impl HookOutput for InstructionsLoadedResponse {
    fn to_json(&self) -> String {
        let mut root = serde_json::Map::new();
        self.universal.apply(&mut root);
        if root.is_empty() { return String::new(); }
        serde_json::to_string(&root).unwrap_or_default()
    }
}

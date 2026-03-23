pub struct Permission {
    pub name: &'static str,
    pub env_var: &'static str,
    pub description: &'static str,
}

pub static KNOWN_PERMISSIONS: &[Permission] = &[
    Permission {
        name: "Gleipnir",
        env_var: "GLEIPNIR_ENABLED",
        description: "Guardrail system access",
    },
    Permission {
        name: "Voice/TTS",
        env_var: "VOICE_ENABLED",
        description: "Text-to-speech announcements",
    },
    Permission {
        name: "Workspace Registry",
        env_var: "WORKSPACE_REGISTRY_ENABLED",
        description: "Cross-workspace awareness",
    },
];

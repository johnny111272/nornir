use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Fragment {
    pub path: PathBuf,
    pub id: String,
    pub category: FragmentCategory,
    pub load: LoadPolicy,
    pub display_name: String,
    pub byte_size: usize,
    pub coding_related: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FragmentCategory {
    Persona,
    CognitiveMode,
    Behavior,
    Communication,
    Safety,
    Anthropic,
    Coding,
    Expertise { subdomain: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum LoadPolicy {
    Always,
    Implement,
    Manual,
}

#[derive(Debug)]
pub struct Library {
    pub personas: Vec<Fragment>,
    pub always: Vec<Fragment>,
    pub coding: Vec<Fragment>,
    pub expertise: Vec<Fragment>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceProfile {
    pub name: String,
    pub path: PathBuf,
    pub persona: String,
    pub coding: bool,
    pub languages: Vec<String>,
    pub expertise: Vec<String>,
    pub permissions: Vec<String>,
}

pub const LANGUAGES: &[&str] = &["python", "rust", "typescript", "go", "shell"];

pub struct AppState {
    pub library: Library,
    pub profiles: Vec<WorkspaceProfile>,
    pub selected_workspace: Option<usize>,
    pub selected_persona: Option<usize>,
    pub coding_enabled: bool,
    pub selected_languages: Vec<bool>,
    pub selected_expertise: Vec<bool>,
    pub selected_permissions: Vec<bool>,
    pub active_section: Section,
    pub section_cursor: usize,
    pub passthrough_flags: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Section {
    Workspace,
    Persona,
    Coding,
    Expertise,
    Permissions,
}

pub enum TuiOutcome {
    Launch,
    Quit,
}

impl Section {
    pub fn next(self) -> Self {
        match self {
            Section::Workspace => Section::Persona,
            Section::Persona => Section::Coding,
            Section::Coding => Section::Expertise,
            Section::Expertise => Section::Permissions,
            Section::Permissions => Section::Workspace,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Section::Workspace => Section::Permissions,
            Section::Persona => Section::Workspace,
            Section::Coding => Section::Persona,
            Section::Expertise => Section::Coding,
            Section::Permissions => Section::Expertise,
        }
    }
}

impl AppState {
    pub fn new(
        library: Library,
        profiles: Vec<WorkspaceProfile>,
        passthrough_flags: Vec<String>,
    ) -> Self {
        let expertise_count = library.expertise.len();
        let perm_count = crate::permissions::KNOWN_PERMISSIONS.len();
        let lang_count = LANGUAGES.len();

        // Python (index 0) always selected by default
        let mut selected_languages = vec![false; lang_count];
        selected_languages[0] = true;

        Self {
            library,
            profiles,
            selected_workspace: None,
            selected_persona: None,
            coding_enabled: false,
            selected_languages,
            selected_expertise: vec![false; expertise_count],
            selected_permissions: vec![false; perm_count],
            active_section: Section::Workspace,
            section_cursor: 0,
            passthrough_flags,
        }
    }

    pub fn selected_language_names(&self) -> Vec<&str> {
        LANGUAGES
            .iter()
            .enumerate()
            .filter(|(index, _)| self.selected_languages.get(*index).copied().unwrap_or(false))
            .map(|(_, language)| *language)
            .collect()
    }

    pub fn workspace_path(&self) -> Option<&Path> {
        self.selected_workspace
            .and_then(|index| self.profiles.get(index))
            .map(|profile| profile.path.as_path())
    }

    pub fn apply_workspace_profile(&mut self, profile_index: usize) {
        let profile = match self.profiles.get(profile_index) {
            Some(profile) => profile.clone(),
            None => return,
        };

        self.selected_workspace = Some(profile_index);

        // Set persona
        self.selected_persona = self
            .library
            .personas
            .iter()
            .position(|persona| persona.id == profile.persona);

        // Set coding
        self.coding_enabled = profile.coding;

        // Set languages — always keep python, add profile languages
        for (index, language) in LANGUAGES.iter().enumerate() {
            self.selected_languages[index] =
                *language == "python" || profile.languages.contains(&language.to_string());
        }

        // Set expertise
        for (index, fragment) in self.library.expertise.iter().enumerate() {
            self.selected_expertise[index] = profile.expertise.contains(&fragment.id);
        }

        // Set permissions by matching lowercase name
        for (index, permission) in crate::permissions::KNOWN_PERMISSIONS.iter().enumerate() {
            self.selected_permissions[index] = profile
                .permissions
                .iter()
                .any(|profile_perm| profile_perm.eq_ignore_ascii_case(permission.name));
        }
    }

    pub fn clear_workspace_profile(&mut self) {
        self.selected_workspace = None;
        self.selected_persona = None;
        self.coding_enabled = false;
        // Reset languages to just python
        for (index, _) in LANGUAGES.iter().enumerate() {
            self.selected_languages[index] = index == 0;
        }
        for selected in &mut self.selected_expertise {
            *selected = false;
        }
        for selected in &mut self.selected_permissions {
            *selected = false;
        }
    }

    pub fn toggle_coding(&mut self) {
        self.coding_enabled = !self.coding_enabled;
        if self.coding_enabled {
            for (index, fragment) in self.library.expertise.iter().enumerate() {
                if fragment.coding_related {
                    if let Some(selected) = self.selected_expertise.get_mut(index) {
                        *selected = true;
                    }
                }
            }
        }
    }

    pub fn section_len(&self, section: Section) -> usize {
        match section {
            Section::Workspace => self.profiles.len() + 1, // +1 for "Auto"
            Section::Persona => self.library.personas.len() + 1, // +1 for "None"
            Section::Coding => 1 + LANGUAGES.len(), // enable toggle + language toggles
            Section::Expertise => self.library.expertise.len(),
            Section::Permissions => crate::permissions::KNOWN_PERMISSIONS.len(),
        }
    }
}

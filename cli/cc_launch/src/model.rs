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

#[derive(Debug, Clone)]
pub struct Descriptor {
    pub path: PathBuf,
    pub id: String,
    pub parent: Option<String>,
    pub languages: Vec<String>,
    pub display_name: String,
    pub byte_size: usize,
    pub kind: DescriptorKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DescriptorKind {
    System,
    Space,
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
    Expertise,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LoadPolicy {
    Always,
    Implement,
    Manual,
}

#[derive(Debug)]
pub struct Library {
    pub foundation: Option<Fragment>,
    pub personas: Vec<Fragment>,
    pub always: Vec<Fragment>,
    pub coding: Vec<Fragment>,
    pub expertise: Vec<Fragment>,
    pub descriptors: Vec<Descriptor>,
    pub languages: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceProfile {
    pub name: String,
    pub path: PathBuf,
    pub persona: String,
    pub coding: bool,
    pub expertise: Vec<String>,
    pub primary_descriptor: Option<String>,
    pub descriptors: Vec<String>,
}

pub struct AppState {
    pub library: Library,
    pub profiles: Vec<WorkspaceProfile>,
    pub selected_workspace: Option<usize>,
    pub selected_persona: Option<usize>,
    pub coding_enabled: bool,
    pub selected_languages: Vec<bool>,
    pub selected_expertise: Vec<bool>,
    pub selected_descriptors: Vec<bool>,
    pub selected_permissions: Vec<bool>,
    pub active_section: Section,
    pub section_cursor: usize,
    pub passthrough_flags: Vec<String>,
    pub update_mode: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Section {
    Workspace,
    Persona,
    Coding,
    Permissions,
    Descriptors,
    Expertise,
}

pub enum TuiOutcome {
    Launch,
    Quit,
}

impl Section {
    /// Next section following column-aware order:
    /// C1: Workspace → Persona → Expertise
    /// C2: Descriptors → Coding → Permissions
    /// Tab wraps: Permissions → Workspace
    pub fn next(self) -> Self {
        match self {
            Section::Workspace => Section::Persona,
            Section::Persona => Section::Expertise,
            Section::Expertise => Section::Descriptors,
            Section::Descriptors => Section::Coding,
            Section::Coding => Section::Permissions,
            Section::Permissions => Section::Workspace,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Section::Workspace => Section::Permissions,
            Section::Persona => Section::Workspace,
            Section::Expertise => Section::Persona,
            Section::Descriptors => Section::Expertise,
            Section::Coding => Section::Descriptors,
            Section::Permissions => Section::Coding,
        }
    }
}

impl Library {
    /// Returns indices of system descriptors only (not spaces).
    pub fn system_indices(&self) -> Vec<usize> {
        self.descriptors
            .iter()
            .enumerate()
            .filter(|(_, d)| d.kind == DescriptorKind::System)
            .map(|(i, _)| i)
            .collect()
    }

    /// Find the space descriptor matching a persona+path combination.
    /// Returns the descriptor index if persona id matches a space id
    /// AND the profile path contains /spaces/{persona_id}.
    pub fn matching_space_index(&self, persona_id: &str, workspace_path: &Path) -> Option<usize> {
        // Check path contains /spaces/{persona_id}
        let path_str = workspace_path.to_string_lossy();
        let space_marker = format!("/spaces/{persona_id}");
        if !path_str.contains(&space_marker) {
            return None;
        }
        self.descriptors
            .iter()
            .position(|d| d.kind == DescriptorKind::Space && d.id == persona_id)
    }
}

impl AppState {
    pub fn new(
        library: Library,
        profiles: Vec<WorkspaceProfile>,
        passthrough_flags: Vec<String>,
        update_mode: bool,
    ) -> Self {
        let expertise_count = library.expertise.len();
        let descriptor_count = library.descriptors.len();
        let perm_count = crate::permissions::KNOWN_PERMISSIONS.len();

        // Coding on by default, python always selected (index 0)
        let mut selected_languages = vec![false; library.languages.len()];
        if !selected_languages.is_empty() {
            selected_languages[0] = true;
        }

        let initial_persona = if library.personas.is_empty() { None } else { Some(0) };

        Self {
            library,
            profiles,
            selected_workspace: None,
            selected_persona: initial_persona,
            coding_enabled: true,
            selected_languages,
            selected_expertise: vec![false; expertise_count],
            selected_descriptors: vec![false; descriptor_count],
            selected_permissions: vec![false; perm_count],
            active_section: Section::Workspace,
            section_cursor: 0,
            passthrough_flags,
            update_mode,
        }
    }

    pub fn selected_language_names(&self) -> Vec<&str> {
        self.library
            .languages
            .iter()
            .enumerate()
            .filter(|(index, _)| self.selected_languages.get(*index).copied().unwrap_or(false))
            .map(|(_, language)| language.as_str())
            .collect()
    }

    /// Recompute language selections from the union of all selected descriptors.
    /// Python is always included when coding is enabled.
    pub fn sync_languages_from_descriptors(&mut self) {
        for lang in &mut self.selected_languages {
            *lang = false;
        }

        // Collect descriptor indices to pull languages from
        let mut desc_indices: Vec<usize> = self
            .selected_descriptors
            .iter()
            .enumerate()
            .filter(|(_, sel)| **sel)
            .map(|(i, _)| i)
            .collect();

        if let Some(space_idx) = self.auto_space_index() {
            desc_indices.push(space_idx);
        }

        // Union all languages
        for desc_idx in desc_indices {
            let langs: Vec<String> = match self.library.descriptors.get(desc_idx) {
                Some(d) => d.languages.clone(),
                None => continue,
            };
            for lang in &langs {
                let pos = match self.library.languages.iter().position(|l| l == lang) {
                    Some(p) => p,
                    None => continue,
                };
                self.selected_languages[pos] = true;
            }
        }

        if self.coding_enabled {
            self.selected_languages[0] = true; // python always on
        }
    }

    pub fn workspace_path(&self) -> Option<&Path> {
        self.selected_workspace
            .and_then(|index| self.profiles.get(index))
            .map(|profile| profile.path.as_path())
    }

    /// Returns the auto-matched space descriptor index, if any.
    /// Uses the currently selected persona (not the profile default).
    pub fn auto_space_index(&self) -> Option<usize> {
        let persona_id = self.selected_persona
            .and_then(|i| self.library.personas.get(i))
            .map(|p| p.id.as_str())?;
        let workspace_path = self.workspace_path()?;
        self.library.matching_space_index(persona_id, workspace_path)
    }

    /// Returns system descriptor indices ordered for prompt assembly:
    /// primary first, then siblings, then parent overview last.
    /// Space descriptors are NOT included here (they go via auto_space_index).
    pub fn ordered_descriptor_indices(&self) -> Vec<usize> {
        let mut primary = Vec::new();
        let mut siblings = Vec::new();
        let mut overviews = Vec::new();

        let primary_id = self
            .selected_workspace
            .and_then(|ws| self.profiles.get(ws))
            .and_then(|profile| profile.primary_descriptor.as_deref());

        for (index, selected) in self.selected_descriptors.iter().enumerate() {
            if !*selected {
                continue;
            }
            let descriptor = match self.library.descriptors.get(index) {
                Some(d) => d,
                None => continue,
            };
            // Skip spaces — they're handled separately
            if descriptor.kind == DescriptorKind::Space {
                continue;
            }

            if primary_id == Some(descriptor.id.as_str()) {
                primary.push(index);
            } else if self.is_overview_descriptor(index) {
                overviews.push(index);
            } else {
                siblings.push(index);
            }
        }

        let mut result = Vec::new();
        result.extend(primary);
        result.extend(siblings);
        result.extend(overviews);
        result
    }

    /// Check if a descriptor is an overview (has children pointing to it)
    fn is_overview_descriptor(&self, index: usize) -> bool {
        let descriptor = match self.library.descriptors.get(index) {
            Some(d) => d,
            None => return false,
        };
        self.library
            .descriptors
            .iter()
            .any(|d| d.parent.as_deref() == Some(descriptor.id.as_str()))
    }

    /// Map from system-only cursor position to full descriptor index.
    pub fn system_cursor_to_index(&self, cursor: usize) -> Option<usize> {
        self.library.system_indices().get(cursor).copied()
    }

    /// Count of system descriptors (for section_len).
    pub fn system_count(&self) -> usize {
        self.library.system_indices().len()
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

        // Set expertise
        for (index, fragment) in self.library.expertise.iter().enumerate() {
            self.selected_expertise[index] = profile.expertise.contains(&fragment.id);
        }

        // Permissions are NEVER auto-selected — user must explicitly enable

        // Permissions: never auto-selected. User toggles manually in TUI.

        // Set system descriptors based on primary_descriptor + smart family logic
        self.apply_descriptor_defaults(&profile);

        // Derive languages from selected descriptors
        self.sync_languages_from_descriptors();
    }

    fn apply_descriptor_defaults(&mut self, profile: &WorkspaceProfile) {
        // Clear all descriptors first
        for selected in &mut self.selected_descriptors {
            *selected = false;
        }

        let descriptors = &self.library.descriptors;
        let explicit_ids = &profile.descriptors;
        let mut indices_to_select: Vec<usize> = Vec::new();

        // Explicit descriptor list from profile (always honored)
        for (index, descriptor) in descriptors.iter().enumerate() {
            if explicit_ids.iter().any(|id| id == &descriptor.id) {
                indices_to_select.push(index);
            }
        }

        // Primary descriptor + smart family logic
        let primary_id = match &profile.primary_descriptor {
            Some(id) => id.as_str(),
            None => {
                // No primary, just apply explicit list
                for index in indices_to_select {
                    if let Some(selected) = self.selected_descriptors.get_mut(index) {
                        *selected = true;
                    }
                }
                return;
            }
        };

        if let Some(primary_index) = descriptors
            .iter()
            .position(|d| d.id == primary_id && d.kind == DescriptorKind::System)
        {
            indices_to_select.push(primary_index);

            // If primary is a child, also select siblings + parent overview
            if let Some(parent_id) = descriptors[primary_index].parent.as_deref() {
                for (index, descriptor) in descriptors.iter().enumerate() {
                    if descriptor.kind != DescriptorKind::System {
                        continue;
                    }
                    if descriptor.parent.as_deref() == Some(parent_id)
                        || descriptor.id == parent_id
                    {
                        indices_to_select.push(index);
                    }
                }
            }
            // Parent selected: just primary + explicit list. Children not auto-selected.
        }

        for index in indices_to_select {
            if let Some(selected) = self.selected_descriptors.get_mut(index) {
                *selected = true;
            }
        }
    }

    pub fn clear_workspace_profile(&mut self) {
        self.selected_workspace = None;
        self.selected_persona = if self.library.personas.is_empty() { None } else { Some(0) };
        self.coding_enabled = true;
        for (index, _) in self.library.languages.iter().enumerate() {
            self.selected_languages[index] = index == 0;
        }
        for selected in &mut self.selected_expertise {
            *selected = false;
        }
        for selected in &mut self.selected_descriptors {
            *selected = false;
        }
        for selected in &mut self.selected_permissions {
            *selected = false;
        }
    }

    pub fn toggle_coding(&mut self) {
        self.coding_enabled = !self.coding_enabled;
        if self.coding_enabled {
            self.sync_languages_from_descriptors();
            for (index, fragment) in self.library.expertise.iter().enumerate() {
                if fragment.coding_related {
                    if let Some(selected) = self.selected_expertise.get_mut(index) {
                        *selected = true;
                    }
                }
            }
        } else {
            for (index, fragment) in self.library.expertise.iter().enumerate() {
                if fragment.coding_related {
                    if let Some(selected) = self.selected_expertise.get_mut(index) {
                        *selected = false;
                    }
                }
            }
        }
    }

    pub fn section_len(&self, section: Section) -> usize {
        match section {
            Section::Workspace => self.profiles.len() + 1,
            Section::Persona => self.library.personas.len(),
            Section::Descriptors => self.system_count(),
            Section::Coding => 1 + self.library.languages.len(),
            Section::Expertise => self.library.expertise.len(),
            Section::Permissions => crate::permissions::KNOWN_PERMISSIONS.len(),
        }
    }
}

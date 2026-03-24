use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::model::WorkspaceProfile;

pub fn load_profiles(config_path: &Path) -> Result<Vec<WorkspaceProfile>, String> {
    if !config_path.exists() {
        return Ok(Vec::new());
    }

    let content =
        fs::read_to_string(config_path).map_err(|e| format!("read profiles: {e}"))?;

    let table: BTreeMap<String, toml::Value> =
        toml::from_str(&content).map_err(|e| format!("parse profiles: {e}"))?;

    let mut profiles: Vec<WorkspaceProfile> = table
        .into_iter()
        .filter_map(|(name, value)| parse_profile(name, value))
        .collect();

    profiles.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(profiles)
}

fn parse_profile(name: String, value: toml::Value) -> Option<WorkspaceProfile> {
    let section = value.as_table()?;

    let path = section
        .get("path")
        .and_then(|value| value.as_str())
        .map(PathBuf::from)
        .unwrap_or_default();

    let persona = section
        .get("persona")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();

    let coding = section
        .get("coding")
        .and_then(|value| value.as_bool())
        .unwrap_or(true); // default: coding enabled

    let expertise = extract_string_array(section, "expertise").unwrap_or_default();

    let primary_descriptor = section
        .get("primary_descriptor")
        .and_then(|value| value.as_str())
        .map(String::from);

    let descriptors = extract_string_array(section, "descriptors").unwrap_or_default();

    Some(WorkspaceProfile {
        name,
        path,
        persona,
        coding,
        expertise,
        primary_descriptor,
        descriptors,
    })
}

fn extract_string_array(
    section: &toml::map::Map<String, toml::Value>,
    key: &str,
) -> Option<Vec<String>> {
    section.get(key).and_then(|value| value.as_array()).map(|array| {
        array
            .iter()
            .filter_map(|value| value.as_str().map(String::from))
            .collect()
    })
}

mod assembly;
mod library;
mod model;
mod permissions;
mod profiles;
mod tui;

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process;

use clap::Parser;

use model::{AppState, TuiOutcome};

#[derive(Parser)]
#[command(
    name = "cc_launch",
    about = "TUI launcher for Claude Code with composable prompt library"
)]
struct Cli {
    /// Update system prompt for an existing session (no launch)
    #[arg(long)]
    update: bool,

    /// Target session ID (with --update)
    #[arg(long, requires = "update")]
    session: Option<String>,

    /// Target workspace path (with --update); resolves to latest session
    #[arg(long, requires = "update")]
    workspace: Option<PathBuf>,

    /// Flags passed through to claude (e.g., --continue, --resume)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    claude_args: Vec<String>,
}

fn main() {
    let arguments = Cli::parse();
    if let Err(error) = run(arguments) {
        eprintln!("cc_launch: {error}");
        process::exit(1);
    }
}

fn run(arguments: Cli) -> Result<(), String> {
    let ai_home = write_engine::ai_home();
    let library_path = ai_home.join("control/library");
    let profiles_path = ai_home.join("control/cc_launch_profiles.toml");

    let update_mode = arguments.update;
    let update_session = arguments.session;
    let update_workspace = arguments.workspace;

    let library = library::scan_library(&library_path)?;
    let workspace_profiles = profiles::load_profiles(&profiles_path)?;
    let mut state = AppState::new(library, workspace_profiles, arguments.claude_args, update_mode);

    match tui::run_tui(&mut state)? {
        TuiOutcome::Launch => {
            if update_mode {
                run_update(&state, &update_session, &update_workspace)
            } else {
                let prompt_path = assembly::write_prompt_file(&state)?;
                exec_claude(&prompt_path, &state)
            }
        }
        TuiOutcome::Quit => Ok(()),
    }
}

fn run_update(
    state: &AppState,
    session_arg: &Option<String>,
    workspace_arg: &Option<PathBuf>,
) -> Result<(), String> {
    // Resolve workspace name and session ID
    let (workspace_name, session_id) = resolve_update_target(session_arg, workspace_arg)?;

    let target = assembly::write_update_file(state, &session_id, &workspace_name)?;
    eprintln!("Updated: {}", target.display());
    eprintln!("Session: {session_id}");
    eprintln!("Workspace: {workspace_name}");
    eprintln!("Next compact/resume will inject the new prompt.");
    Ok(())
}

/// Resolve the target session for --update.
/// Three modes:
///   --session <id>        → look up workspace from session
///   --workspace <path>    → resolve workspace name, find latest session
///   (neither)             → use cwd to resolve workspace, find latest session
fn resolve_update_target(
    session_arg: &Option<String>,
    workspace_arg: &Option<PathBuf>,
) -> Result<(String, String), String> {
    if let Some(session_id) = session_arg {
        // Explicit session — look up its workspace
        let workspace_name = workspace_registry::workspace_from_session(session_id)?
            .ok_or_else(|| format!("No workspace found for session: {session_id}"))?;
        return Ok((workspace_name, session_id.clone()));
    }

    // Resolve workspace name from --workspace path or cwd
    let resolve_path = match workspace_arg {
        Some(path) => path.to_string_lossy().to_string(),
        None => std::env::current_dir()
            .map_err(|e| format!("get cwd: {e}"))?
            .to_string_lossy()
            .to_string(),
    };

    let workspace_name = workspace_registry::resolve_workspace_from_path(&resolve_path)?
        .ok_or_else(|| format!("No registered workspace for path: {resolve_path}"))?;

    let session_id = workspace_registry::latest_session_for_workspace(&workspace_name)?
        .ok_or_else(|| format!("No sessions found for workspace: {workspace_name}"))?;

    Ok((workspace_name, session_id))
}

fn exec_claude(prompt_path: &Path, state: &AppState) -> Result<(), String> {
    // Set HOOK_LLM_ALLOW_PATHS if any permissions selected
    if let Some(allow_paths) = assembly::build_allow_paths(state) {
        std::env::set_var("HOOK_LLM_ALLOW_PATHS", allow_paths);
    }

    let mut command = std::process::Command::new("claude");
    command
        .arg("--system-prompt-file")
        .arg(prompt_path)
        .args(&state.passthrough_flags);

    // If a workspace is selected, cd there before launching
    if let Some(workspace_path) = state.workspace_path() {
        if workspace_path.is_dir() {
            command.current_dir(workspace_path);
        }
    }

    // exec replaces this process — only returns on error
    let error = command.exec();
    Err(format!("exec claude: {error}"))
}

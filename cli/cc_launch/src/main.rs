mod assembly;
mod library;
mod model;
mod permissions;
mod profiles;
mod tui;

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process;

use clap::Parser;

use model::{AppState, TuiOutcome};

#[derive(Parser)]
#[command(
    name = "cc_launch",
    about = "TUI launcher for Claude Code with composable prompt library"
)]
struct Cli {
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

    let library = library::scan_library(&library_path)?;
    let workspace_profiles = profiles::load_profiles(&profiles_path)?;
    let mut state = AppState::new(library, workspace_profiles, arguments.claude_args);

    match tui::run_tui(&mut state)? {
        TuiOutcome::Launch => {
            let prompt_path = assembly::write_prompt_file(&state)?;
            exec_claude(&prompt_path, &state)
        }
        TuiOutcome::Quit => Ok(()),
    }
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

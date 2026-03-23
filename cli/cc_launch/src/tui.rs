use std::io;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::execute;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;

use crate::assembly::estimate_tokens;
use crate::model::{AppState, Section, TuiOutcome, LANGUAGES};
use crate::permissions::KNOWN_PERMISSIONS;

pub fn run_tui(state: &mut AppState) -> Result<TuiOutcome, String> {
    enable_raw_mode().map_err(|e| format!("enable raw mode: {e}"))?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(|e| format!("enter alt screen: {e}"))?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| format!("create terminal: {e}"))?;

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(panic_info);
    }));

    let outcome = run_event_loop(&mut terminal, state);

    disable_raw_mode().map_err(|e| format!("disable raw mode: {e}"))?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .map_err(|e| format!("leave alt screen: {e}"))?;
    terminal
        .show_cursor()
        .map_err(|e| format!("show cursor: {e}"))?;

    outcome
}

fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut AppState,
) -> Result<TuiOutcome, String> {
    loop {
        terminal
            .draw(|frame| draw_ui(frame, state))
            .map_err(|e| format!("draw: {e}"))?;

        if let Event::Key(key) = event::read().map_err(|e| format!("read event: {e}"))? {
            match handle_key(state, key) {
                KeyAction::Continue => {}
                KeyAction::Launch => return Ok(TuiOutcome::Launch),
                KeyAction::Quit => return Ok(TuiOutcome::Quit),
            }
        }
    }
}

enum KeyAction {
    Continue,
    Launch,
    Quit,
}

fn handle_key(state: &mut AppState, key: KeyEvent) -> KeyAction {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => return KeyAction::Quit,
        KeyCode::Enter => return KeyAction::Launch,
        KeyCode::Tab => {
            state.active_section = if key.modifiers.contains(KeyModifiers::SHIFT) {
                state.active_section.prev()
            } else {
                state.active_section.next()
            };
            state.section_cursor = 0;
            return KeyAction::Continue;
        }
        KeyCode::BackTab => {
            state.active_section = state.active_section.prev();
            state.section_cursor = 0;
            return KeyAction::Continue;
        }
        _ => {}
    }

    let section_len = state.section_len(state.active_section);

    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            if state.section_cursor > 0 {
                state.section_cursor -= 1;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.section_cursor + 1 < section_len {
                state.section_cursor += 1;
            }
        }
        KeyCode::Char(' ') => handle_space(state),
        _ => {}
    }

    KeyAction::Continue
}

fn handle_space(state: &mut AppState) {
    match state.active_section {
        Section::Workspace => {
            if state.section_cursor == 0 {
                state.clear_workspace_profile();
            } else {
                let profile_index = state.section_cursor - 1;
                if profile_index < state.profiles.len() {
                    state.apply_workspace_profile(profile_index);
                }
            }
        }
        Section::Persona => {
            if state.section_cursor == 0 {
                state.selected_persona = None;
            } else {
                let persona_index = state.section_cursor - 1;
                if persona_index < state.library.personas.len() {
                    state.selected_persona = Some(persona_index);
                }
            }
        }
        Section::Coding => {
            if state.section_cursor == 0 {
                state.toggle_coding();
            } else {
                // Language toggles (cursor 1 = first language, etc.)
                let lang_index = state.section_cursor - 1;
                if lang_index < LANGUAGES.len() {
                    // Don't allow deselecting python (index 0)
                    if lang_index == 0 {
                        return;
                    }
                    if let Some(selected) = state.selected_languages.get_mut(lang_index) {
                        *selected = !*selected;
                    }
                }
            }
        }
        Section::Expertise => {
            if let Some(selected) = state.selected_expertise.get_mut(state.section_cursor) {
                *selected = !*selected;
            }
        }
        Section::Permissions => {
            if let Some(selected) = state.selected_permissions.get_mut(state.section_cursor) {
                *selected = !*selected;
            }
        }
    }
}

// --- Drawing ---

fn draw_ui(frame: &mut ratatui::Frame, state: &AppState) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(2)])
        .split(frame.area());

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(outer[0]);

    draw_left_panel(frame, columns[0], state);
    draw_summary_panel(frame, columns[1], state);
    draw_key_bar(frame, outer[1]);
}

fn draw_left_panel(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let workspace_height = 2 + state.profiles.len() as u16 + 1;
    let persona_height = 2 + state.library.personas.len() as u16 + 1;
    let coding_height = 2 + 1 + LANGUAGES.len() as u16;
    let permissions_height = 2 + KNOWN_PERMISSIONS.len() as u16;

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(workspace_height),
            Constraint::Length(persona_height),
            Constraint::Length(coding_height),
            Constraint::Min(4),
            Constraint::Length(permissions_height),
        ])
        .split(area);

    draw_workspace_section(frame, sections[0], state);
    draw_persona_section(frame, sections[1], state);
    draw_coding_section(frame, sections[2], state);
    draw_expertise_section(frame, sections[3], state);
    draw_permissions_section(frame, sections[4], state);
}

fn active_style(active: bool) -> Style {
    if active {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn cursor_style() -> Style {
    Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
}

fn item_style(active: bool, is_cursor: bool) -> Style {
    if active && is_cursor {
        cursor_style()
    } else {
        Style::default()
    }
}

fn draw_workspace_section(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let active = state.active_section == Section::Workspace;
    let block = Block::default()
        .title(" Workspace ")
        .borders(Borders::ALL)
        .border_style(active_style(active));

    let mut lines = Vec::new();

    let marker = if state.selected_workspace.is_none() { "●" } else { "○" };
    lines.push(Line::from(Span::styled(
        format!("  {marker} Auto (current dir)"),
        item_style(active, state.section_cursor == 0),
    )));

    for (index, profile) in state.profiles.iter().enumerate() {
        let marker = if state.selected_workspace == Some(index) { "●" } else { "○" };
        lines.push(Line::from(Span::styled(
            format!("  {marker} {}", capitalize(&profile.name)),
            item_style(active, state.section_cursor == index + 1),
        )));
    }

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_persona_section(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let active = state.active_section == Section::Persona;
    let block = Block::default()
        .title(" Persona ")
        .borders(Borders::ALL)
        .border_style(active_style(active));

    let mut lines = Vec::new();

    let marker = if state.selected_persona.is_none() { "●" } else { "○" };
    lines.push(Line::from(Span::styled(
        format!("  {marker} None (general)"),
        item_style(active, state.section_cursor == 0),
    )));

    for (index, persona) in state.library.personas.iter().enumerate() {
        let marker = if state.selected_persona == Some(index) { "●" } else { "○" };
        lines.push(Line::from(Span::styled(
            format!("  {marker} {}", persona.display_name),
            item_style(active, state.section_cursor == index + 1),
        )));
    }

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_coding_section(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let active = state.active_section == Section::Coding;
    let block = Block::default()
        .title(" Coding ")
        .borders(Borders::ALL)
        .border_style(active_style(active));

    let mut lines = Vec::new();

    let toggle_marker = if state.coding_enabled { "[x]" } else { "[ ]" };
    lines.push(Line::from(Span::styled(
        format!("  {toggle_marker} Enable coding"),
        item_style(active, state.section_cursor == 0),
    )));

    for (index, language) in LANGUAGES.iter().enumerate() {
        let selected = state.selected_languages.get(index).copied().unwrap_or(false);
        let marker = if selected { "[x]" } else { "[ ]" };
        let locked = if index == 0 { " (always)" } else { "" };
        lines.push(Line::from(Span::styled(
            format!("  {marker} {language}{locked}"),
            item_style(active, state.section_cursor == index + 1),
        )));
    }

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_expertise_section(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let active = state.active_section == Section::Expertise;
    let block = Block::default()
        .title(" Expertise ")
        .borders(Borders::ALL)
        .border_style(active_style(active));

    let mut lines = Vec::new();
    for (index, fragment) in state.library.expertise.iter().enumerate() {
        let selected = state.selected_expertise.get(index).copied().unwrap_or(false);
        let marker = if selected { "[x]" } else { "[ ]" };
        lines.push(Line::from(Span::styled(
            format!("  {marker} {}", fragment.display_name),
            item_style(active, state.section_cursor == index),
        )));
    }

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_permissions_section(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let active = state.active_section == Section::Permissions;
    let block = Block::default()
        .title(" Permissions ")
        .borders(Borders::ALL)
        .border_style(active_style(active));

    let mut lines = Vec::new();
    for (index, permission) in KNOWN_PERMISSIONS.iter().enumerate() {
        let selected = state.selected_permissions.get(index).copied().unwrap_or(false);
        let marker = if selected { "[x]" } else { "[ ]" };
        lines.push(Line::from(Span::styled(
            format!("  {marker} {}", permission.name),
            item_style(active, state.section_cursor == index),
        )));
    }

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_summary_panel(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .title(" Session Summary ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green));

    let mut lines = Vec::new();
    summary_workspace(&mut lines, state);
    summary_persona(&mut lines, state);
    summary_coding(&mut lines, state);
    summary_expertise(&mut lines, state);
    summary_permissions(&mut lines, state);
    summary_always_loaded(&mut lines, state);
    summary_token_estimate(&mut lines, state);

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn summary_workspace(lines: &mut Vec<Line<'_>>, state: &AppState) {
    let workspace_name = match state.selected_workspace {
        Some(index) => state
            .profiles
            .get(index)
            .map(|profile| capitalize(&profile.name))
            .unwrap_or_else(|| "?".to_string()),
        None => "Auto (current dir)".to_string(),
    };
    lines.push(Line::from(format!("  Workspace: {workspace_name}")));
    lines.push(Line::from(""));
}

fn summary_persona<'a>(lines: &mut Vec<Line<'a>>, state: &'a AppState) {
    let persona_name = match state.selected_persona {
        Some(index) => state
            .library
            .personas
            .get(index)
            .map(|persona| persona.display_name.as_str())
            .unwrap_or("?"),
        None => "None (general)",
    };
    lines.push(Line::from(format!("  Persona:   {persona_name}")));
    lines.push(Line::from(""));
}

fn summary_coding(lines: &mut Vec<Line<'_>>, state: &AppState) {
    if state.coding_enabled {
        let langs = state.selected_language_names().join(", ");
        lines.push(Line::from(format!("  Coding:    enabled ({langs})")));
    } else {
        lines.push(Line::from("  Coding:    disabled"));
    }
    lines.push(Line::from(""));
}

fn summary_expertise<'a>(lines: &mut Vec<Line<'a>>, state: &'a AppState) {
    let selected: Vec<&str> = state
        .library
        .expertise
        .iter()
        .enumerate()
        .filter(|(index, _)| state.selected_expertise.get(*index).copied().unwrap_or(false))
        .map(|(_, fragment)| fragment.display_name.as_str())
        .collect();

    if selected.is_empty() {
        lines.push(Line::from("  Expertise: none"));
    } else {
        lines.push(Line::from("  Expertise:"));
        for name in &selected {
            lines.push(Line::from(format!("    - {name}")));
        }
    }
    lines.push(Line::from(""));
}

fn summary_permissions(lines: &mut Vec<Line<'_>>, state: &AppState) {
    let selected: Vec<&str> = KNOWN_PERMISSIONS
        .iter()
        .enumerate()
        .filter(|(index, _)| state.selected_permissions.get(*index).copied().unwrap_or(false))
        .map(|(_, permission)| permission.name)
        .collect();

    if selected.is_empty() {
        lines.push(Line::from("  Permissions: none"));
    } else {
        lines.push(Line::from("  Permissions:"));
        for name in &selected {
            lines.push(Line::from(format!("    - {name}")));
        }
    }
    lines.push(Line::from(""));
}

fn summary_always_loaded(lines: &mut Vec<Line<'_>>, state: &AppState) {
    lines.push(Line::from(Span::styled(
        "  Always loaded:",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(Span::styled(
        format!("    {} fragments", state.library.always.len()),
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(""));
}

fn summary_token_estimate(lines: &mut Vec<Line<'_>>, state: &AppState) {
    let tokens = estimate_tokens(state);
    lines.push(Line::from(Span::styled(
        format!("  Est. tokens: ~{tokens}"),
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));
}

fn draw_key_bar(frame: &mut ratatui::Frame, area: Rect) {
    let keys = Line::from(vec![
        Span::styled(
            " Tab",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" section  "),
        Span::styled(
            "↑↓",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" navigate  "),
        Span::styled(
            "Space",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" toggle  "),
        Span::styled(
            "Enter",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" launch  "),
        Span::styled(
            "q",
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" quit"),
    ]);

    frame.render_widget(Paragraph::new(keys), area);
}

fn capitalize(input: &str) -> String {
    let mut chars = input.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().to_string() + chars.as_str(),
    }
}

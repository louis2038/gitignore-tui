use anyhow::{bail, Context, Result};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::env;
use std::fs;
use std::io::{stdout, Write};
use std::path::{Path, PathBuf};
 

use crossterm::event::{poll, read, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, ClearType};
use crossterm::{cursor, execute, queue, style, terminal};

#[derive(Debug, Clone)]
struct Node {
    path: PathBuf,
    name: String,
    is_dir: bool,
    depth: usize,
    expanded: bool,
    selected: bool,
    locked: bool,  // true if ignored by a generic rule
}

fn list_dir_entries(path: &Path, root: &Path, gi: Option<&Gitignore>) -> Result<Vec<Node>> {
    let mut entries = Vec::new();
    let read = fs::read_dir(path).context(format!("Reading directory {:?}", path))?;
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for ent in read {
        if let Ok(e) = ent {
            let p = e.path();
            let name = p
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "".into());
            if p.is_dir() {
                dirs.push((p, name));
            } else {
                files.push((p, name));
            }
        }
    }
    dirs.sort_by_key(|(_, n)| n.clone());
    files.sort_by_key(|(_, n)| n.clone());
    for (p, n) in dirs.into_iter().chain(files.into_iter()) {
        let mut selected = false;
        let mut locked = false;
        let is_dir_flag = p.is_dir();
        if let Some(g) = gi {
            let rel = p.strip_prefix(root).unwrap_or(&p);
            let m = g.matched(rel, is_dir_flag);
            if m.is_ignore() {
                selected = true;
                // Check if it's an exact or generic rule
                // If the exact path is not in the gitignore, it's a generic rule
                locked = !is_exact_match(root, rel);
            }
        }
        entries.push(Node {
            path: p,
            name: n,
            is_dir: is_dir_flag,
            depth: 0,
            expanded: false,
            selected,
            locked,
        });
    }
    Ok(entries)
}

fn is_exact_match(root: &Path, rel_path: &Path) -> bool {
    let gitignore_path = root.join(".gitignore");
    if let Ok(content) = fs::read_to_string(&gitignore_path) {
        let path_str = rel_path.to_string_lossy();
        for line in content.lines() {
            let trimmed = line.trim();
            
            // Ignore empty lines and comments
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            
            // Ignore negation rules (!)
            let pattern = if trimmed.starts_with('!') {
                continue;
            } else {
                trimmed
            };
            
            // Check if the rule contains wildcards
            if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
                continue; // It's a generic rule
            }
            
            // Check if it's an exact match
            let normalized_pattern = pattern.trim_end_matches('/');
            let normalized_path = path_str.trim_end_matches('/');
            
            // Simple exact match
            if normalized_pattern == normalized_path {
                return true;
            }
            
            // Match with leading slash
            if normalized_pattern == format!("/{}", normalized_path) {
                return true;
            }
            
            // Match if pattern starts with ./
            if normalized_pattern.starts_with("./") {
                let pattern_without_dot = &normalized_pattern[2..];
                if pattern_without_dot == normalized_path {
                    return true;
                }
            }
        }
    }
    false
}

fn insert_children(nodes: &mut Vec<Node>, idx: usize, root: &Path, gi: Option<&Gitignore>) -> Result<()> {
    let depth = nodes[idx].depth;
    let parent_path = nodes[idx].path.clone();
    let children = list_dir_entries(&parent_path, root, gi)?;
    let mut insert_at = idx + 1;
    for mut c in children {
        c.depth = depth + 1;
        c.expanded = false;
        nodes.insert(insert_at, c);
        insert_at += 1;
    }
    Ok(())
}

fn collapse_subtree(nodes: &mut Vec<Node>, idx: usize) {
    let depth = nodes[idx].depth;
    let mut remove_from = idx + 1;
    while remove_from < nodes.len() && nodes[remove_from].depth > depth {
        remove_from += 1;
    }
    nodes.drain(idx + 1..remove_from);
}

fn render_header(out: &mut impl Write) -> Result<()> {
    queue!(
        out,
        style::SetAttribute(style::Attribute::Bold),
        style::SetBackgroundColor(style::Color::DarkGrey),
        style::Print(" [S]ave "),
        style::ResetColor,
        style::SetAttribute(style::Attribute::Reset),
        style::Print("  "),
        style::SetAttribute(style::Attribute::Bold),
        style::SetBackgroundColor(style::Color::DarkGrey),
        style::Print(" [Q]uit "),
        style::ResetColor,
        style::SetAttribute(style::Attribute::Reset),
        style::Print("\r\n\r\n")
    )?;
    Ok(())
}

fn has_selected_children(nodes: &Vec<Node>, parent_idx: usize) -> bool {
    let parent_path = &nodes[parent_idx].path;
    
    // Iterate through all nodes to find descendants
    for n in nodes.iter() {
        // Check if it's a descendant of the parent
        if n.path.starts_with(parent_path) && n.path != *parent_path {
            if n.selected && !n.locked {
                return true;
            }
        }
    }
    false
}

fn render(nodes: &Vec<Node>, cursor_pos: usize) -> Result<()> {
    let mut out = stdout();
    queue!(out, terminal::Clear(ClearType::All), cursor::MoveTo(0, 0))?;
    
    // Display button bar at the top
    render_header(&mut out)?;
    
    for (i, n) in nodes.iter().enumerate() {
        if i == cursor_pos {
            queue!(out, style::SetAttribute(style::Attribute::Reverse))?;
        }
        
        // Indentation with a clear character
        for _ in 0..n.depth {
            queue!(out, style::Print("│ "))?;
        }
        
        // Display selection box with a different symbol for locked items
        if n.locked {
            queue!(
                out,
                style::SetForegroundColor(style::Color::DarkGrey),
                style::Print("[X] "),
                style::ResetColor
            )?;
        } else if n.selected {
            queue!(out, style::Print("[x] "))?;
        } else if n.is_dir && has_selected_children(nodes, i) {
            queue!(out, style::Print("[/] "))?;
        } else {
            queue!(out, style::Print("[ ] "))?;
        }
        
        // Display name with color based on type
        if n.is_dir {
            let marker = if n.expanded { "▾" } else { "▸" };
            let color = if n.locked { style::Color::DarkGrey } else { style::Color::Blue };
            queue!(
                out,
                style::SetForegroundColor(color),
                style::SetAttribute(style::Attribute::Bold),
                style::Print(format!("{} {}", marker, n.name)),
                style::ResetColor,
                style::SetAttribute(style::Attribute::Reset)
            )?;
        } else {
            let color = if n.locked { style::Color::DarkGrey } else { style::Color::White };
            queue!(
                out,
                style::SetForegroundColor(color),
                style::Print(format!("  {}", n.name)),
                style::ResetColor
            )?;
        }
        
        if i == cursor_pos {
            queue!(out, style::SetAttribute(style::Attribute::Reset))?;
        }
        queue!(out, style::Print("\r\n"))?;
    }
    out.flush()?;
    Ok(())
}

fn main() -> Result<()> {
    // Get directory from arguments or use "." by default
    let args: Vec<String> = env::args().collect();
    let root_path = if args.len() > 1 {
        &args[1]
    } else {
        "."
    };
    
    let root = Path::new(root_path);
    
    // Check that the directory exists
    if !root.exists() || !root.is_dir() {
        bail!("Path '{}' does not exist or is not a directory", root_path);
    }
    
    let gitignore_path = root.join(".gitignore");

    // Build gitignore matcher if .gitignore exists
    let gi: Option<Gitignore> = if gitignore_path.exists() {
        let mut matcher = GitignoreBuilder::new(root);
        matcher.add(&gitignore_path);
        match matcher.build() {
            Ok(m) => Some(m),
            Err(_) => None,
        }
    } else {
        None
    };

    // Start with root entries
    let mut nodes: Vec<Node> = Vec::new();
    let root_children = list_dir_entries(root, root, gi.as_ref())?;
    for mut c in root_children {
        c.depth = 0;
        c.expanded = false;
        nodes.push(c);
    }

    enable_raw_mode()?;
    execute!(stdout(), terminal::EnterAlternateScreen)?;

    let mut cursor_pos = 0usize;
    render(&nodes, cursor_pos)?;

    loop {
        if poll(std::time::Duration::from_millis(200))? {
            if let Event::Key(k) = read()? {
                match k.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Up => {
                        if cursor_pos > 0 {
                            cursor_pos -= 1;
                        }
                    }
                    KeyCode::Down => {
                        if cursor_pos + 1 < nodes.len() {
                            cursor_pos += 1;
                        }
                    }
                    KeyCode::Right => {
                        if nodes[cursor_pos].is_dir && !nodes[cursor_pos].expanded {
                            if let Err(_e) = insert_children(&mut nodes, cursor_pos, root, gi.as_ref()) {
                                // ignore insert errors
                            } else {
                                nodes[cursor_pos].expanded = true;
                            }
                        }
                    }
                    KeyCode::Left => {
                        if nodes[cursor_pos].is_dir && nodes[cursor_pos].expanded {
                            collapse_subtree(&mut nodes, cursor_pos);
                            nodes[cursor_pos].expanded = false;
                        } else {
                            // try to move to parent
                            if nodes[cursor_pos].depth > 0 {
                                let depth = nodes[cursor_pos].depth;
                                let mut p = cursor_pos;
                                while p > 0 {
                                    p -= 1;
                                    if nodes[p].depth < depth {
                                        cursor_pos = p;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    KeyCode::Enter => {
                        // toggle select for this node (only if not locked)
                        if !nodes[cursor_pos].locked {
                            nodes[cursor_pos].selected = !nodes[cursor_pos].selected;
                        }
                    }
                    KeyCode::Char('s') => {
                        // Read existing .gitignore content if it exists
                        let existing_content = if gitignore_path.exists() {
                            fs::read_to_string(&gitignore_path)
                                .context("Reading existing .gitignore")?
                        } else {
                            String::new()
                        };
                        
                        // Split content into lines
                        let mut lines: Vec<String> = existing_content
                            .lines()
                            .map(|s| s.to_string())
                            .collect();
                        
                        // For each node, handle addition or removal
                        for n in &nodes {
                            // Ignore locked nodes (generic rules)
                            if n.locked {
                                continue;
                            }
                            
                            let rel = n.path.strip_prefix(root).unwrap_or(&n.path);
                            let entry = rel.to_string_lossy().to_string();
                            
                            // Check if the entry already exists
                            let existing_index = lines.iter().position(|line| {
                                line.trim() == entry.trim()
                            });
                            
                            if n.selected {
                                // Add if not already present
                                if existing_index.is_none() {
                                    lines.push(entry);
                                }
                            } else {
                                // Remove if present
                                if let Some(idx) = existing_index {
                                    lines.remove(idx);
                                }
                            }
                        }
                        
                        // Rebuild content with remaining lines
                        let mut new_content = lines.join("\n");
                        if !new_content.is_empty() && !new_content.ends_with('\n') {
                            new_content.push('\n');
                        }
                        
                        fs::write(&gitignore_path, new_content)
                            .context("Writing .gitignore")?;
                        break;
                    }
                    _ => {}
                }
            }
        }
        render(&nodes, cursor_pos)?;
    }

    execute!(stdout(), terminal::LeaveAlternateScreen)?;
    disable_raw_mode()?;

    println!("Selection completed. The `.gitignore` file has been updated in '{}'.", root_path);
    Ok(())
}
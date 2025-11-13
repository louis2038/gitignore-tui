use anyhow::{bail, Context, Result};
use std::env;
use std::fs;
use std::io::{stdout, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crossterm::event::{read, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, ClearType};
use crossterm::{cursor, execute, queue, style, terminal};
use ignore::gitignore::GitignoreBuilder; // NEW

const HEADER_ROWS: u16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    C, // Règle classique dans .gitignore
    E, // Exception (!...)
    N, // Normal (aucune règle)
}

#[derive(Debug, Clone)]
struct Node {
    path: PathBuf,
    name: String,
    is_dir: bool,
    depth: usize,
    expanded: bool,
    mode: Mode,
    mark: bool,
    cpt_exception: usize,
    cpt_mixed_marks: usize,
    generic_mark: bool, // NEW : fichier marqué par une règle générique (*.png, etc.)
}

#[derive(Debug, Clone)]
struct Rule {
    pattern: String, // chemin relatif normalisé "target/flycheck0"
    mode: Mode,      // C ou E
}

/// Parsing du .gitignore :
/// - on garde uniquement les règles SANS wildcard compliqué (* ? [)
///   sauf "*" ou "/*" que l'on accepte comme "tout le repo"
/// - on reconnaît "dir/*" comme "dir"
/// - on accepte les règles avec ou sans "/" en tête, mais on normalise sans "/"
/// - on distingue C (ligne normale) et E (ligne commençant par !)
/// - on retourne une liste ordonnée de règles
fn parse_gitignore(root: &Path) -> Result<Vec<Rule>> {
    let gitignore_path = root.join(".gitignore");
    if !gitignore_path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&gitignore_path)
        .context("Reading existing .gitignore")?;

    let mut rules = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let mut pattern = trimmed;
        let mut mode = Mode::C;

        // Exception ?
        if pattern.starts_with('!') {
            mode = Mode::E;
            pattern = &pattern[1..];
        }

        // On enlève un éventuel "/" au début (on normalise les chemins sans "/")
        if pattern.starts_with('/') {
            pattern = &pattern[1..];
        }

        // Cas spécial : "*" ou "" (si la ligne originale était "/" ou "/*")
        let mut is_root_wildcard = false;
        if pattern == "*" {
            is_root_wildcard = true;
        } else if pattern == "" {
            // Cas bizarre mais au cas où quelqu'un mettrait juste "/"
            is_root_wildcard = true;
        }

        if is_root_wildcard {
            rules.push(Rule {
                pattern: "*".to_string(), // on encode le "tout" avec "*"
                mode,
            });
            continue;
        }

        // On traite "xxx/*" comme "xxx" (répertoire)
        if let Some(stripped) = pattern.strip_suffix("/*") {
            pattern = stripped;
        }

        // On enlève un éventuel "/" final
        let pattern = pattern.trim_end_matches('/');

        if pattern.is_empty() {
            continue;
        }

        // On ignore les règles trop génériques avec wildcard,
        // sauf celles déjà gérées ci-dessus.
        if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
            continue;
        }

        let normalized = pattern.replace("\\", "/");

        rules.push(Rule {
            pattern: normalized,
            mode,
        });
    }

    Ok(rules)
}

/// Construit l'arbre COMPLET de tous les fichiers/répertoires (en pré-ordre).
/// On ajoute un noeud racine "/" qui contient tout le répertoire `root`.
/// Tous les nodes démarrent avec mode = N, mark = false
fn build_full_tree(root: &Path) -> Result<Vec<Node>> {
    fn build_dir(
        current: &Path,
        root: &Path,
        depth: usize,
        nodes: &mut Vec<Node>,
    ) -> Result<()> {
        let read = fs::read_dir(current)
            .context(format!("Reading directory {:?}", current))?;

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
            let is_dir = p.is_dir();
            let node = Node {
                path: p.clone(),
                name: n,
                is_dir,
                depth,
                expanded: false,
                mode: Mode::N,
                mark: false,
                cpt_exception: 0,
                cpt_mixed_marks: 0,
                generic_mark: false, // NEW
            };
            nodes.push(node);
            if is_dir {
                build_dir(&p, root, depth + 1, nodes)?;
            }
        }
        Ok(())
    }

    let mut nodes = Vec::new();

    // --- NOEUD RACINE VIRTUEL CLIQUABLE ---
    nodes.push(Node {
        path: root.to_path_buf(),
        name: "/".to_string(),
        is_dir: true,
        depth: 0,
        expanded: true, // on commence ouvert
        mode: Mode::N,
        mark: false,
        cpt_exception: 0,
        cpt_mixed_marks: 0,
        generic_mark: false, // NEW
    });

    // Les enfants du root sont en profondeur 1
    build_dir(root, root, 1, &mut nodes)?;
    Ok(nodes)
}

fn apply_rules_to_nodes(nodes: &mut Vec<Node>, root: &Path, rules: &[Rule]) {
    let len = nodes.len();

    for i in 0..len {
        let rel = if nodes[i].path == root {
            // noeud racine virtuel -> chemin relatif vide
            Path::new("")
        } else {
            nodes[i].path.strip_prefix(root).unwrap_or(&nodes[i].path)
        };
        let rel_str = rel.to_string_lossy().replace("\\", "/");

        // reset de base
        nodes[i].mode = Mode::N;
        nodes[i].mark = false;

        for rule in rules {
            let pat = &rule.pattern;

            // Cas spécial : "*" = toute l'arborescence
            if pat == "*" {
                match rule.mode {
                    Mode::C => {
                        nodes[i].mark = true;
                        if nodes[i].mode == Mode::E {
                            nodes[i].mode = Mode::N;
                        }
                    }
                    Mode::E => {
                        nodes[i].mark = false;
                        if nodes[i].mode == Mode::C {
                            nodes[i].mode = Mode::N;
                        }
                    }
                    Mode::N => {}
                }
                continue;
            }

            let is_exact = rel_str == *pat;
            let is_descendant = rel_str.starts_with(pat)
                && rel_str.len() > pat.len()
                && rel_str.as_bytes()[pat.len()] == b'/';

            match rule.mode {
                Mode::C => {
                    if is_exact {
                        nodes[i].mode = Mode::C;
                        nodes[i].mark = true;
                    } else if is_descendant {
                        nodes[i].mark = true;
                        if nodes[i].mode == Mode::E {
                            nodes[i].mode = Mode::N;
                        }
                    }
                }
                Mode::E => {
                    if is_exact {
                        nodes[i].mode = Mode::E;
                        nodes[i].mark = false;
                    } else if is_descendant {
                        nodes[i].mark = false;
                        if nodes[i].mode == Mode::C {
                            nodes[i].mode = Mode::N;
                        }
                    }
                }
                Mode::N => {}
            }
        }
    }

    // cpt_exception pour tout l'arbre
    recompute_cpt_exception(nodes);
    // cpt_mixed_marks pour tout l'arbre
    recompute_cpt_mixed_marks(nodes);
}

/// Recalcule cpt_exception pour tous les nodes.
/// - fichier : 1 si mode = E, sinon 0
/// - répertoire : (1 si mode = E) + somme récursive de tous les descendants
fn recompute_cpt_exception(nodes: &mut Vec<Node>) {
    for n in nodes.iter_mut() {
        n.cpt_exception = if n.mode == Mode::E { 1 } else { 0 };
    }

    let len = nodes.len();
    if len == 0 {
        return;
    }

    // Comme nodes est en pré-ordre, les descendants d'un répertoire
    // sont dans un bloc contigu après lui, avec depth plus grand.
    for i in (0..len).rev() {
        if nodes[i].is_dir {
            let depth = nodes[i].depth;
            let mut j = i + 1;
            let mut sum = nodes[i].cpt_exception;
            while j < len && nodes[j].depth > depth {
                sum += nodes[j].cpt_exception;
                j += 1;
            }
            nodes[i].cpt_exception = sum;
        }
    }
}

/// Recalcule cpt_mixed_marks pour tous les nodes.
/// Pour un répertoire : compte le nombre total de descendants (récursif) avec une marque différente
fn recompute_cpt_mixed_marks(nodes: &mut Vec<Node>) {
    let len = nodes.len();
    if len == 0 {
        return;
    }

    // Reset tous les compteurs
    for n in nodes.iter_mut() {
        n.cpt_mixed_marks = 0;
    }

    // Parcours en ordre inverse (post-ordre) pour remonter les compteurs
    for i in (0..len).rev() {
        if nodes[i].is_dir {
            let parent_mark = nodes[i].mark;
            let depth = nodes[i].depth;
            let mut j = i + 1;
            let mut count = 0;
            
            while j < len && nodes[j].depth > depth {
                // Compte si l'enfant a une marque différente
                if nodes[j].mark != parent_mark {
                    count += 1;
                }
                // Ajoute le compteur de l'enfant s'il est un répertoire
                if nodes[j].is_dir {
                    count += nodes[j].cpt_mixed_marks;
                }
                j += 1;
            }
            
            nodes[i].cpt_mixed_marks = count;
        }
    }
}

/// Applique mark + reset des modes/cpt_exception récursivement sur un répertoire.
/// - mark : valeur à mettre sur tous les enfants (-R)
/// - mode des enfants : N
/// - cpt_exception des enfants : 0
/// - cpt_exception du répertoire : 0 (sera recalculé globalement ensuite)
fn apply_recursive_mark_on_dir(nodes: &mut Vec<Node>, idx: usize, mark: bool) {
    let depth = nodes[idx].depth;
    nodes[idx].cpt_exception = 0;

    let mut i = idx + 1;
    while i < nodes.len() && nodes[i].depth > depth {
        if !nodes[i].generic_mark {
            // NEW : on ne touche pas aux fichiers génériques
            nodes[i].mark = mark;
            nodes[i].mode = Mode::N;
        }
        nodes[i].cpt_exception = 0;
        i += 1;
    }
}

/// Construit la liste des indices visibles en fonction de expanded / depth.
fn build_visible_indices(nodes: &Vec<Node>) -> Vec<usize> {
    let mut visible = Vec::new();
    let mut i = 0;
    while i < nodes.len() {
        visible.push(i);
        if nodes[i].is_dir && !nodes[i].expanded {
            let depth = nodes[i].depth;
            i += 1;
            while i < nodes.len() && nodes[i].depth > depth {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    visible
}

fn render_header(out: &mut impl Write) -> Result<()> {
    queue!(
        out,
        cursor::MoveTo(0, 0),
        terminal::Clear(ClearType::CurrentLine),
        style::SetAttribute(style::Attribute::Bold),
        style::SetBackgroundColor(style::Color::DarkGrey),
        style::SetForegroundColor(style::Color::White),
        style::Print(" [S]ave "),
        style::ResetColor,
        style::SetAttribute(style::Attribute::Reset),
        style::Print("  "),
        style::SetAttribute(style::Attribute::Bold),
        style::SetBackgroundColor(style::Color::DarkGrey),
        style::SetForegroundColor(style::Color::White),
        style::Print(" [Q]uit "),
        style::ResetColor,
        style::SetAttribute(style::Attribute::Reset),
        cursor::MoveTo(0, 1),
        terminal::Clear(ClearType::CurrentLine)
    )?;
    Ok(())
}

fn render(nodes: &Vec<Node>, visible: &Vec<usize>, cursor_pos: usize, scroll_offset: usize) -> Result<()> {
    let mut out = stdout();

    let (_, term_height) = terminal::size()?;
    let viewport_rows = term_height.saturating_sub(HEADER_ROWS) as usize;

    queue!(
        out,
        cursor::Hide,
        terminal::Clear(ClearType::All),
        style::ResetColor,
        style::SetAttribute(style::Attribute::Reset)
    )?;

    render_header(&mut out)?;

    let visible_start = scroll_offset.min(visible.len());
    let visible_end = (visible_start + viewport_rows).min(visible.len());

    for (line_idx, vis_idx) in (visible_start..visible_end).enumerate() {
        let i = visible[vis_idx];
        let n = &nodes[i];
        let y = HEADER_ROWS + line_idx as u16;

        queue!(out, cursor::MoveTo(0, y), terminal::Clear(ClearType::CurrentLine))?;

        if vis_idx == cursor_pos {
            queue!(out, style::SetAttribute(style::Attribute::Reverse))?;
        }

        for _ in 0..n.depth {
            queue!(out, style::Print("│ "))?;
        }

        // NEW : affichage du symbole de mark
        let mark_symbol = if n.generic_mark {
            "[o]" // NEW : fichier marqué par règle générique
        } else if n.mark {
            "[x]"
        } else {
            "[ ]"
        };

        queue!(out, style::Print(format!("{} ", mark_symbol)))?;

        if n.is_dir {
            let marker = if n.expanded { "▾" } else { "▸" };
            let has_mixed = n.cpt_mixed_marks > 0;
            
            if has_mixed {
                queue!(
                    out,
                    style::SetForegroundColor(style::Color::Yellow),
                    style::SetAttribute(style::Attribute::Bold),
                    style::Print(format!("{} {}", marker, n.name)),
                    style::ResetColor,
                    style::SetAttribute(style::Attribute::Reset)
                )?;
            } else {
                // Inversé : bleu foncé pour marqué, bleu clair pour non marqué
                let dir_color = if n.mark {
                    style::Color::DarkBlue    // marqué : bleu foncé
                } else {
                    style::Color::Blue        // non marqué : bleu clair
                };

                queue!(
                    out,
                    style::SetForegroundColor(dir_color),
                    style::SetAttribute(style::Attribute::Bold),
                    style::Print(format!("{} {}", marker, n.name)),
                    style::ResetColor,
                    style::SetAttribute(style::Attribute::Reset)
                )?;
            }
        } else {
            // NEW : fichier marqué -> gris
            let file_color = if n.mark {
                style::Color::DarkGrey
            } else {
                style::Color::White
            };

            queue!(
                out,
                style::SetForegroundColor(file_color),
                style::Print(format!("  {}", n.name)),
                style::ResetColor
            )?;
        }

        if vis_idx == cursor_pos {
            queue!(out, style::SetAttribute(style::Attribute::Reset))?;
        }
    }

    out.flush()?;
    Ok(())
}

/// Vérifie si un fichier devrait être ignoré selon les règles du .gitignore
fn should_be_ignored(file_path: &str, rules: &[Rule]) -> bool {
    let normalized = file_path.replace("\\", "/");
    let mut should_ignore = false;

    for rule in rules {
        let pat = &rule.pattern;

        // "*" = tout
        if pat == "*" {
            match rule.mode {
                Mode::C => {
                    should_ignore = true;
                }
                Mode::E => {
                    should_ignore = false;
                }
                Mode::N => {}
            }
            continue;
        }

        let is_exact = normalized == *pat;
        let is_descendant = normalized.starts_with(pat)
            && normalized.len() > pat.len()
            && normalized.as_bytes()[pat.len()] == b'/';

        match rule.mode {
            Mode::C => {
                if is_exact || is_descendant {
                    should_ignore = true;
                }
            }
            Mode::E => {
                if is_exact || is_descendant {
                    should_ignore = false;
                }
            }
            Mode::N => {}
        }
    }

    should_ignore
}

/// Exécute `jj file list` et désindexe les fichiers qui devraient être ignorés
fn untrack_ignored_files(root: &Path) -> Result<()> {
    // Exécute `jj file list`
    let output = Command::new("jj")
        .arg("file")
        .arg("list")
        .current_dir(root)
        .output()
        .context("Failed to execute 'jj file list'")?;

    if !output.status.success() {
        bail!("'jj file list' failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    let tracked_files = String::from_utf8_lossy(&output.stdout);
    
    // Parse les règles du .gitignore actuel (règles simples)
    let rules = parse_gitignore(root)?;

    // NEW : matcher pour les règles génériques (*.png, etc.)
    let generic_gitignore = build_generic_gitignore(root)?;
    
    let mut untracked_count = 0;
    
    for file in tracked_files.lines() {
        let file = file.trim();
        if file.is_empty() {
            continue;
        }
        
        // Vérifie si le fichier devrait être ignoré par les règles simples
        let mut ignored = should_be_ignored(file, &rules);

        // NEW : vérifie aussi contre les patterns génériques
        if !ignored {
            if let Some(ref gi) = generic_gitignore {
                let path = Path::new(file);
                if gi.matched(path, false).is_ignore() {
                    ignored = true;
                }
            }
        }
        
        if ignored {
            println!("Untracking: {}", file);
            
            let untrack_output = Command::new("jj")
                .arg("file")
                .arg("untrack")
                .arg(file)
                .current_dir(root)
                .output()
                .context(format!("Failed to untrack '{}'", file))?;
            
            if !untrack_output.status.success() {
                eprintln!("Warning: Failed to untrack '{}': {}", 
                    file, 
                    String::from_utf8_lossy(&untrack_output.stderr));
            } else {
                untracked_count += 1;
            }
        }
    }
    
    if untracked_count > 0 {
        println!("\nUntracked {} file(s) that should be ignored.", untracked_count);
    } else {
        println!("\nNo files to untrack.");
    }
    
    Ok(())
}

/// NEW : Construit un matcher pour les règles génériques (*.png, etc.)
fn build_generic_gitignore(root: &Path) -> Result<Option<ignore::gitignore::Gitignore>> {
    let gitignore_path = root.join(".gitignore");
    if !gitignore_path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&gitignore_path)
        .context("Reading .gitignore for generic patterns")?;

    let mut builder = GitignoreBuilder::new(root);
    let mut has_patterns = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // On ignore les exceptions génériques pour l'instant
        if trimmed.starts_with('!') {
            continue;
        }

        // On ne veut pas "*" ou "/*"
        if trimmed == "*" || trimmed == "/*" {
            continue;
        }

        // On ne veut pas les "qqchose/*"
        if trimmed.ends_with("/*") {
            continue;
        }

        // On ne garde que les patterns avec wildcard
        if trimmed.contains('*') || trimmed.contains('?') || trimmed.contains('[') {
            builder
                .add_line(None, trimmed)
                .context("Adding generic pattern to GitignoreBuilder")?;
            has_patterns = true;
        }
    }

    if !has_patterns {
        return Ok(None);
    }

    let gitignore = builder
        .build()
        .context("Building generic Gitignore matcher")?;

    Ok(Some(gitignore))
}

/// NEW : Marque les fichiers qui correspondent aux patterns génériques
fn mark_generic_matches(nodes: &mut Vec<Node>, root: &Path) -> Result<()> {
    let gitignore_opt = build_generic_gitignore(root)?;
    let Some(gitignore) = gitignore_opt else {
        return Ok(());
    };

    for n in nodes.iter_mut() {
        if n.path == root {
            continue;
        }
        if !n.path.is_file() {
            continue;
        }

        let rel = n.path.strip_prefix(root).unwrap_or(&n.path);
        let matched = gitignore.matched(rel, false);

        if matched.is_ignore() {
            n.mark = true;
            n.generic_mark = true;
        }
    }

    // Les marks ayant changé, on recalcule les mixed-marks
    recompute_cpt_mixed_marks(nodes);
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    
    let mut root_path = ".";
    let mut use_jj = false;
    
    // Parse des arguments
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-j" | "--jj" => {
                use_jj = true;
            }
            arg if !arg.starts_with('-') => {
                root_path = arg;
            }
            _ => {
                bail!("Unknown argument: {}", args[i]);
            }
        }
        i += 1;
    }
    
    let root = Path::new(root_path);

    if !root.exists() || !root.is_dir() {
        bail!("Path '{}' does not exist or is not a directory", root_path);
    }

    let gitignore_path = root.join(".gitignore");

    // 1) On parse le .gitignore comme liste ordonnée de règles
    let rules = parse_gitignore(root)?;

    // 2) On construit l'arbre COMPLET (tous les fichiers, même dans les dossiers "repliés")
    let mut nodes: Vec<Node> = build_full_tree(root)?;

    // 3) On applique les règles : propagation des marks + exceptions
    apply_rules_to_nodes(&mut nodes, root, &rules);

    // NEW : on applique les patterns génériques (*.png, etc.)
    mark_generic_matches(&mut nodes, root)?;

    // 4) On recalcule les cpt_exception et cpt_mixed_marks
    recompute_cpt_exception(&mut nodes);
    recompute_cpt_mixed_marks(&mut nodes);

    enable_raw_mode()?;
    execute!(stdout(), terminal::EnterAlternateScreen, cursor::Hide)?;

    let mut cursor_pos: usize = 0;      // index dans les visibles
    let mut scroll_offset: usize = 0;

    let mut visible = build_visible_indices(&nodes);
    render(&nodes, &visible, cursor_pos, scroll_offset)?;

    loop {
        match read()? {
            Event::Key(k) => {
                let (_, term_height) = terminal::size()?;
                let available_height = (term_height as usize).saturating_sub(HEADER_ROWS as usize).max(1);

                // Liste des visibles AVANT de traiter la touche
                visible = build_visible_indices(&nodes);
                if visible.is_empty() {
                    cursor_pos = 0;
                    scroll_offset = 0;
                    continue;
                }
                if cursor_pos >= visible.len() {
                    cursor_pos = visible.len().saturating_sub(1);
                }

                let mut jump_to_idx: Option<usize> = None;

                match k.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Up => {
                        if cursor_pos > 0 {
                            cursor_pos -= 1;
                            if cursor_pos < scroll_offset {
                                scroll_offset = cursor_pos;
                            }
                        }
                    }
                    KeyCode::Down => {
                        if cursor_pos + 1 < visible.len() {
                            cursor_pos += 1;
                            if cursor_pos >= scroll_offset + available_height {
                                scroll_offset = cursor_pos + 1 - available_height;
                            }
                        }
                    }
                    KeyCode::Right => {
                        let idx = visible[cursor_pos];
                        if nodes[idx].is_dir && !nodes[idx].expanded {
                            nodes[idx].expanded = true;
                        }
                    }
                    KeyCode::Left => {
                        let idx = visible[cursor_pos];
                        if nodes[idx].is_dir && nodes[idx].expanded {
                            nodes[idx].expanded = false;
                        } else {
                            // Aller au parent si possible
                            let depth = nodes[idx].depth;
                            if depth > 0 {
                                let mut p = idx;
                                while p > 0 {
                                    p -= 1;
                                    if nodes[p].depth < depth {
                                        jump_to_idx = Some(p);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    KeyCode::Enter => {
                        if visible.is_empty() {
                            continue;
                        }

                        let idx = visible[cursor_pos];
                        
                        // NEW : les fichiers marqués par une règle générique (*.png, etc.) ne sont pas cliquables
                        if nodes[idx].generic_mark && !nodes[idx].is_dir {
                            continue;
                        }
                        
                        let was_marked = nodes[idx].mark;
                        let is_dir = nodes[idx].is_dir;

                        if !was_marked {
                            // mark : false -> true
                            nodes[idx].mark = true;

                            match nodes[idx].mode {
                                Mode::E => {
                                    nodes[idx].mode = Mode::N;
                                }
                                Mode::N => {
                                    nodes[idx].mode = Mode::C;
                                }
                                Mode::C => {}
                            }

                            if is_dir {
                                apply_recursive_mark_on_dir(&mut nodes, idx, true);
                            }
                        } else {
                            // mark : true -> false
                            nodes[idx].mark = false;

                            match nodes[idx].mode {
                                Mode::N => {
                                    nodes[idx].mode = Mode::E;
                                }
                                Mode::C => {
                                    nodes[idx].mode = Mode::N;
                                }
                                Mode::E => {}
                            }

                            if is_dir {
                                apply_recursive_mark_on_dir(&mut nodes, idx, false);
                            }
                        }

                        // Recalcul global des compteurs
                        recompute_cpt_exception(&mut nodes);
                        recompute_cpt_mixed_marks(&mut nodes);
                    }
                    KeyCode::Char('s') => {
                        let existing_content = if gitignore_path.exists() {
                            fs::read_to_string(&gitignore_path)
                                .context("Reading existing .gitignore")?
                        } else {
                            String::new()
                        };

                        let mut lines: Vec<String> =
                            existing_content.lines().map(|s| s.to_string()).collect();

                        use std::collections::HashSet;
                        let mut to_remove: HashSet<String> = HashSet::new();

                        // On prépare les variantes à supprimer (avec et sans "/")
                        for n in &nodes {
                            let rel = n.path.strip_prefix(root).unwrap_or(&n.path);
                            let mut entry = rel.to_string_lossy().to_string();
                            entry = entry.replace("\\", "/");

                            if entry.is_empty() {
                                continue; // le noeud racine "/" est géré à part
                            }

                            let base = entry.clone();

                            // Anciennes formes sans "/" devant
                            to_remove.insert(base.clone());
                            to_remove.insert(format!("{base}/*"));
                            to_remove.insert(format!("!{base}"));
                            to_remove.insert(format!("!{base}/*"));

                            // Nouvelles formes avec "/" devant
                            to_remove.insert(format!("/{base}"));
                            to_remove.insert(format!("/{base}/*"));
                            to_remove.insert(format!("!/{base}"));
                            to_remove.insert(format!("!/{base}/*"));
                        }

                        // On gère aussi les patterns globaux "*", "/*", "!*", "/*!*"
                        to_remove.insert("*".to_string());
                        to_remove.insert("/*".to_string());
                        to_remove.insert("!*".to_string());
                        to_remove.insert("!/*".to_string());

                        // On garde les lignes qui ne nous concernent pas
                        lines.retain(|line| {
                            let trimmed = line.trim();
                            if trimmed.is_empty() || trimmed.starts_with('#') {
                                return true;
                            }
                            !to_remove.contains(trimmed)
                        });

                        // --- CAS PARTICULIER : NOEUD RACINE "/" ---
                        // On commence par gérer le noeud racine s'il est marqué
                        if !nodes.is_empty() {
                            let root_node = &nodes[0];
                            if root_node.mark {
                                // Le noeud racine est marqué -> on veut "/*" en premier
                                lines.insert(0, "/*".to_string());
                            }
                        }

                        // On ajoute les nouvelles règles selon mode / cpt_exception
                        for n in &nodes {
                            let rel = n.path.strip_prefix(root).unwrap_or(&n.path);
                            let mut entry = rel.to_string_lossy().to_string();
                            entry = entry.replace("\\", "/");

                            // Sauter le noeud racine, déjà traité ci-dessus
                            if entry.is_empty() {
                                continue;
                            }

                            // Pour les autres entrées : on écrit toujours un "/" devant
                            match n.mode {
                                Mode::N => {
                                    // Répertoire "normal" mais qui contient au moins une exception
                                    // -> on veut :
                                    // !/entry
                                    // /entry/*
                                    if n.is_dir && n.cpt_exception > 0 {
                                        lines.push(format!("!/{entry}"));
                                        lines.push(format!("/{entry}/*"));
                                    }
                                }
                                Mode::C => {
                                    // Règle d'ignore classique
                                    // - si c'est un dossier avec des exceptions -> /entry/*
                                    // - sinon -> /entry
                                    if n.is_dir && n.cpt_exception > 0 {
                                        lines.push(format!("/{entry}/*"));
                                    } else {
                                        lines.push(format!("/{entry}"));
                                    }
                                }
                                Mode::E => {
                                    // Exception explicite
                                    lines.push(format!("!/{entry}"));
                                }
                            }
                        }

                        let mut new_content = String::new();
                        for (i, line) in lines.iter().enumerate() {
                            if i > 0 {
                                new_content.push('\n');
                            }
                            new_content.push_str(line);
                        }
                        if !new_content.is_empty() && !new_content.ends_with('\n') {
                            new_content.push('\n');
                        }

                        fs::write(&gitignore_path, new_content)
                            .context("Writing .gitignore")?;
                        break;
                    }
                    _ => {}
                }

                // Après modification, on recalcule les visibles et on corrige le curseur / scroll
                visible = build_visible_indices(&nodes);
                if visible.is_empty() {
                    cursor_pos = 0;
                    scroll_offset = 0;
                } else {
                    if let Some(target_idx) = jump_to_idx {
                        if let Some(new_row) = visible.iter().position(|&i| i == target_idx) {
                            cursor_pos = new_row;
                        }
                    }

                    if cursor_pos >= visible.len() {
                        cursor_pos = visible.len().saturating_sub(1);
                    }

                    let max_scroll = visible.len().saturating_sub(available_height);
                    if cursor_pos < scroll_offset {
                        scroll_offset = cursor_pos;
                    } else if cursor_pos >= scroll_offset + available_height {
                        scroll_offset = cursor_pos + 1 - available_height;
                    }
                    scroll_offset = scroll_offset.min(max_scroll);
                }

                render(&nodes, &visible, cursor_pos, scroll_offset)?;
            }
            Event::Resize(_, _) => {
                visible = build_visible_indices(&nodes);
                render(&nodes, &visible, cursor_pos, scroll_offset)?;
            }
            _ => {}
        }
    }

    execute!(stdout(), cursor::Show, terminal::LeaveAlternateScreen)?;
    disable_raw_mode()?;

    println!(
        "Selection completed. The `.gitignore` file has been updated in '{}'.",
        root_path
    );
    
    // Si l'option -j est activée, on désindexe les fichiers ignorés
    if use_jj {
        println!("\nChecking tracked files with jj...");
        if let Err(e) = untrack_ignored_files(root) {
            eprintln!("Error while untracking files: {}", e);
        }
    }
    
    Ok(())
}

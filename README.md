# gitignore-tui

Interactive terminal UI for visually managing .gitignore entries in your Git or Jujutsu projects with support for complex ignore patterns and exceptions.

## Development Note

This project was developed through a collaboration between human logic and AI implementation:
- The core logic (mode system, mark propagation, rule application) was designed and specified by the author
- The user interface, terminal rendering, and integration code were implemented with AI assistance

This project was developed as an initial version in a single day. Please don't hesitate to send bug reports.

## Features

- 🎨 **Interactive Terminal UI** - Navigate through your project files with an intuitive interface
- 📁 **Tree View** - Expand and collapse directories to explore your project structure
- ✅ **Visual Selection** - Check/uncheck files and directories to add/remove from .gitignore
- 🎯 **Smart Rule Management** - Supports both standard ignore rules and exception patterns
- 🌟 **Generic Pattern Support** - Handles wildcard patterns like `*.png`, `*.log`, etc. with visual feedback
- 🔄 **Reverse Gitignore** - Create "ignore everything except" patterns with `/*` + exceptions
- 🌳 **Root Directory Control** - Clickable `/` root node to ignore or whitelist the entire project
- 🎨 **Visual Indicators** - Color-coded directories and files show selection states
- 💾 **Smart Editing** - Preserves existing .gitignore entries and comments (including generic patterns)
- 🦀 **Jujutsu Integration** - Optional `-j` flag to automatically untrack ignored files in Jujutsu repos
- ⚡ **Fast Navigation** - Keyboard shortcuts for efficient workflow

## Installation

### Prerequisites

First, you need to install Rust and Cargo (Rust's package manager):

1. Visit [https://rustup.rs/](https://rustup.rs/)
2. Follow the installation instructions for your operating system
3. Verify installation with: `cargo --version`

### Install from Git Repository

Once Cargo is installed, you can install `git-ignore` directly from the Git repository:

```bash
cargo install --git https://github.com/louis2038/gitignore-tui
```

This will compile and install the binary as `git-ignore` in your Cargo bin directory (usually `~/.cargo/bin/`).

Make sure `~/.cargo/bin/` is in your PATH to use the command from anywhere.

## Uninstall

To uninstall the tool, run:

```bash
cargo uninstall git-ignore
```

## Usage

Navigate to your project directory and run:

```bash
git-ignore
```

Or specify a directory path:

```bash
git-ignore /path/to/your/project
```

### Jujutsu Integration

If you're using [Jujutsu](https://github.com/martinvonz/jj) as your version control system, you can use the `-j` or `--jj` flag to automatically untrack files that should be ignored:

```bash
git-ignore -j
```

This will:
1. Save your .gitignore changes
2. Run `jj file list` to get all tracked files
3. Automatically untrack any files that match the ignore rules (including generic patterns)

This is useful when you add new ignore rules and want to immediately remove those files from tracking.

### Keyboard Shortcuts

- **↑/↓** - Navigate up and down
- **←/→** - Collapse/expand directories or move to parent directory
- **Enter** - Toggle selection (ignore/unignore) for the current item
- **S** - Save changes to .gitignore and exit
- **Q** - Quit without saving

### Visual Indicators

#### Selection States
- `[ ]` - Not ignored (file/directory will be tracked)
- `[x]` - Ignored (file/directory will be ignored by Git/Jujutsu)
- `[o]` - Matched by generic pattern (e.g., `*.png`, `*.log`) - **non-interactive**

**Note:** Files marked with `[o]` are matched by wildcard patterns in your .gitignore and cannot be toggled in the UI. These patterns are preserved when saving but managed separately from the interactive tree.

#### File Colors
- **White** - Not ignored, will be tracked
- **Dark Grey** - Ignored (either by direct selection `[x]` or generic pattern `[o]`)

#### Directory Colors
- **Light Blue** - Not ignored, all children have consistent selection state
- **Dark Blue** - Ignored, all children have consistent selection state  
- **Yellow** - Mixed selection state (some children ignored, some not)

#### Directory Expansion
- `▸` - Collapsed directory (children hidden)
- `▾` - Expanded directory (children visible)

#### Root Directory
- `/` - Special root node representing the entire project directory
  - Can be checked to create a "reverse gitignore" (ignore everything by default)
  - Useful for projects where you want to whitelist specific files

## How It Works

### Rule Types

The tool manages three types of patterns in your .gitignore:

1. **Classic Rules (C)** - Standard ignore patterns
   - Example: `/build` ignores the build directory
   
2. **Exception Rules (E)** - Whitelist patterns starting with `!`
   - Example: `!/build/important.txt` makes an exception for a specific file
   
3. **Generic Patterns** - Wildcard patterns (preserved but read-only in UI)
   - Example: `*.log`, `*.png`, `**/*.tmp`
   - Displayed as `[o]` in the interface
   - Cannot be toggled interactively
   - Preserved when saving .gitignore

### Rule Application

Rules are processed in order from top to bottom of the .gitignore file, with later rules overriding earlier ones:

1. The tool reads your existing `.gitignore` file
2. Accepts rules with or without leading `/` (e.g., `src` or `/src`)
3. Supports the special pattern `/*` to ignore everything at the root
4. Generic patterns (`*.png`, etc.) are parsed separately using the `ignore` crate
5. Applies rules to the file tree, with the last matching rule winning
6. Propagates ignore state recursively to child files/directories

### Smart Pattern Generation

When you save, the tool generates optimized .gitignore patterns while preserving existing generic patterns:

- **Simple file/directory**: `/path/to/file`
- **Directory with exceptions**: 
  ```
  !/parent/dir
  /parent/dir/*
  ```
  This pattern allows the directory but ignores all its contents (useful for nested exceptions)
- **Root wildcard**: `/*` (when the root `/` node is marked as ignored)
- **Exception**: `!/path/to/exception`
- **Generic patterns**: Preserved unchanged (e.g., `*.png`, `*.log`)

All generated non-generic patterns use leading `/` for consistency and precision (anchored to repository root).

### Recursive Selection

When you toggle a directory:
- Checking a directory marks all its children as ignored (except files already matched by generic patterns)
- Unchecking creates an exception for that directory and its children
- Files marked by generic patterns `[o]` are not affected by recursive operations
- The tool automatically generates the necessary patterns to maintain your selections

### Example Workflow

#### Scenario 1: Standard Ignore
```
[ ] /
  [ ] src/
    [ ] main.rs
  [x] target/     <- Mark as ignored
  [ ] README.md
```

Result in .gitignore:
```
/target
```

#### Scenario 2: Reverse Gitignore
```
[x] /            <- Mark root as ignored
  [x] src/
    [ ] main.rs   <- Unmark this file
  [x] target/
  [ ] README.md   <- Unmark this file
```

Result in .gitignore:
```
/*
!/src
/src/*
!/src/main.rs
!/README.md
```

This creates an "ignore everything except" pattern.

#### Scenario 3: Directory with Exceptions
```
[x] build/       <- Directory ignored
  [x] output/
  [ ] config.yml <- Exception
```

Result in .gitignore:
```
/build/*
!/build/config.yml
```

#### Scenario 4: Generic Patterns (Read-Only)
```
[ ] /
  [ ] images/
    [o] photo1.png    <- Matched by *.png (non-interactive)
    [o] photo2.png    <- Matched by *.png (non-interactive)
    [ ] README.md
  [ ] logs/
    [o] app.log       <- Matched by *.log (non-interactive)
```

Existing .gitignore:
```
*.png
*.log
```

These patterns are preserved when saving. Clicking on `photo1.png` or `app.log` has no effect.

## Technical Details

### Pattern Normalization

- Input patterns: Accepts both `/src` and `src`
- Output patterns: Always generates `/src` (anchored to root)
- Path separators: Automatically converts Windows `\` to `/`
- Generic patterns: Preserved as-is

### File Exclusions

The tool automatically handles:
- Comments (lines starting with `#`)
- Empty lines
- Complex wildcard patterns (`*`, `?`, `[...]`) - preserved but displayed as `[o]` for matching files

### Generic Pattern Handling

The tool uses the `ignore` crate to properly evaluate wildcard patterns:
- Patterns like `*.png`, `**/*.tmp`, `?.log` are evaluated against actual files
- Only files (not directories) can be marked by generic patterns
- Generic pattern matches are shown as `[o]` and are read-only
- These patterns are never removed when saving

### Counter Display

Directories show useful counters:
- **Exception count**: Number of exception rules (`!pattern`) in the subtree
- **Mixed marks**: Visual indicator (yellow) when children have different selection states

## Examples

### Basic Usage
```bash
cd my-project
git-ignore
```

### With Jujutsu Auto-Untrack
```bash
git-ignore --jj
```

### Specify Different Directory
```bash
git-ignore ~/projects/my-app
```

## Requirements

- Rust 1.70 or higher
- Terminal with Unicode support for tree characters (▸, ▾, │)
- For Jujutsu integration: `jj` command must be in PATH

## License

Proprietary

## Author

Louis Triouleyre-Roberjot <louis.triouleyre@gmail.com>

## Contributing

Contributions are welcome! Please feel free to submit issues or pull requests.

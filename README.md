# gitignore-tui

Simple interactive terminal for visually selecting and managing .gitignore entries in your Git projects.

## Features

- 🎨 **Interactive Terminal UI** - Navigate through your project files with an intuitive interface
- 📁 **Tree View** - Expand and collapse directories to explore your project structure
- ✅ **Visual Selection** - Check/uncheck files and directories to add/remove from .gitignore
- 🔒 **Generic Rule Detection** - Automatically detects and locks items matched by wildcard patterns
- 💾 **Smart Editing** - Preserves existing .gitignore entries and comments
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
cargo uninstall --bin git-ignore
```

## Usage

Navigate to your Git project directory and run:

```bash
git-ignore
```

Or specify a directory path:

```bash
git-ignore /path/to/your/project
```

### Keyboard Shortcuts

- **↑/↓** - Navigate up and down
- **←/→** - Collapse/expand directories or move to parent
- **Enter** - Toggle selection for the current item
- **S** - Save changes to .gitignore
- **Q** - Quit without saving

### Interface Symbols

- `[ ]` - Not ignored
- `[x]` - Explicitly ignored (can be toggled)
- `[X]` - Ignored by a generic rule (locked, cannot be toggled)
- `[/]` - Directory with some children ignored
- `▸` - Collapsed directory
- `▾` - Expanded directory

## How It Works

1. The tool scans your project directory
2. It reads your existing `.gitignore` file (if present)
3. Files/directories matched by exact entries are shown as selected
4. Files/directories matched by generic patterns (wildcards) are locked
5. You can toggle selection for non-locked items
6. When you save (press **S**), the `.gitignore` file is updated with your changes

## Example

```bash
cd my-project
git-ignore
```

Navigate through your files, select what you want to ignore, press **S** to save, and **Q** to quit.

## License

Proprietary

## Author

Louis Triouleyre-Roberjot <louis.triouleyre@gmail.com>

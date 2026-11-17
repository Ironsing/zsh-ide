# Zsh IDE

This project aims for an experience at least as feature-complete as Mads Hartmann's Bash IDE extension, for Zsh.

## Layout

vscode/ contains the VSCode extension, which acts as the LSP client. It's a regular VSCode extension npm project. At n=2 editors, move it into an editors/ folder.

zsh-ls/ contains a language server written in Rust. It uses tree-sitter and tree-sitter-zsh.

## Git

### Commit style
Short, lowercase-only, imperative commit messages with no body unless explicitly asked for by the user.

### Other instructions
Some users require you to automatically make commits in the repo's style; others prefer authoring history themselves with some exceptions where they delegate to you. Ask the user which they are on first contact rather than guessing.

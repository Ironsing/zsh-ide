#!/usr/bin/env zsh

# A script for exercising document symbols: every `source`/`.` command, every
# `alias`, every function and every assignment should show up in the outline,
# and nothing else should.

# source: unquoted, tilde-expanded
source ~/.zshrc

# source: double-quoted (the outline selects the path, not the quotes)
source "$ZDOTDIR/.zshrc"

# source: single-quoted relative path
source './lib/helpers.zsh'

# source: the dot form
. ./lib/completion.zsh

# source: `--` is not the symbol's name
source -- "$HOME/.zshrc"

# alias: single-quoted value
alias lzg='lazygit'

# alias: double-quoted value with spaces
alias gs="git status --short"

# alias: unquoted value
alias la=ls

# alias: only the first `=` splits name from value
alias eq="foo=bar"

# alias: global, the `-g` flag is not the symbol's name
alias -g L='| less'

# alias: suffix
alias -s txt=nvim

# alias: multi-byte name and value, so the ranges have to count UTF-16 code
# units rather than bytes
alias héllo='echo "héllo, wörld 🌍"'

# a symbol after multi-byte text on the same line: this alias starts at byte 29
# of the line but at code unit 26
printf '%s\n' 'héllo 🌍'; alias greet='echo "héllo"'

# none of these are symbols
unalias zz
echo "source ~/.zshrc"
echo "alias l=lazygit"
# source commented-out.zsh
# alias commented=1

# functions, nested ones included
build() {
  echo "build"
}

outer() {
  inner() {
    echo "nested"
  }
  inner
}

# anonymous function (zsh 5.4+)
() {
  echo "anon"
}

# loops are still not symbols
for item in alpha beta; do
  echo "$item"
done

# --- plain scalar assignment ---
PLAIN=value
QUOTED="value with spaces"
SINGLE='single quoted'
EMPTY=

# --- appending ---
APPENDED+=more

# --- expansions on the right-hand side ---
FROM_VAR=${OTHER}
FROM_COMMAND=$(echo hi)
FROM_ARITHMETIC=$((1 + 2))

# --- arrays ---
ARRAY=(one two three)
EMPTY_ARRAY=()
INDEXED[2]=value

# --- associative arrays ---
declare -A ASSOC=( [key]=value )

# --- declaration commands ---
local LOCAL_VAR=value
export EXPORTED=value
readonly READONLY_VAR=value
typeset TYPESET_VAR=value

# --- several assignments in one command ---
FIRST=one SECOND=two

# --- assignment as a command prefix ---
PREFIX=value some_command

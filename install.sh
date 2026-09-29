#!/usr/bin/env bash
# Build the workspace and install the native plugins where the host finds them.
#
# The host (`todo`) is the drop-in todo.sh replacement; the plugin binaries are
# symlinked into $TODO_RUST_ACTIONS_DIR (default: ~/.config/todo/rust-actions),
# which is searched before the legacy action directories. Existing shell/Python
# add-ons keep working untouched.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
dest=${TODO_RUST_ACTIONS_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/todo/rust-actions}

echo "==> building release"
cargo build --release --manifest-path "$here/Cargo.toml"

bin="$here/target/release"
mkdir -p "$dest"

echo "==> installing plugins into $dest"
# Auto-discover every plugin binary so adding a crate needs no change here.
for path in "$bin"/*; do
    [ -f "$path" ] && [ -x "$path" ] || continue
    name=$(basename "$path")
    case $name in
        todo|*.*) continue ;; # skip the host and library artifacts
    esac
    ln -sf "$path" "$dest/$name"
done
# One binary, several action names.
link_alias() { [ -x "$bin/$1" ] && ln -sf "$bin/$1" "$dest/$2"; }
link_alias dep blocked
link_alias dep ready
link_alias dep next
# `editor-open` / `link-open` are dispatched by the open/link binaries.
link_alias open editor-open
link_alias link link-open

if [ -n "${TODO_BIN_DIR:-}" ]; then
    mkdir -p "$TODO_BIN_DIR"
    ln -sf "$bin/todo" "$TODO_BIN_DIR/todo"
    echo "==> linked host into $TODO_BIN_DIR/todo"
fi

echo "==> done. host: $bin/todo"
echo "    add it to PATH, then: todo -d path/to/.todo/config help"

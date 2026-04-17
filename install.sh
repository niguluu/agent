#!/usr/bin/env bash
# Custom install script for junie -> `aj` command
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"
CMD_NAME="${CMD_NAME:-aj}"

echo ">> building release in $SCRIPT_DIR"
cd "$SCRIPT_DIR"
cargo build --release

SRC_BIN="$SCRIPT_DIR/target/release/junie"
if [ ! -x "$SRC_BIN" ]; then
  echo "!! build artifact not found at $SRC_BIN" >&2
  exit 1
fi

mkdir -p "$BIN_DIR"
DEST="$BIN_DIR/$CMD_NAME"

echo ">> linking $SRC_BIN -> $DEST"
ln -sf "$SRC_BIN" "$DEST"

# Ensure BIN_DIR is on PATH
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *)
    SHELL_NAME="$(basename "${SHELL:-bash}")"
    case "$SHELL_NAME" in
      zsh)  RC="$HOME/.zshrc" ;;
      bash) RC="$HOME/.bashrc" ;;
      fish) RC="$HOME/.config/fish/config.fish" ;;
      *)    RC="$HOME/.profile" ;;
    esac
    LINE="export PATH=\"$BIN_DIR:\$PATH\""
    if [ -f "$RC" ] && grep -Fq "$LINE" "$RC"; then
      :
    else
      mkdir -p "$(dirname "$RC")"
      echo "$LINE" >> "$RC"
      echo ">> added $BIN_DIR to PATH in $RC (restart shell to apply)"
    fi
    ;;
esac

echo ">> done. run '$CMD_NAME' from anywhere."

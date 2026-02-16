#!/bin/sh
git config core.hooksPath .githooks
echo "✅ Git hooks installed. Pre-commit will run fmt + clippy."

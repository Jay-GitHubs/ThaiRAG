#!/bin/sh
# Point this clone at the repository's hooks (data guard pre-commit).
set -e
cd "$(git rev-parse --show-toplevel)"
chmod +x .githooks/*
git config core.hooksPath .githooks
echo "hooks installed: core.hooksPath=.githooks"
echo "optional: put customer names (one per line) in ~/.config/thairag/forbidden-terms.txt;"
echo "          the pre-commit guard blocks any staged file containing them. Never commit that file."

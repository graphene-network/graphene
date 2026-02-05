#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

cd "$REPO_ROOT"

if [[ ! -d poky ]]; then
  git clone --depth 1 --branch "$YOCTO_RELEASE" https://git.yoctoproject.org/poky poky
else
  echo "poky already cloned, skipping"
fi

if [[ ! -d meta-rust-bin ]]; then
  git clone --depth 1 https://github.com/rust-embedded/meta-rust-bin.git meta-rust-bin
else
  echo "meta-rust-bin already cloned, skipping"
fi

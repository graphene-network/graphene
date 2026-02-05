#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

cd "$REPO_ROOT"

if [[ ! -d poky ]]; then
  git clone --depth 1 --branch "$YOCTO_RELEASE" https://git.yoctoproject.org/poky poky
else
  if git -C poky diff --quiet && git -C poky diff --cached --quiet; then
    git -C poky fetch --depth 1 origin "$YOCTO_RELEASE"
    git -C poky checkout -B "$YOCTO_RELEASE" "origin/$YOCTO_RELEASE"
  else
    echo "poky has local changes; skipping branch update"
  fi
fi

if [[ ! -d meta-rust-bin ]]; then
  git clone --depth 1 https://github.com/rust-embedded/meta-rust-bin.git meta-rust-bin
else
  if git -C meta-rust-bin diff --quiet && git -C meta-rust-bin diff --cached --quiet; then
    default_branch=$(git -C meta-rust-bin symbolic-ref --short refs/remotes/origin/HEAD 2>/dev/null | sed 's@^origin/@@')
    if [[ -z "$default_branch" ]]; then
      default_branch="master"
    fi
    git -C meta-rust-bin fetch --depth 1 origin "$default_branch"
    git -C meta-rust-bin checkout -B "$default_branch" "origin/$default_branch"
  else
    echo "meta-rust-bin has local changes; skipping branch update"
  fi
fi

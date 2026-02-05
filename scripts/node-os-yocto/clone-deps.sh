#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

cd "$REPO_ROOT"

function clone_or_update() {
  local repo="$1"
  local dir="$2"
  local branch="$3"

  if [[ ! -d "$dir/.git" ]]; then
    git clone --depth 1 --branch "$branch" "$repo" "$dir"
    return
  fi

  if git -C "$dir" diff --quiet && git -C "$dir" diff --cached --quiet; then
    git -C "$dir" fetch --depth 1 origin "$branch"
    git -C "$dir" checkout -B "$branch" "origin/$branch"
  else
    echo "$dir has local changes; skipping branch update"
  fi
}

function resolve_branch() {
  local repo="$1"
  local requested="$2"
  local fallback="$3"

  if git ls-remote --heads "$repo" "$requested" | rg -q "."; then
    echo "$requested"
    return
  fi

  echo "WARNING: Branch ${requested} not found in ${repo}; falling back to ${fallback}." >&2
  echo "$fallback"
}

if [[ "$YOCTO_MODE" == "layers" ]]; then
  mkdir -p "$YOCTO_LAYERS_DIR"
  bitbake_branch=$(resolve_branch "https://git.openembedded.org/bitbake" "$YOCTO_BRANCH" "master")
  oecore_branch=$(resolve_branch "https://git.openembedded.org/openembedded-core" "$YOCTO_BRANCH" "master")
  metayocto_branch=$(resolve_branch "https://git.yoctoproject.org/meta-yocto" "$YOCTO_BRANCH" "master")
  clone_or_update "https://git.openembedded.org/bitbake" "$YOCTO_LAYERS_DIR/bitbake" "$bitbake_branch"
  clone_or_update "https://git.openembedded.org/openembedded-core" "$YOCTO_LAYERS_DIR/openembedded-core" "$oecore_branch"
  clone_or_update "https://git.yoctoproject.org/meta-yocto" "$YOCTO_LAYERS_DIR/meta-yocto" "$metayocto_branch"
else
  clone_or_update "https://git.yoctoproject.org/poky" "$REPO_ROOT/poky" "$YOCTO_RELEASE"
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

#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

cd "$REPO_ROOT"

function clone_or_update() {
  local repo="$1"
  local dir="$2"
  local ref_spec="$3"
  local ref_type="head"
  local ref_name="$ref_spec"

  if [[ "$ref_spec" == *:* ]]; then
    ref_type="${ref_spec%%:*}"
    ref_name="${ref_spec#*:}"
  fi

  if [[ ! -d "$dir/.git" ]]; then
    git clone --depth 1 --branch "$ref_name" "$repo" "$dir"
    return
  fi

  if git -C "$dir" diff --quiet && git -C "$dir" diff --cached --quiet; then
    if [[ "$ref_type" == "tag" ]]; then
      git -C "$dir" fetch --depth 1 origin "refs/tags/$ref_name:refs/tags/$ref_name"
      git -C "$dir" checkout -B "$ref_name" "refs/tags/$ref_name"
    else
      git -C "$dir" fetch --depth 1 origin "$ref_name"
      git -C "$dir" checkout -B "$ref_name" "origin/$ref_name"
    fi
  else
    echo "$dir has local changes; skipping branch update"
  fi
}

function resolve_ref() {
  local repo="$1"
  local requested="$2"
  local fallback="$3"
  local strict="${4:-1}"

  if git ls-remote --tags "$repo" "$requested" | rg -q "."; then
    echo "tag:$requested"
    return
  fi

  if git ls-remote --heads "$repo" "$requested" | rg -q "."; then
    echo "head:$requested"
    return
  fi

  if [[ "$strict" -eq 1 ]]; then
    echo "ERROR: Ref ${requested} not found in ${repo}." >&2
    return 1
  fi

  echo "WARNING: Ref ${requested} not found in ${repo}; falling back to ${fallback}." >&2
  echo "head:$fallback"
}

if [[ "$YOCTO_MODE" == "layers" ]]; then
  mkdir -p "$YOCTO_LAYERS_DIR"
  bitbake_ref=$(resolve_ref "https://git.openembedded.org/bitbake" "$YOCTO_BITBAKE_REF" "master" "$YOCTO_STRICT")
  oecore_ref=$(resolve_ref "https://git.openembedded.org/openembedded-core" "$YOCTO_OECORE_REF" "master" "$YOCTO_STRICT")
  metayocto_ref=$(resolve_ref "https://git.yoctoproject.org/meta-yocto" "$YOCTO_METAYOCTO_REF" "master" "$YOCTO_STRICT")
  clone_or_update "https://git.openembedded.org/bitbake" "$YOCTO_LAYERS_DIR/bitbake" "$bitbake_ref"
  clone_or_update "https://git.openembedded.org/openembedded-core" "$YOCTO_LAYERS_DIR/openembedded-core" "$oecore_ref"
  clone_or_update "https://git.yoctoproject.org/meta-yocto" "$YOCTO_LAYERS_DIR/meta-yocto" "$metayocto_ref"
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

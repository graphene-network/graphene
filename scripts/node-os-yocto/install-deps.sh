#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

apt_packages=(
  gawk wget git diffstat unzip texinfo gcc build-essential
  chrpath socat cpio python3 python3-pip python3-pexpect
  xz-utils debianutils iputils-ping python3-git python3-jinja2
  python3-subunit zstd liblz4-tool file locales libacl1 qemu-system-x86
)

sudo apt-get update
sudo apt-get install -y "${apt_packages[@]}"

sudo locale-gen en_US.UTF-8

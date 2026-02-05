#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

cd "$YOCTO_TOP"
export BBSERVER="${BBSERVER:-}"
export ZSH_NAME="${ZSH_NAME:-}"
set +u
if [[ -n "${YOCTO_TEMPLATECONF}" ]]; then
  TEMPLATECONF="${YOCTO_TEMPLATECONF}" source "${YOCTO_INIT_ENV}" "$BUILD_DIR"
else
  source "${YOCTO_INIT_ENV}" "$BUILD_DIR"
fi
set -u

# Preflight: BitBake needs UNIX sockets and POSIX semaphores (multiprocessing.Lock).
python3 - <<'PY'
import os
import socket
import multiprocessing as mp
import sys

build_dir = os.environ.get("BUILD_DIR", "/tmp/yocto-build")
sock_path = os.path.join(build_dir, "bitbake-preflight.sock")

def fail(msg, err):
    sys.stderr.write("ERROR: BitBake preflight failed: %s: %s\n" % (msg, err))
    sys.stderr.write("ERROR: This environment must allow UNIX sockets and POSIX semaphores.\n")
    sys.stderr.write("ERROR: If you are in a restricted shell or service unit, relax IPC restrictions and retry.\n")
    sys.exit(1)

try:
    mp.Lock()
except Exception as e:
    fail("multiprocessing.Lock() not permitted", e)

try:
    if os.path.exists(sock_path):
        os.unlink(sock_path)
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.bind(sock_path)
    s.listen(1)
    s.close()
except Exception as e:
    fail("UNIX domain socket bind not permitted", e)
finally:
    if os.path.exists(sock_path):
        os.unlink(sock_path)
PY

set +e
bitbake graphene-node-image 2>&1 | tee "$BUILD_DIR/bitbake.log"
bb_status=${PIPESTATUS[0]}
tee_status=${PIPESTATUS[1]:-0}
set -e

if [[ $bb_status -ne 0 ]]; then
  echo "BitBake build failed (exit ${bb_status})"
  tail -n 100 "$BUILD_DIR/bitbake.log"
  exit "$bb_status"
fi

if [[ $tee_status -ne 0 && $tee_status -ne 141 ]]; then
  echo "WARNING: tee failed (exit ${tee_status}); build succeeded."
fi

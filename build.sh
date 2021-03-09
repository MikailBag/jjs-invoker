#!/usr/bin/env
set -euxo pipefail
# Builds invoker docker image
# All arguments are passed to cargo

RUSTC_BOOTSTRAP=1 cargo build -p invoker "$@" -Zunstable-options --out-dir ./out

docker build -t jjs-invoker .

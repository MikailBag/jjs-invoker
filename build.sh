#!/usr/bin/env
set -euxo pipefail
# Builds invoker docker image
# All arguments are passed to cargo

RUSTC_BOOTSTRAP=1 RUSTFLAGS='-Ctarget-feature=+crt-static' cargo build -p invoker "$@" -Zunstable-options --out-dir ./out --target=x86_64-unknown-linux-gnu

docker build -t jjs-invoker .

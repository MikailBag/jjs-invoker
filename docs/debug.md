# Interactive debugging
Sometimes you may want to debug failing `InvocationRequest` using tools like `strace` or `gdb`. Invoker
provides interactive debugging feature to facilitate this.

When interactive debugging is enabled invoker will wait after each sandbox creation until explicitly resumed.

Debugging workflow looks like:

1. Launch invoker with interactive debugging enabled (see the flags starting with `--interactive-debug`).
2. Send a request.
3. Connect to sandbox using desired debugging tools.
4. Resume sandbox.

## `strace-debugger`
We maintain an http-based debugger which automatically collects strace logs when enabled. This debugger
does not disrupt invoker workflow.
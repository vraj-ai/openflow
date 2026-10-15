You are working in a git worktree of the OpenFlow repo (a local Wispr Flow clone in Rust). Read AGENTS.md first.
Environment: run `. /workspace/openflow/scripts/dev-env.sh` in every shell before cargo (keeps CARGO_HOME local, sets ORT_DYLIB_PATH and OPENFLOW_MODELS=/workspace/openflow/models). Use `CARGO_BUILD_JOBS=3`.
Rules: edit ONLY the files listed as yours. Do not touch other files, do not run git commit/push, do not install system packages, do not write outside this worktree. No GPL deps. Keep comments minimal (only non-obvious why). Tests must assert literal expected values through the public API.
When done, print a short summary: files changed, test command run, and its final `test result:` lines.

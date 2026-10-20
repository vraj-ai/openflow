# Doctor

`openflow doctor` reports the settings path, the models dir, accelerator backends, the ONNX Runtime library, permissions, and which catalog models are downloaded.

## Sub-features

- `doctor-paths` prints the settings file and models dir in use.
- `doctor-ort` prints the ONNX Runtime library path and whether it loads.
- `doctor-models` lists each catalog model as `ok` or `missing`.

## How to get to it (user POV)

- Run `openflow doctor` in a terminal.

## Driving it with verify.sh

Preconditions:

- The release binary is built and `OPENFLOW_HOME` points at a fresh dir.

- **Run doctor.** Run `target/release/openflow doctor`. Exit code `0`.
- **Check paths.** stdout has the exact line `models dir: $OPENFLOW_MODELS` and `onnxruntime: $ORT_DYLIB_PATH (true)`.
- **Check models.** Lines such as `  parakeet-tdt-0.6b-v3       ok` show what later features can use.

## Gotchas

- Doctor exits `0` even when models are missing or settings point at an undownloaded model. Read the table instead of trusting the exit code.
- With default settings it warns that `whisper-large-v3-turbo` and `qwen3-4b-instruct-2507` are not downloaded on the dev box. That warning is expected there.

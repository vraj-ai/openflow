# Transcribe

`openflow transcribe` decodes a WAV, trims silence with Silero VAD, and prints the speech model's raw transcript with no cleanup.

## Sub-features

- `transcribe-parakeet` uses the default Parakeet TDT model.
- `transcribe-no-vad` skips VAD trimming (`--no-vad`).
- `transcribe-whisper` routes to Whisper when `stt.language` is outside Parakeet's languages.

## How to get to it (user POV)

- Run `openflow transcribe [--no-vad] <file.wav>`.

## Driving it with verify.sh

Preconditions:

- Doctor lists `parakeet-tdt-0.6b-v3  ok` and `silero-vad  ok`.

- **Transcribe the fixture.** Run `openflow transcribe crates/engines/tests/fixtures/speech_en.wav`. Exit code `0`. stdout is `mister Quilter is the apostle of the middle classes, and we are glad to welcome his gospel.`

## Gotchas

- The output is raw ASR. The lowercase "mister" and the trailing period are expected. Cleanup happens in `dictate`.
- The Whisper route needs `whisper-large-v3-turbo` by default, which is not downloaded on the dev box.

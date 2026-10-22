# Dictate

`openflow dictate` runs the whole dictation pipeline on a WAV (VAD, speech-to-text, cleanup, snippets, styles) against a simulated desktop and prints what the daemon would paste, as JSON.

## Sub-features

- `dictate-paste` pastes cleaned text into the simulated focused field.
- `dictate-command` runs Command Mode on a simulated selection (`--mode command --selection <text>`).
- `dictate-learn` reports dictionary entries a later correction would teach (`--learn --field <text>`).

## How to get to it (user POV)

- A user holds the dictate hotkey, speaks and releases. The daemon then runs this pipeline and pastes.
- On the dev box, run `openflow dictate [--no-llm] [--app <id>] <file.wav>`.

## Driving it with verify.sh

Preconditions:

- Doctor lists `parakeet-tdt-0.6b-v3  ok` and `silero-vad  ok`.

- **Dictate the fixture.** Run `openflow dictate --no-llm crates/engines/tests/fixtures/speech_en.wav`. Exit code `0`. stdout is exactly this JSON, and verify.sh compares all of it:

  ```json
  {
    "keys": "[Paste]",
    "learned": [],
    "notifications": [],
    "outcome": "Inserted(\"Mister Quilter is the apostle of the middle classes, and we are glad to welcome his gospel\")",
    "pasted": [
      "Mister Quilter is the apostle of the middle classes, and we are glad to welcome his gospel"
    ]
  }
  ```

## Gotchas

- The fixture is a LibriSpeech sentence, not a spoken command. Command Mode runs with it complete the plumbing but do not prove the edit quality.
- Command Mode needs an LLM. Run `openflow set cleanup.llm_model qwen3-1.7b` first on the dev box.
- The real daemon's hotkey, microphone and paste are not exercised here.

# Text cleanup

`openflow clean` runs the dictation cleanup pipeline on typed text: rule passes (fillers, stutters, self-corrections, lists, spoken commands, tidy), then an optional local LLM pass with a drift guard.

## Sub-features

- `clean-rules` runs only the rule passes (`--no-llm`).
- `clean-llm` adds the LLM pass when the text has at least `min_words_for_llm` words.
- `clean-style` picks a tone from the app id (`--app com.apple.mail`).

## How to get to it (user POV)

- Run `openflow clean [--no-llm] [--app <id>] "<text>"`.
- The same pipeline runs inside `dictate` and the daemon after transcription.

## Driving it with verify.sh

Preconditions:

- `OPENFLOW_HOME` is fresh. For the LLM pass, doctor lists `qwen3-1.7b  ok`.

- **Rule pass.** Run `openflow clean --no-llm "um, so, uh, I think we should ship it on on Friday"`. stdout is `So I think we should ship it on on Friday`.
- **Select the test LLM.** Run `openflow set cleanup.llm_model qwen3-1.7b`. stdout echoes `cleanup.llm_model = qwen3-1.7b`.
- **LLM pass.** Run `openflow clean "um, so, uh, I think we should ship it on on Friday"`. stdout is `So I think we should ship it on Friday` and stderr shows `llm: true, rejected: None`.

## Gotchas

- The default LLM is `qwen3-4b-instruct-2507`, which is not downloaded on the dev box. Without the `set` step the LLM pass exits `1` with "model ... is not downloaded".
- The rule pass collapses stutters only for a short list of function words. "on on" is not on it, so only the LLM removes it.
- Tidy never adds a final period.
- The LLM pass takes about 20 s on the dev box CPU.

"""Regenerate crates/core/src/catalog.json from the Hugging Face API.

Pins every file to a commit SHA and records its size and SHA-256 (the LFS oid),
so downloads are reproducible and verified. Run: python3 scripts/gen_catalog.py
"""
import json, sys, urllib.request

def api(path):
    with urllib.request.urlopen(f"https://huggingface.co/api/{path}") as r:
        return json.load(r)

# id, kind, name, description, ram_mb (est.), [(repo, remote path, local name)]
MODELS = [
    ("parakeet-tdt-0.6b-v3", "stt", "Parakeet TDT 0.6B v3", "Default. 25 European languages, punctuation, very fast.", 1000,
     [("istupakov/parakeet-tdt-0.6b-v3-onnx", f, f) for f in ["config.json", "nemo128.onnx", "encoder-model.int8.onnx", "decoder_joint-model.int8.onnx", "vocab.txt"]]),
    ("parakeet-tdt-0.6b-v2", "stt", "Parakeet TDT 0.6B v2", "English only, slightly better English accuracy.", 1000,
     [("istupakov/parakeet-tdt-0.6b-v2-onnx", f, f) for f in ["config.json", "nemo128.onnx", "encoder-model.int8.onnx", "decoder_joint-model.int8.onnx", "vocab.txt"]]),
    ("silero-vad", "vad", "Silero VAD v5", "Voice activity detection; trims silence before transcription.", 20,
     [("onnx-community/silero-vad", "onnx/model.onnx", "silero_vad.onnx")]),
    ("whisper-large-v3-turbo", "multilingual", "Whisper large-v3-turbo", "Fallback for Hindi and other languages Parakeet lacks (99 languages).", 2000,
     [("openai/whisper-large-v3-turbo", f, f) for f in ["config.json", "tokenizer.json", "model.safetensors"]]),
    ("whisper-tiny", "multilingual", "Whisper tiny", "Small multilingual fallback for low-memory machines.", 300,
     [("openai/whisper-tiny", f, f) for f in ["config.json", "tokenizer.json", "model.safetensors"]]),
    ("qwen3-4b-instruct-2507", "llm", "Qwen3 4B Instruct 2507 (Q4_K_M)", "Default cleanup and Command Mode model for 16 GB Macs.", 3200,
     [("unsloth/Qwen3-4B-Instruct-2507-GGUF", "Qwen3-4B-Instruct-2507-Q4_K_M.gguf", "model.gguf"), ("Qwen/Qwen3-4B-Instruct-2507", "tokenizer.json", "tokenizer.json")]),
    ("qwen3-1.7b", "llm", "Qwen3 1.7B (Q4_K_M)", "Fast option, roughly half the latency of the 4B model.", 1500,
     [("unsloth/Qwen3-1.7B-GGUF", "Qwen3-1.7B-Q4_K_M.gguf", "model.gguf"), ("Qwen/Qwen3-1.7B", "tokenizer.json", "tokenizer.json")]),
    ("gemma-3-1b-it", "llm", "Gemma 3 1B IT (Q4_K_M)", "Smallest option.", 1100,
     [("unsloth/gemma-3-1b-it-GGUF", "gemma-3-1b-it-Q4_K_M.gguf", "model.gguf"), ("unsloth/gemma-3-1b-it", "tokenizer.json", "tokenizer.json")]),
    ("llama-3.2-3b-instruct", "llm", "Llama 3.2 3B Instruct (Q4_K_M)", "Alternative 3B model.", 2600,
     [("unsloth/Llama-3.2-3B-Instruct-GGUF", "Llama-3.2-3B-Instruct-Q4_K_M.gguf", "model.gguf"), ("unsloth/Llama-3.2-3B-Instruct", "tokenizer.json", "tokenizer.json")]),
    ("qwen3-8b", "llm", "Qwen3 8B (Q4_K_M)", "Larger model for PCs with an 8 GB+ GPU (RTX 3070).", 6000,
     [("unsloth/Qwen3-8B-GGUF", "Qwen3-8B-Q4_K_M.gguf", "model.gguf"), ("Qwen/Qwen3-8B", "tokenizer.json", "tokenizer.json")]),
    ("qwen3-14b", "llm", "Qwen3 14B (Q4_K_M)", "Largest option for 24 GB GPUs (RTX 3090).", 10000,
     [("unsloth/Qwen3-14B-GGUF", "Qwen3-14B-Q4_K_M.gguf", "model.gguf"), ("Qwen/Qwen3-14B", "tokenizer.json", "tokenizer.json")]),
]

catalog = []
for mid, kind, name, desc, ram, files in MODELS:
    out_files = []
    for repo, remote, local in files:
        info = api(f"models/{repo}")
        sha = info["sha"]
        parent = remote.rsplit("/", 1)[0] if "/" in remote else ""
        tree = {f["path"]: f for f in api(f"models/{repo}/tree/{sha}/{parent}")}
        entry = tree[remote]
        out_files.append({
            "url": f"https://huggingface.co/{repo}/resolve/{sha}/{remote}",
            "name": local,
            "size": entry["size"],
            "sha256": entry["lfs"]["oid"] if "lfs" in entry else None,
        })
    catalog.append({"id": mid, "kind": kind, "name": name, "description": desc, "ram_mb": ram, "files": out_files})
    print(mid, sum(f["size"] for f in out_files) // 1_000_000, "MB", file=sys.stderr)

with open("crates/core/src/catalog.json", "w") as f:
    json.dump(catalog, f, indent=1)
    f.write("\n")

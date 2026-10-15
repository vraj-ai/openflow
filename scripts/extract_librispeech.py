"""Extract the LibriSpeech dummy split (CC-BY 4.0) into 16 kHz WAVs plus a JSONL manifest."""
import io, json, sys
import pyarrow.parquet as pq
import soundfile as sf

src, out_dir, manifest = sys.argv[1], sys.argv[2], sys.argv[3]
table = pq.read_table(src).to_pylist()
with open(manifest, "w") as m:
    for row in table:
        data, sr = sf.read(io.BytesIO(row["audio"]["bytes"]), dtype="float32")
        assert sr == 16000, sr
        name = f"ls_{row['id']}.wav"
        sf.write(f"{out_dir}/{name}", data, sr, subtype="PCM_16")
        m.write(json.dumps({"audio": name, "text": row["text"].lower(), "source": "librispeech-dummy"}) + "\n")
print(len(table))

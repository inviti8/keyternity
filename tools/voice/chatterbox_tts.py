# /// script
# requires-python = ">=3.10,<3.13"
# dependencies = ["chatterbox-tts>=0.1.4", "torchaudio"]
# ///
"""Keyternity's Chatterbox helper (docs/LIP_SYNC.md §8): speak a line with Resemble
AI's Chatterbox (MIT), optionally cloning a reference clip's voice, to a WAV.

Keyternity writes this file to its data folder and runs it with `uv run` (which
installs Chatterbox and PyTorch the first time), or with a Python that already has
chatterbox-tts. Chatterbox's output carries Resemble's imperceptible watermark.

    uv run chatterbox_tts.py --text-file line.txt --out line.wav [--reference voice.wav]
        [--language en] [--exaggeration 0.5]
"""

import argparse

import torch
import torchaudio as ta


def device() -> str:
    if torch.cuda.is_available():
        return "cuda"
    if getattr(torch.backends, "mps", None) and torch.backends.mps.is_available():
        return "mps"
    return "cpu"


def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("--text-file", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--reference", default="")
    p.add_argument("--language", default="en")
    p.add_argument("--exaggeration", type=float, default=0.5)
    a = p.parse_args()
    text = open(a.text_file, encoding="utf-8").read().strip()
    ref = a.reference or None
    dev = device()
    if a.language == "en":
        from chatterbox.tts import ChatterboxTTS

        model = ChatterboxTTS.from_pretrained(device=dev)
        wav = model.generate(text, audio_prompt_path=ref, exaggeration=a.exaggeration)
    else:
        from chatterbox.mtl_tts import ChatterboxMultilingualTTS

        model = ChatterboxMultilingualTTS.from_pretrained(device=dev)
        wav = model.generate(text, language_id=a.language, audio_prompt_path=ref, exaggeration=a.exaggeration)
    ta.save(a.out, wav, model.sr)


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Synthesizes the key click sounds embedded in dianmo.exe (按键音, sound.rs).

    python3 crates/dianmo/res/sounds/gen.py

Writes 48 kHz mono 16-bit WAVs next to this script: {crisp,soft}_{char,func}.wav, each a few KB.
Deterministic (fixed noise seed), so re-running gives the same files.
"""
import math
import os
import random
import struct
import wave

RATE = 48000
PEAK = 0.7  # ~-3 dBFS: the 大 volume plays them as is


def synth(ms, tones, noise, noise_lp, attack_ms, decay_ms, seed):
    """`tones`: [(freq Hz, amplitude, decay ms)], `noise`: amplitude of a one-pole low-passed
    noise burst (`noise_lp` = cutoff Hz) with the overall `decay_ms`."""
    rnd = random.Random(seed)
    n = int(RATE * ms / 1000)
    out = []
    lp = 0.0
    a = 1.0 - math.exp(-2 * math.pi * noise_lp / RATE)
    hp_prev_in = hp_prev_out = 0.0
    for i in range(n):
        t = i / RATE
        env = min(1.0, t * 1000 / attack_ms) * math.exp(-t * 1000 / decay_ms)
        v = 0.0
        for f, amp, d in tones:
            v += amp * math.sin(2 * math.pi * f * t) * math.exp(-t * 1000 / d)
        lp += a * (rnd.uniform(-1, 1) - lp)
        # One-pole high-pass at ~150 Hz keeps the noise from sounding like a bump.
        hp = 0.98 * (hp_prev_out + lp - hp_prev_in)
        hp_prev_in, hp_prev_out = lp, hp
        v += noise * hp * 3.0
        out.append(v * env)
    # Fade the last 3 ms to avoid a click at the end.
    fade = int(RATE * 0.003)
    for i in range(fade):
        out[n - 1 - i] *= i / fade
    peak = max(abs(x) for x in out) or 1.0
    return [x / peak * PEAK for x in out]


SOUNDS = {
    # 清脆: a bright, short tick (like a phone keyboard).
    "crisp_char": dict(ms=38, tones=[(3200, 0.6, 9), (1900, 0.35, 14)], noise=0.5, noise_lp=9000, attack_ms=0.4, decay_ms=10, seed=1),
    "crisp_func": dict(ms=48, tones=[(1500, 0.6, 14), (900, 0.4, 20)], noise=0.4, noise_lp=6000, attack_ms=0.5, decay_ms=14, seed=2),
    # 柔和: a soft, low tap.
    "soft_char": dict(ms=45, tones=[(820, 0.7, 14), (1400, 0.2, 8)], noise=0.25, noise_lp=2500, attack_ms=2.0, decay_ms=13, seed=3),
    "soft_func": dict(ms=55, tones=[(480, 0.8, 20), (950, 0.2, 10)], noise=0.2, noise_lp=1800, attack_ms=2.5, decay_ms=17, seed=4),
}


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    for name, p in SOUNDS.items():
        samples = synth(**p)
        path = os.path.join(here, name + ".wav")
        with wave.open(path, "wb") as w:
            w.setnchannels(1)
            w.setsampwidth(2)
            w.setframerate(RATE)
            w.writeframes(b"".join(struct.pack("<h", int(round(x * 32767))) for x in samples))
        print(f"{path}: {len(samples)} samples, {os.path.getsize(path)} bytes")


if __name__ == "__main__":
    main()

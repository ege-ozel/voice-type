"""Generate the soft processing cue and match its LUFS to the existing sound packs.

Build-time only: numpy and pyloudnorm are required, not bundled with the application.
Example: python tools/make_processing_sound.py --dependencies build/audio-tools
"""
import argparse
import json
import random
import wave
from pathlib import Path
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dependencies', type=Path)
    args = parser.parse_args()
    if args.dependencies:
        sys.path.insert(0, str(args.dependencies.resolve()))
    import numpy as np
    import pyloudnorm as pyln

    assets = Path(__file__).resolve().parents[1] / 'assets' / 'sounds'
    rate = 48_000

    def loudness(samples, sample_rate):
        if len(samples) < sample_rate * 0.4:
            samples = np.pad(samples, ((0, int(sample_rate * 0.4) - len(samples)), (0, 0)))
        return float(pyln.Meter(sample_rate, filter_class='DeMan').integrated_loudness(samples))

    measurements = {}
    for path in sorted(assets.glob('*.wav')):
        if path.name == 'processing.wav':
            continue
        with wave.open(str(path), 'rb') as sound:
            assert sound.getsampwidth() == 2
            data = np.frombuffer(sound.readframes(sound.getnframes()), dtype='<i2')
            samples = data.reshape(-1, sound.getnchannels()) / 32768.0
            measurements[path.name] = loudness(samples, sound.getframerate())

    # Keep this incidental cue slightly quieter than the median of the six supplied sounds.
    target = float(np.median(list(measurements.values()))) - 1.5
    rng = random.Random(304)
    low = slow = 0.0
    samples = []
    for i in range(int(rate * 0.32)):
        t = i / rate
        low += 0.13 * (rng.uniform(-1, 1) - low)
        slow += 0.025 * (low - slow)
        envelope = np.exp(-0.5 * ((t - 0.075) / 0.027) ** 2) + 0.7 * np.exp(-0.5 * ((t - 0.205) / 0.036) ** 2)
        edge = min(1, i / 480, (rate * 0.32 - 1 - i) / 480)
        samples.append((low - slow) * envelope * edge)
    # Small lead-in allows the output device to open before the quiet first transient.
    mono = np.pad(np.asarray(samples), (int(rate * 0.12), int(rate * 0.04)))
    stereo = np.repeat(mono[:, None], 2, axis=1)
    stereo *= 10 ** ((target - loudness(stereo, rate)) / 20)
    assert np.max(np.abs(stereo)) < 1.0, 'Normalization would clip'
    pcm = np.rint(stereo * 32767).astype('<i2')
    with wave.open(str(assets / 'processing.wav'), 'wb') as sound:
        sound.setparams((2, 2, rate, 0, 'NONE', 'not compressed'))
        sound.writeframes(pcm.tobytes())
    measurements['processing.wav'] = loudness(pcm.astype(float) / 32768.0, rate)
    print(json.dumps({name: round(value, 2) for name, value in measurements.items()}, indent=2))


if __name__ == '__main__':
    main()

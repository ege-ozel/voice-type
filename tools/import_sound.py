"""Import a 16/24-bit PCM WAV as canonical 16-bit PCM for embedding in the EXE."""
import argparse
from pathlib import Path
import wave


def import_sound(source: Path, target: Path) -> None:
    with wave.open(str(source), 'rb') as sound:
        if sound.getcomptype() != 'NONE' or sound.getsampwidth() not in (2, 3):
            raise ValueError('Expected an uncompressed 16-bit or 24-bit PCM WAV')
        channels, rate = sound.getnchannels(), sound.getframerate()
        data = sound.readframes(sound.getnframes())
        if sound.getsampwidth() == 3:
            # Drop only the least significant byte; preserve channels, rate and duration.
            data = b''.join(data[i + 1:i + 3] for i in range(0, len(data), 3))
    target.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(target), 'wb') as out:
        out.setparams((channels, 2, rate, 0, 'NONE', 'not compressed'))
        out.writeframes(data)
    print(f'{target}: {channels} channels, {rate} Hz, 16-bit, {len(data)} PCM bytes')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('target', type=Path)
    args = parser.parse_args()
    import_sound(args.source, args.target)

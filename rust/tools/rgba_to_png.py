"""Ham RGBA dosyalarını (`<ad>_<g>x<y>.rgba`) sıkıştırılmış PNG'ye çevirir; yalnızca standart kütüphane.

Kullanım: python tools/rgba_to_png.py <rgba klasörü> <png klasörü>
"""
import pathlib
import re
import struct
import sys
import zlib


def png(width, height, rgba):
    def chunk(kind, data):
        body = kind + data
        return struct.pack('>I', len(data)) + body + struct.pack('>I', zlib.crc32(body) & 0xffffffff)

    stride = width * 4
    raw = b''.join(b'\x00' + rgba[y * stride:(y + 1) * stride] for y in range(height))
    header = struct.pack('>IIBBBBB', width, height, 8, 6, 0, 0, 0)
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', header) + chunk(b'IDAT', zlib.compress(raw, 9))
            + chunk(b'IEND', b''))


def main(src, dst):
    dst = pathlib.Path(dst)
    dst.mkdir(parents=True, exist_ok=True)
    for path in sorted(pathlib.Path(src).glob('*.rgba')):
        name, width, height = re.fullmatch(r'(.+)_(\d+)x(\d+)', path.stem).groups()
        out = dst / f'{name}.png'
        out.write_bytes(png(int(width), int(height), path.read_bytes()))
        print(out, f'{int(width)}x{int(height)}', out.stat().st_size, 'bayt')


if __name__ == '__main__':
    main(*sys.argv[1:3])

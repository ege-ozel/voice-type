"""Rebuild the portable Windows executable with this installation's config."""
from pathlib import Path
import shutil
import subprocess
import sys

from tray_app import make_icon

root = Path(__file__).resolve().parent
assets = root / 'build' / 'assets'
assets.mkdir(parents=True, exist_ok=True)
icon = assets / 'dictation.ico'
make_icon().save(icon, sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64)])
subprocess.run([sys.executable, '-m', 'PyInstaller', '--noconfirm', '--clean', '--onefile',
                '--windowed', '--name', 'ChatGPT-Dikte', '--exclude-module', 'numpy', '--icon', str(icon),
                '--distpath', str(root / 'dist'), '--workpath', str(root / 'build'),
                '--specpath', str(root), str(root / 'tray_app.py')], cwd=root, check=True)
shutil.copy2(root / 'bridge-config.json', root / 'dist' / 'bridge-config.json')
print('Ready:', root / 'dist' / 'ChatGPT-Dikte.exe')

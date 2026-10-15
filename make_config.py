"""Bu kuruluma özel rastgele anahtarı üretir: bridge-config.json ve extension/config.js."""
import json
from pathlib import Path
import secrets
import sys

sys.stdout.reconfigure(encoding="utf-8", errors="replace")

root = Path(__file__).resolve().parent
if (root / 'bridge-config.json').exists():
    print('bridge-config.json zaten var; dokunulmadı.')
else:
    token = secrets.token_urlsafe(32)
    (root / 'bridge-config.json').write_text(json.dumps({'token': token}), encoding='utf-8')
    (root / 'extension' / 'config.js').write_text('const BRIDGE_TOKEN = ' + json.dumps(token) + ';\n', encoding='utf-8')
    print('Anahtar üretildi. Chrome eklentisini yeniden yükleyin.')

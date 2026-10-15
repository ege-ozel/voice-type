"""Bas-konuş dikte: kısayola bas, konuş, tekrar bas; metin imlecin olduğu yere yazılır.

Ses tanıma OpenAI'ın transcription API'si (gpt-4o-transcribe) ile yapılır;
ChatGPT'deki dikte ile aynı model ailesi, Türkçe/İngilizce karışık konuşmayı iyi çözer.
"""

import io
import os
import sys
import threading
import time
import wave
import winsound

import keyboard
import numpy as np
import pyperclip
import sounddevice as sd
from openai import OpenAI

HOTKEY = os.environ.get("VOICE_TYPE_HOTKEY", "ctrl+alt+space")
MODEL = os.environ.get("VOICE_TYPE_MODEL", "gpt-4o-transcribe")
# "type": karakter karakter yazar (pano bozulmaz). "paste": panoya koyup Ctrl+V gönderir.
MODE = os.environ.get("VOICE_TYPE_MODE", "type")
SAMPLE_RATE = 16000
PROMPT = (
    "Konuşma Türkçe ve İngilizce karışık olabilir. Yazılım terimleri "
    "(commit, branch, build, deploy, test, API, terminal) İngilizce yazılır."
)

client = OpenAI()
lock = threading.Lock()
recording = False
chunks: list[np.ndarray] = []
stream: sd.InputStream | None = None


def on_audio(indata, frames, time_info, status):
    chunks.append(indata.copy())


def start_recording():
    global recording, stream
    chunks.clear()
    stream = sd.InputStream(samplerate=SAMPLE_RATE, channels=1, dtype="int16", callback=on_audio)
    stream.start()
    recording = True
    winsound.Beep(880, 120)
    print("● Kayıt... (bitirmek için tekrar", HOTKEY + ")", flush=True)


def stop_recording() -> bytes | None:
    global recording, stream
    if stream is not None:
        stream.stop()
        stream.close()
        stream = None
    recording = False
    winsound.Beep(440, 120)
    if not chunks:
        return None
    audio = np.concatenate(chunks)
    if len(audio) < SAMPLE_RATE * 0.3:
        return None
    buf = io.BytesIO()
    with wave.open(buf, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SAMPLE_RATE)
        w.writeframes(audio.tobytes())
    return buf.getvalue()


def wait_for_modifiers_released(timeout=2.0):
    # Kısayolun Ctrl/Alt'ı hâlâ basılıyken yazarsak harfler AltGr/Ctrl kombinasyonu olur.
    end = time.time() + timeout
    while time.time() < end and any(keyboard.is_pressed(k) for k in ("ctrl", "alt", "shift")):
        time.sleep(0.02)


def transcribe_and_output(wav: bytes):
    print("… Çözümleniyor", flush=True)
    try:
        result = client.audio.transcriptions.create(
            model=MODEL,
            file=("audio.wav", wav, "audio/wav"),
            prompt=PROMPT,
        )
    except Exception as e:
        winsound.Beep(220, 300)
        print("✗ Hata:", e, flush=True)
        return
    text = result.text.strip()
    if not text:
        print("(boş)", flush=True)
        return
    pyperclip.copy(text)
    wait_for_modifiers_released()
    if MODE == "paste":
        keyboard.send("ctrl+v")
    else:
        keyboard.write(text, delay=0.003)
    print("✓", text, flush=True)


def toggle():
    with lock:
        if not recording:
            start_recording()
            return
        wav = stop_recording()
    if wav is None:
        print("(çok kısa, atlandı)", flush=True)
        return
    threading.Thread(target=transcribe_and_output, args=(wav,), daemon=True).start()


def main():
    if not os.environ.get("OPENAI_API_KEY"):
        print("OPENAI_API_KEY tanımlı değil. README.md'ye bakın.")
        sys.exit(1)
    keyboard.add_hotkey(HOTKEY, toggle, suppress=True, trigger_on_release=True)
    print(f"Hazır. {HOTKEY} ile kaydı başlat/bitir. Çıkmak için bu pencerede Ctrl+C.", flush=True)
    keyboard.wait()


if __name__ == "__main__":
    main()

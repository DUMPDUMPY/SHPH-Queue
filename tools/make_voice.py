"""Generate the Thai announcement clips listed in voice/voice_lines.json.

Uses Microsoft Edge's online neural voices through the edge-tts package
(needs internet). The default voice th-TH-PremwadeeNeural is female.

    pip install edge-tts
    python tools/make_voice.py                 # writes voice/*.mp3
    python tools/make_voice.py --out D:\\SHPH-Queue\\voice --force

The clips can also be recorded by hand: one file per line in the manifest,
named <name>.mp3 (or .wav/.ogg/.m4a), uploaded from the admin page.
"""

import argparse
import asyncio
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent


async def make_one(sem, name, text, dest, voice, rate, retries=3):
    import edge_tts

    async with sem:
        for attempt in range(1, retries + 1):
            try:
                await edge_tts.Communicate(text, voice, rate=rate).save(str(dest))
                if dest.stat().st_size > 0:
                    return True
            except Exception as e:  # network hiccups are common; retry
                print(f"  {name}: attempt {attempt} failed: {e}", file=sys.stderr)
                await asyncio.sleep(2 * attempt)
        return False


async def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", default=str(ROOT / "voice"), help="folder to write clips to")
    ap.add_argument("--manifest", default=str(ROOT / "voice" / "voice_lines.json"))
    ap.add_argument("--voice", default="th-TH-PremwadeeNeural", help="female: th-TH-PremwadeeNeural, male: th-TH-NiwatNeural")
    ap.add_argument("--rate", default="-5%", help="speaking rate, e.g. -10%% for slower")
    ap.add_argument("--force", action="store_true", help="overwrite existing clips")
    args = ap.parse_args()

    try:
        import edge_tts  # noqa: F401
    except ImportError:
        sys.exit("ต้องติดตั้งก่อน: pip install edge-tts")

    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    lines = json.loads(pathlib.Path(args.manifest).read_text(encoding="utf-8"))
    sem = asyncio.Semaphore(4)
    jobs = []
    for name, text in lines.items():
        dest = out / f"{name}.mp3"
        if dest.exists() and not args.force:
            continue
        jobs.append((name, make_one(sem, name, text, dest, args.voice, args.rate)))
    print(f"สร้างไฟล์เสียง {len(jobs)} ไฟล์ ด้วยเสียง {args.voice} …")
    results = await asyncio.gather(*(j for _, j in jobs))
    failed = [name for (name, _), ok in zip(jobs, results) if not ok]
    print(f"เสร็จ {len(jobs) - len(failed)} ไฟล์ ที่ {out}")
    if failed:
        sys.exit(f"สร้างไม่สำเร็จ {len(failed)} ไฟล์: {', '.join(failed)}")


if __name__ == "__main__":
    asyncio.run(main())

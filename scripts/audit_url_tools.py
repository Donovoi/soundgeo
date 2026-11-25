"""Audit embedded PyInstaller libraries as well as each downloader's outer PE."""
import json
from pathlib import Path
import sys
import tempfile
from PyInstaller.archive.readers import CArchiveReader
from pe_dependencies import imports, system


def main(folder):
    report = {}
    for tool in ('yt-dlp', 'gallery-dl'):
        archive = CArchiveReader(str(folder / (tool + '.exe')))
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binaries = {}
            for name in archive.toc:
                normalized = name.replace('\\', '/')
                if normalized.lower().endswith(('.dll', '.pyd', '.exe')):
                    path = root / normalized
                    assert path.resolve().is_relative_to(root.resolve())
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(archive.extract(name))
                    binaries[normalized] = path
            available = {p.name.lower() for p in binaries.values()}
            assert {'python314.dll', 'vcruntime140.dll'}.issubset(available)
            for name, path in binaries.items():
                required = imports(path)
                missing = {item for item in required if item not in available and not system(item)}
                assert not missing, (tool, name, missing)
                report[tool + '/' + name] = sorted(required)
    (folder / 'URL-TOOLS-PE.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')


if __name__ == '__main__': main(Path(sys.argv[1]))

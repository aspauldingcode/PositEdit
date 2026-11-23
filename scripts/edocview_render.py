#!/usr/bin/env python3
"""edocview-compatible markdown → PDF → PNG pages for PositEdit Kitty preview.

Commands:
  compile <source.md> <target.pdf> <resource_dir>
  pages   <pdf> <output_dir> <pixel_width> [max_pages]
  preview <source.md> <cache_dir> <resource_dir> <pixel_width> [max_pages]

`preview` is the fast path: SHA-cached PDF + width-cached pages, one Python
process, optional first-page-only for immediate Canvas paint.

Tools: pandoc + typst (preferred) or pandoc + xelatex; pymupdf for pages.
Override binaries with POSIT_PANDOC / POSIT_TYPST.
"""
from __future__ import annotations

import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import sys


def find_bin(name: str, env_key: str) -> str | None:
    override = os.environ.get(env_key)
    if override and pathlib.Path(override).is_file():
        return override
    found = shutil.which(name)
    if found:
        return found
    store = pathlib.Path('/nix/store')
    if not store.is_dir():
        return None
    matches = sorted(store.glob(f'*-{name}*/bin/{name}')) + sorted(
        store.glob(f'*-{name}-*/bin/{name}')
    )
    if name == 'pandoc':
        matches = sorted(store.glob('*-pandoc-cli-*/bin/pandoc')) + matches
    for path in reversed(matches):
        if path.is_file() and os.access(path, os.X_OK):
            return str(path)
    return None


def run(command, cwd, env=None):
    result = subprocess.run(
        command,
        cwd=cwd,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        detail = (result.stderr or result.stdout or 'command failed').strip()
        raise RuntimeError(detail[-1600:])


def ensure_typst_fonts(env: dict) -> dict:
    env = dict(env)
    if env.get('TYPST_FONT_PATHS'):
        return env
    candidates = [
        '/System/Library/Fonts',
        '/System/Library/Fonts/Supplemental',
        '/Library/Fonts',
        str(pathlib.Path.home() / 'Library/Fonts'),
        '/usr/share/fonts',
    ]
    existing = [p for p in candidates if pathlib.Path(p).is_dir()]
    if existing:
        env['TYPST_FONT_PATHS'] = os.pathsep.join(existing)
    return env


def compile_markdown(source: pathlib.Path, target: pathlib.Path, original: pathlib.Path):
    pandoc = find_bin('pandoc', 'POSIT_PANDOC')
    if not pandoc:
        raise RuntimeError('pandoc not found (set POSIT_PANDOC)')
    typst = find_bin('typst', 'POSIT_TYPST')
    resource_path = os.pathsep.join((str(original), str(source.parent)))
    env = ensure_typst_fonts(os.environ)
    if typst:
        env = dict(env)
        env['PATH'] = str(pathlib.Path(typst).parent) + os.pathsep + env.get('PATH', '')
        try:
            run(
                [
                    pandoc,
                    str(source),
                    '--from=gfm+tex_math_dollars',
                    f'--resource-path={resource_path}',
                    '--pdf-engine=typst',
                    '-o',
                    str(target),
                ],
                original,
                env,
            )
            return
        except RuntimeError:
            pass
    run(
        [
            pandoc,
            str(source),
            '--from=gfm+tex_math_dollars',
            f'--resource-path={resource_path}',
            '--pdf-engine=xelatex',
            '-o',
            str(target),
        ],
        original,
        env,
    )


def render_pages(pdf: pathlib.Path, output_dir: pathlib.Path, width: int, max_pages: int = 0):
    import fitz

    doc = fitz.open(pdf)
    output_dir.mkdir(parents=True, exist_ok=True)
    width = max(1, int(width))
    rendered = []
    try:
        if not doc.page_count:
            raise ValueError('empty PDF')
        limit = doc.page_count if max_pages <= 0 else min(doc.page_count, max_pages)
        for index in range(limit):
            page = doc[index]
            zoom = min(width / page.rect.width, 3.0)
            path = (output_dir / f'page-{index + 1:04d}.png').resolve()
            # Reuse existing PNG when present (width-stable cache).
            if path.is_file() and path.stat().st_size > 32:
                rendered.append(
                    {
                        'path': str(path),
                        'hash': hashlib.sha256(path.read_bytes()).hexdigest(),
                        'width': int(page.rect.width * zoom),
                        'height': int(page.rect.height * zoom),
                    }
                )
                continue
            pixmap = page.get_pixmap(matrix=fitz.Matrix(zoom, zoom), alpha=False)
            pixmap.save(path)
            rendered.append(
                {
                    'path': str(path),
                    'hash': hashlib.sha256(pixmap.samples).hexdigest(),
                    'width': pixmap.width,
                    'height': pixmap.height,
                }
            )
    finally:
        doc.close()
    return rendered


def pages(pdf: pathlib.Path, output_dir: pathlib.Path, width: int, max_pages: int = 0):
    print(json.dumps(render_pages(pdf, output_dir, width, max_pages)))


def file_sha(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def preview(
    source: pathlib.Path,
    cache_dir: pathlib.Path,
    resource_dir: pathlib.Path,
    width: int,
    max_pages: int = 0,
):
    """Cached compile + raster. Reuses PDF across widths; pages per width."""
    cache_dir.mkdir(parents=True, exist_ok=True)
    digest = file_sha(source)[:32]
    doc_dir = cache_dir / digest
    doc_dir.mkdir(parents=True, exist_ok=True)
    cached_md = doc_dir / 'document.md'
    raw = source.read_bytes()
    if not cached_md.is_file() or cached_md.read_bytes() != raw:
        cached_md.write_bytes(raw)
    pdf = doc_dir / 'document.pdf'
    if not pdf.is_file() or pdf.stat().st_size == 0:
        compile_markdown(cached_md, pdf, resource_dir)
    width = max(1, int(width))
    if max_pages == 1:
        pages_dir = doc_dir / f'pages-{width}-p1'
        print(json.dumps(render_pages(pdf, pages_dir, width, max_pages=1)))
        return
    pages_dir = doc_dir / f'pages-{width}'
    manifest = pages_dir / 'manifest.json'
    if manifest.is_file():
        try:
            data = json.loads(manifest.read_text())
            if isinstance(data, list) and data and all(
                pathlib.Path(item.get('path', '')).is_file() for item in data
            ):
                print(json.dumps(data))
                return
        except Exception:
            pass
    rendered = render_pages(pdf, pages_dir, width, max_pages=0)
    manifest.write_text(json.dumps(rendered))
    print(json.dumps(rendered))


def main():
    try:
        if len(sys.argv) < 2:
            raise ValueError('expected compile, pages, or preview')
        cmd = sys.argv[1]
        if cmd == 'compile':
            compile_markdown(
                pathlib.Path(sys.argv[2]),
                pathlib.Path(sys.argv[3]),
                pathlib.Path(sys.argv[4]),
            )
        elif cmd == 'pages':
            max_pages = int(sys.argv[5]) if len(sys.argv) > 5 else 0
            pages(
                pathlib.Path(sys.argv[2]),
                pathlib.Path(sys.argv[3]),
                int(sys.argv[4]),
                max_pages=max_pages,
            )
        elif cmd == 'preview':
            max_pages = int(sys.argv[6]) if len(sys.argv) > 6 else 0
            preview(
                pathlib.Path(sys.argv[2]),
                pathlib.Path(sys.argv[3]),
                pathlib.Path(sys.argv[4]),
                int(sys.argv[5]),
                max_pages=max_pages,
            )
        else:
            raise ValueError('expected compile, pages, or preview')
    except Exception as error:
        print(str(error)[-1600:], file=sys.stderr)
        sys.exit(1)


if __name__ == '__main__':
    main()

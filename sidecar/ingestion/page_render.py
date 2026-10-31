"""Render PDF page / region clips to PNG; Rust owns cache keys, Python evicts LRU."""

from __future__ import annotations

import base64
import os
import time
from pathlib import Path
from typing import Any

import fitz  # PyMuPDF

_DEFAULT_MAX_CACHE_BYTES = 300 * 1024 * 1024
_MAX_WIDTH = 1400
_DEFAULT_SCALE = 2.0


def _cache_limit_bytes() -> int:
    raw = os.environ.get("COGNITO_PAGE_CACHE_MAX_BYTES")
    if raw is None:
        return _DEFAULT_MAX_CACHE_BYTES
    try:
        return max(0, int(raw))
    except ValueError:
        return _DEFAULT_MAX_CACHE_BYTES


def _touch(path: Path) -> None:
    now = time.time()
    try:
        os.utime(path, (now, now))
    except OSError:
        pass


def _evict_lru(cache_dir: Path, max_bytes: int | None = None) -> None:
    limit = _cache_limit_bytes() if max_bytes is None else max(0, max_bytes)
    if not cache_dir.is_dir():
        return
    files = [p for p in cache_dir.iterdir() if p.is_file() and p.suffix == ".png"]
    total = 0
    sized: list[tuple[float, int, Path]] = []
    for p in files:
        try:
            st = p.stat()
        except OSError:
            continue
        sized.append((st.st_mtime, st.st_size, p))
        total += st.st_size
    if total <= limit:
        return
    sized.sort(key=lambda t: t[0])  # oldest first
    # Always keep the newest file so a single oversized render cannot empty the cache.
    while len(sized) > 1 and total > limit:
        _mtime, size, p = sized.pop(0)
        try:
            p.unlink()
            total -= size
        except OSError:
            continue


def _effective_scale(page: fitz.Page, scale: float, clip: fitz.Rect | None) -> float:
    s = float(scale) if scale and scale > 0 else _DEFAULT_SCALE
    width = float((clip or page.rect).width) * s
    if width > _MAX_WIDTH and width > 0:
        s = s * (_MAX_WIDTH / width)
    return s


def _render_to_path(
    path: Path,
    page: int,
    rect: list[float] | None,
    scale: float,
    out_path: Path,
    *,
    max_cache_bytes: int | None = None,
) -> dict[str, Any]:
    out_path = Path(out_path)
    out_path.parent.mkdir(parents=True, exist_ok=True)

    doc = fitz.open(path)
    try:
        if page < 1 or page > len(doc):
            raise ValueError(f"page out of range: {page}")
        pg = doc.load_page(page - 1)
        clip = None
        if rect is not None and len(rect) >= 4:
            clip = fitz.Rect(float(rect[0]), float(rect[1]), float(rect[2]), float(rect[3]))
            clip = clip & pg.rect
        eff = _effective_scale(pg, scale, clip)
        mat = fitz.Matrix(eff, eff)
        pix = pg.get_pixmap(matrix=mat, clip=clip, alpha=False)
        png = pix.tobytes("png")
        width = int(pix.width)
        height = int(pix.height)
    finally:
        doc.close()

    out_path.write_bytes(png)
    _touch(out_path)
    _evict_lru(out_path.parent, max_cache_bytes)
    return {
        "pngBase64": base64.b64encode(png).decode("ascii"),
        "width": width,
        "height": height,
        "outPath": str(out_path),
    }


def render_region(
    path: str | Path,
    page: int,
    rect: list[float],
    scale: float = _DEFAULT_SCALE,
    out_path: str | Path | None = None,
    *,
    max_cache_bytes: int | None = None,
) -> dict[str, Any]:
    if out_path is None:
        raise ValueError("out_path required")
    return _render_to_path(
        Path(path),
        int(page),
        list(rect),
        float(scale),
        Path(out_path),
        max_cache_bytes=max_cache_bytes,
    )


def render_page(
    path: str | Path,
    page: int,
    scale: float = _DEFAULT_SCALE,
    out_path: str | Path | None = None,
    *,
    max_cache_bytes: int | None = None,
) -> dict[str, Any]:
    if out_path is None:
        raise ValueError("out_path required")
    return _render_to_path(
        Path(path),
        int(page),
        None,
        float(scale),
        Path(out_path),
        max_cache_bytes=max_cache_bytes,
    )


def _out_path_from_params(params: dict[str, Any]) -> Path:
    raw = params.get("outPath") or params.get("out_path")
    if not raw:
        raise ValueError("outPath required")
    return Path(str(raw))


def rpc_render_region(params: dict[str, Any]) -> dict[str, Any]:
    rect = params.get("rect")
    if not isinstance(rect, list) or len(rect) < 4:
        raise ValueError("rect required")
    scale = float(params.get("scale") or _DEFAULT_SCALE)
    max_bytes = params.get("max_cache_bytes")
    max_cache = int(max_bytes) if max_bytes is not None else None
    return render_region(
        str(params["path"]),
        int(params["page"]),
        [float(x) for x in rect[:4]],
        scale,
        _out_path_from_params(params),
        max_cache_bytes=max_cache,
    )


def rpc_render_page(params: dict[str, Any]) -> dict[str, Any]:
    scale = float(params.get("scale") or _DEFAULT_SCALE)
    max_bytes = params.get("max_cache_bytes")
    max_cache = int(max_bytes) if max_bytes is not None else None
    return render_page(
        str(params["path"]),
        int(params["page"]),
        scale,
        _out_path_from_params(params),
        max_cache_bytes=max_cache,
    )

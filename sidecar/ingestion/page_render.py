"""Render PDF page / region clips to PNG with a disk cache."""

from __future__ import annotations

import base64
import hashlib
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


def _file_mtime(path: Path) -> float:
    try:
        return path.stat().st_mtime
    except OSError:
        return 0.0


def _cache_key(path: Path, page: int, rect: list[float] | None, scale: float) -> str:
    mtime = _file_mtime(path)
    rect_part = ""
    if rect is not None and len(rect) >= 4:
        rect_part = f"{rect[0]:.4f},{rect[1]:.4f},{rect[2]:.4f},{rect[3]:.4f}"
    raw = f"{path.resolve()}|{mtime}|{page}|{rect_part}|{scale:.4f}"
    return hashlib.sha256(raw.encode("utf-8")).hexdigest()


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


def _render(
    path: Path,
    page: int,
    rect: list[float] | None,
    scale: float,
    cache_dir: Path,
    *,
    max_cache_bytes: int | None = None,
) -> dict[str, Any]:
    cache_dir.mkdir(parents=True, exist_ok=True)
    key = _cache_key(path, page, rect, scale)
    cached = cache_dir / f"{key}.png"
    if cached.is_file():
        _touch(cached)
        data = cached.read_bytes()
        # Recover dimensions from pixmap header via fitz if needed; use image size.
        doc = fitz.open(stream=data, filetype="png")
        try:
            w = int(doc[0].rect.width)
            h = int(doc[0].rect.height)
        finally:
            doc.close()
        return {
            "pngBase64": base64.b64encode(data).decode("ascii"),
            "width": w,
            "height": h,
            "cacheKey": key,
        }

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

    cached.write_bytes(png)
    _evict_lru(cache_dir, max_cache_bytes)
    return {
        "pngBase64": base64.b64encode(png).decode("ascii"),
        "width": width,
        "height": height,
        "cacheKey": key,
    }


def render_region(
    path: str | Path,
    page: int,
    rect: list[float],
    scale: float = _DEFAULT_SCALE,
    cache_dir: str | Path | None = None,
    *,
    max_cache_bytes: int | None = None,
) -> dict[str, Any]:
    p = Path(path)
    if cache_dir is None:
        raise ValueError("cache_dir required")
    return _render(p, int(page), list(rect), float(scale), Path(cache_dir), max_cache_bytes=max_cache_bytes)


def render_page(
    path: str | Path,
    page: int,
    scale: float = _DEFAULT_SCALE,
    cache_dir: str | Path | None = None,
    *,
    max_cache_bytes: int | None = None,
) -> dict[str, Any]:
    p = Path(path)
    if cache_dir is None:
        raise ValueError("cache_dir required")
    return _render(p, int(page), None, float(scale), Path(cache_dir), max_cache_bytes=max_cache_bytes)


def rpc_render_region(params: dict[str, Any]) -> dict[str, Any]:
    app_data = params.get("app_data") or params.get("appData")
    if not app_data:
        raise ValueError("app_data required")
    cache_dir = Path(str(app_data)) / "cache" / "pages"
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
        cache_dir,
        max_cache_bytes=max_cache,
    )


def rpc_render_page(params: dict[str, Any]) -> dict[str, Any]:
    app_data = params.get("app_data") or params.get("appData")
    if not app_data:
        raise ValueError("app_data required")
    cache_dir = Path(str(app_data)) / "cache" / "pages"
    scale = float(params.get("scale") or _DEFAULT_SCALE)
    max_bytes = params.get("max_cache_bytes")
    max_cache = int(max_bytes) if max_bytes is not None else None
    return render_page(
        str(params["path"]),
        int(params["page"]),
        scale,
        cache_dir,
        max_cache_bytes=max_cache,
    )

"""Busy timeout lets a second writer succeed while another holds a lock briefly."""

from __future__ import annotations

import sqlite3
import threading
import time
from pathlib import Path

from common.sqlite_vec_util import SQLITE_BUSY_TIMEOUT_S, connect_sqlite


def test_busy_timeout_waits_for_short_writer(tmp_path: Path) -> None:
    assert SQLITE_BUSY_TIMEOUT_S >= 15.0
    db = tmp_path / "busy.sqlite"
    setup = connect_sqlite(str(db))
    setup.execute("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
    setup.commit()
    setup.close()

    holder_ready = threading.Event()
    holder_done = threading.Event()
    second_ok = threading.Event()
    second_err: list[BaseException] = []

    def hold_write() -> None:
        con = connect_sqlite(str(db))
        try:
            con.execute("BEGIN IMMEDIATE")
            con.execute("INSERT INTO t(v) VALUES ('hold')")
            holder_ready.set()
            time.sleep(2.0)
            con.commit()
        finally:
            con.close()
            holder_done.set()

    def second_write() -> None:
        holder_ready.wait(timeout=5)
        try:
            con = connect_sqlite(str(db))
            try:
                con.execute("INSERT INTO t(v) VALUES ('second')")
                con.commit()
                second_ok.set()
            finally:
                con.close()
        except BaseException as exc:  # noqa: BLE001
            second_err.append(exc)

    t1 = threading.Thread(target=hold_write)
    t2 = threading.Thread(target=second_write)
    t1.start()
    t2.start()
    t1.join(timeout=10)
    t2.join(timeout=10)
    assert holder_done.is_set()
    assert not second_err, second_err
    assert second_ok.is_set()
    rows = sqlite3.connect(str(db)).execute("SELECT v FROM t ORDER BY id").fetchall()
    assert [r[0] for r in rows] == ["hold", "second"]

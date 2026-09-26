from __future__ import annotations

import json
import os
import shlex
import sqlite3
import subprocess
from pathlib import Path
from typing import Any

from fastapi import FastAPI, Header, HTTPException
from pydantic import BaseModel, Field

DB_PATH = Path(os.getenv("NEXUS_BRIDGE_DB", "var/nexus_bridge.db"))
DB_PATH.parent.mkdir(parents=True, exist_ok=True)

app = FastAPI(title="SYNERGY NEXUS compatibility bridge", version="0.1.0")


def guard(auth: str | None) -> None:
    token = os.getenv("NEXUS_BRIDGE_TOKEN", "")
    if token and auth != f"Bearer {token}":
        raise HTTPException(401, "invalid token")


def db() -> sqlite3.Connection:
    conn = sqlite3.connect(DB_PATH)
    conn.row_factory = sqlite3.Row
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS memory (
            namespace TEXT NOT NULL,
            key TEXT NOT NULL,
            content TEXT NOT NULL,
            metadata_json TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY(namespace, key)
        )
        """
    )
    return conn


class SearchIn(BaseModel):
    query: str
    namespaces: list[str] = Field(default_factory=list)
    limit: int = 10


class UpsertIn(BaseModel):
    key: str
    content: str
    namespace: str
    metadata: dict[str, Any] = Field(default_factory=dict)
    source: str = "synergy-ai-autopilot"


class InferIn(BaseModel):
    prompt: str
    context: dict[str, Any] = Field(default_factory=dict)


@app.get("/health")
def health():
    return {
        "ok": True,
        "service": "synergy-nexus-bridge",
        "memory": "sqlite-compatibility-layer",
        "inference_configured": bool(os.getenv("NEXUS_INFERENCE_CMD", "")),
        "note": "The bridge is a compatibility surface, not the NEXUS model itself.",
    }


@app.post("/v1/memory/search")
def search(body: SearchIn, authorization: str | None = Header(default=None)):
    guard(authorization)
    q = f"%{body.query}%"
    with db() as conn:
        if body.namespaces:
            placeholders = ",".join("?" for _ in body.namespaces)
            rows = conn.execute(
                f"SELECT namespace,key,content,metadata_json,updated_at FROM memory WHERE namespace IN ({placeholders}) AND (content LIKE ? OR key LIKE ?) ORDER BY updated_at DESC LIMIT ?",
                [*body.namespaces, q, q, max(1, min(body.limit, 100))],
            ).fetchall()
        else:
            rows = conn.execute(
                "SELECT namespace,key,content,metadata_json,updated_at FROM memory WHERE content LIKE ? OR key LIKE ? ORDER BY updated_at DESC LIMIT ?",
                (q, q, max(1, min(body.limit, 100))),
            ).fetchall()
    return {
        "items": [
            {
                "namespace": row["namespace"],
                "key": row["key"],
                "content": row["content"],
                "metadata": json.loads(row["metadata_json"]),
                "updated_at": row["updated_at"],
            }
            for row in rows
        ]
    }


@app.post("/v1/memory/upsert")
def upsert(body: UpsertIn, authorization: str | None = Header(default=None)):
    guard(authorization)
    metadata = dict(body.metadata)
    metadata.setdefault("source", body.source)
    with db() as conn:
        conn.execute(
            """
            INSERT INTO memory(namespace,key,content,metadata_json)
            VALUES(?,?,?,?)
            ON CONFLICT(namespace,key) DO UPDATE SET
              content=excluded.content,
              metadata_json=excluded.metadata_json,
              updated_at=CURRENT_TIMESTAMP
            """,
            (body.namespace, body.key, body.content, json.dumps(metadata, ensure_ascii=False)),
        )
    return {"ok": True, "namespace": body.namespace, "key": body.key}


@app.post("/v1/infer")
def infer(body: InferIn, authorization: str | None = Header(default=None)):
    guard(authorization)
    raw = os.getenv("NEXUS_INFERENCE_CMD", "").strip()
    if not raw:
        raise HTTPException(503, "NEXUS_INFERENCE_CMD is not configured")
    argv = shlex.split(raw)
    payload = json.dumps({"prompt": body.prompt, "context": body.context}, ensure_ascii=False)
    try:
        completed = subprocess.run(
            argv,
            input=payload.encode("utf-8"),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            shell=False,
            check=False,
            timeout=float(os.getenv("NEXUS_INFERENCE_TIMEOUT", "120")),
        )
    except Exception as exc:
        raise HTTPException(502, f"inference bridge failed: {type(exc).__name__}: {exc}") from exc
    if completed.returncode != 0:
        raise HTTPException(502, completed.stderr.decode("utf-8", errors="replace")[:4000])
    text = completed.stdout.decode("utf-8", errors="replace")
    try:
        value = json.loads(text)
    except json.JSONDecodeError:
        value = {"text": text}
    return {"ok": True, "result": value, "bridge_only": True}


def main() -> None:
    import uvicorn
    uvicorn.run(app, host="127.0.0.1", port=int(os.getenv("NEXUS_BRIDGE_PORT", "8130")))


if __name__ == "__main__":
    main()

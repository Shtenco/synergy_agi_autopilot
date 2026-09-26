from __future__ import annotations

import json
import os
import sqlite3
import urllib.error
import urllib.parse
import urllib.request
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from fastapi import FastAPI, Header, HTTPException
from pydantic import BaseModel, Field

app = FastAPI(title="SYNERGY Connector Gateway", version="0.1.0")

DB_PATH = Path(os.getenv("CONNECTOR_GATEWAY_DB", "var/connector_gateway.db"))
DB_PATH.parent.mkdir(parents=True, exist_ok=True)
DB_ROOT = Path(os.getenv("CONNECTOR_DB_ROOT", ".")).resolve()


def guard(auth: str | None) -> None:
    token = os.getenv("CONNECTOR_GATEWAY_TOKEN", "")
    if token and auth != f"Bearer {token}":
        raise HTTPException(401, "invalid token")


def allowed_hosts() -> set[str]:
    return {item.strip().lower() for item in os.getenv("CONNECTOR_ALLOWED_HOSTS", "").split(",") if item.strip()}


def validate_url(raw: str) -> str:
    parsed = urllib.parse.urlparse(raw)
    if parsed.scheme not in {"http", "https"} or not parsed.hostname:
        raise HTTPException(422, "only http/https URLs are allowed")
    host = parsed.hostname.lower()
    loopback = host in {"127.0.0.1", "localhost", "::1"}
    allowed = allowed_hosts()
    if not loopback and host not in allowed:
        raise HTTPException(403, f"host is not allowlisted: {host}")
    return raw


def resolve_secret(name: str | None) -> str | None:
    if not name:
        return None
    if not name or any(ch not in "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_" for ch in name):
        raise HTTPException(422, "invalid secret reference")
    allow = {item.strip() for item in os.getenv("CONNECTOR_SECRET_ALLOWLIST", "").split(",") if item.strip()}
    if name not in allow:
        raise HTTPException(403, f"secret is not allowlisted: {name}")
    value = os.getenv(name)
    if value is None:
        raise HTTPException(503, f"secret is unavailable: {name}")
    return value


def request_json(url: str, method: str, headers: dict[str, str], body: Any = None) -> Any:
    validate_url(url)
    payload = None if body is None else json.dumps(body, ensure_ascii=False).encode("utf-8")
    req_headers = {"Accept": "application/json", **headers}
    if payload is not None:
        req_headers.setdefault("Content-Type", "application/json")
    request = urllib.request.Request(url, data=payload, method=method.upper(), headers=req_headers)
    try:
        with urllib.request.urlopen(request, timeout=float(os.getenv("CONNECTOR_HTTP_TIMEOUT", "30"))) as response:
            raw = response.read()
            content_type = response.headers.get("Content-Type", "")
            if not raw:
                return {"status": response.status}
            if "json" in content_type.lower():
                return json.loads(raw.decode("utf-8"))
            return {"status": response.status, "text": raw.decode("utf-8", errors="replace")[:200000]}
    except urllib.error.HTTPError as exc:
        raw = exc.read().decode("utf-8", errors="replace")[:20000]
        raise HTTPException(502, f"upstream status={exc.code}: {raw}") from exc
    except Exception as exc:
        raise HTTPException(502, f"connector request failed: {type(exc).__name__}: {exc}") from exc


def oauth_token(config: dict[str, Any]) -> str:
    token_url = validate_url(str(config["token_url"]))
    client_id = resolve_secret(str(config["client_id_secret"]))
    client_secret = resolve_secret(str(config["client_secret_secret"]))
    form = urllib.parse.urlencode({
        "grant_type": "client_credentials",
        "client_id": client_id,
        "client_secret": client_secret,
        "scope": " ".join(config.get("scope") or []),
    }).encode("utf-8")
    request = urllib.request.Request(
        token_url, data=form, method="POST",
        headers={"Content-Type": "application/x-www-form-urlencoded", "Accept": "application/json"},
    )
    try:
        with urllib.request.urlopen(request, timeout=float(os.getenv("CONNECTOR_HTTP_TIMEOUT", "30"))) as response:
            value = json.load(response)
    except Exception as exc:
        raise HTTPException(502, f"OAuth token exchange failed: {type(exc).__name__}: {exc}") from exc
    token = value.get("access_token")
    if not token:
        raise HTTPException(502, "OAuth server returned no access_token")
    return str(token)


def persist_autopilot_event(topic: str, payload: dict[str, Any]) -> str | None:
    raw = os.getenv("AUTOPILOT_CONTROL_DB", "").strip()
    if not raw:
        return None
    event_id = str(uuid.uuid4())
    occurred_at = datetime.now(timezone.utc).isoformat()
    try:
        conn = sqlite3.connect(raw, timeout=10)
        try:
            conn.execute(
                """INSERT INTO automation_events(
                    id,topic,payload_json,occurred_at,status,attempts,next_attempt_at
                ) VALUES(?,?,?,?, 'pending',0,?)""",
                (event_id, topic, json.dumps(payload, ensure_ascii=False), occurred_at, occurred_at),
            )
            conn.commit()
        finally:
            conn.close()
    except Exception as exc:
        raise HTTPException(503, f"failed to persist Autopilot event: {type(exc).__name__}: {exc}") from exc
    return event_id


def queue_db() -> sqlite3.Connection:
    conn = sqlite3.connect(DB_PATH)
    conn.row_factory = sqlite3.Row
    conn.execute("""
        CREATE TABLE IF NOT EXISTS queue (
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            topic TEXT NOT NULL,
            body_json TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            consumed_at TEXT
        )
    """)
    conn.execute("""
        CREATE TABLE IF NOT EXISTS webhooks (
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            topic TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
    """)
    return conn


class HttpIn(BaseModel):
    url: str
    method: str = "GET"
    headers: dict[str, str] = Field(default_factory=dict)
    body: Any = None
    bearer_secret: str | None = None


class OpenApiIn(BaseModel):
    base_url: str
    operation_id: str
    document: dict[str, Any]
    path_params: dict[str, str] = Field(default_factory=dict)
    query_params: dict[str, Any] = Field(default_factory=dict)
    header_params: dict[str, str] = Field(default_factory=dict)
    body: Any = None
    oauth2: dict[str, Any] | None = None


class DbIn(BaseModel):
    path: str
    sql: str
    params: list[Any] = Field(default_factory=list)


class QueuePublishIn(BaseModel):
    topic: str
    body: Any


class QueueConsumeIn(BaseModel):
    topic: str


@app.get("/health")
def health():
    return {
        "ok": True,
        "service": "synergy-connector-gateway",
        "allowed_hosts": sorted(allowed_hosts()),
        "db_root": str(DB_ROOT),
    }


@app.post("/v1/http")
def http_call(body: HttpIn, authorization: str | None = Header(default=None)):
    guard(authorization)
    headers = dict(body.headers)
    if body.bearer_secret:
        headers["Authorization"] = f"Bearer {resolve_secret(body.bearer_secret)}"
    return request_json(body.url, body.method, headers, body.body)


@app.post("/v1/openapi")
def openapi_call(body: OpenApiIn, authorization: str | None = Header(default=None)):
    guard(authorization)
    found: tuple[str, str] | None = None
    for path, item in (body.document.get("paths") or {}).items():
        if not isinstance(item, dict):
            continue
        for method, operation in item.items():
            if isinstance(operation, dict) and operation.get("operationId") == body.operation_id:
                found = (method.upper(), path)
                break
        if found:
            break
    if not found:
        raise HTTPException(404, f"operationId not found: {body.operation_id}")
    method, path = found
    for key, value in body.path_params.items():
        path = path.replace("{" + key + "}", urllib.parse.quote(str(value), safe=""))
    if "{" in path:
        raise HTTPException(422, f"unresolved path params: {path}")
    url = body.base_url.rstrip("/") + path
    if body.query_params:
        query = urllib.parse.urlencode(body.query_params, doseq=True)
        url = url + ("&" if "?" in url else "?") + query

    forbidden_headers = {"host", "content-length", "authorization", "connection"}
    headers: dict[str, str] = {}
    for key, value in body.header_params.items():
        if key.lower() in forbidden_headers:
            raise HTTPException(403, f"header cannot be supplied by generated adapter: {key}")
        headers[key] = str(value)
    if body.oauth2:
        headers["Authorization"] = f"Bearer {oauth_token(body.oauth2)}"
    return request_json(url, method, headers, body.body)


@app.post("/v1/db/query")
def db_query(body: DbIn, authorization: str | None = Header(default=None)):
    guard(authorization)
    target = Path(body.path).resolve()
    if DB_ROOT not in target.parents and target != DB_ROOT:
        raise HTTPException(403, "database path escapes CONNECTOR_DB_ROOT")
    sql = body.sql.strip()
    lowered = sql.lower()
    if not (lowered.startswith("select ") or lowered.startswith("with ")):
        raise HTTPException(403, "only SELECT/CTE statements are allowed")
    if ";" in sql:
        raise HTTPException(403, "multiple statements are not allowed")
    conn = sqlite3.connect(f"file:{target}?mode=ro", uri=True)
    conn.row_factory = sqlite3.Row
    try:
        rows = conn.execute(sql, body.params).fetchmany(int(os.getenv("CONNECTOR_DB_MAX_ROWS", "1000")))
        return {"rows": [dict(row) for row in rows]}
    finally:
        conn.close()


@app.post("/v1/queue/publish")
def queue_publish(body: QueuePublishIn, authorization: str | None = Header(default=None)):
    guard(authorization)
    with queue_db() as conn:
        cur = conn.execute("INSERT INTO queue(topic,body_json) VALUES(?,?)", (body.topic, json.dumps(body.body, ensure_ascii=False)))
        return {"ok": True, "id": cur.lastrowid}


@app.post("/v1/queue/consume")
def queue_consume(body: QueueConsumeIn, authorization: str | None = Header(default=None)):
    guard(authorization)
    with queue_db() as conn:
        row = conn.execute(
            "SELECT seq,body_json,created_at FROM queue WHERE topic=? AND consumed_at IS NULL ORDER BY seq LIMIT 1",
            (body.topic,),
        ).fetchone()
        if not row:
            return {"message": None}
        conn.execute("UPDATE queue SET consumed_at=CURRENT_TIMESTAMP WHERE seq=?", (row["seq"],))
        return {"message": {"id": row["seq"], "body": json.loads(row["body_json"]), "created_at": row["created_at"]}}


@app.post("/v1/webhooks/{topic:path}")
def webhook(topic: str, payload: dict[str, Any], authorization: str | None = Header(default=None)):
    guard(authorization)
    event_topic = f"webhook.{topic}"
    with queue_db() as conn:
        cur = conn.execute("INSERT INTO webhooks(topic,payload_json) VALUES(?,?)", (topic, json.dumps(payload, ensure_ascii=False)))
        conn.execute("INSERT INTO queue(topic,body_json) VALUES(?,?)", (event_topic, json.dumps(payload, ensure_ascii=False)))
    autopilot_event_id = persist_autopilot_event(event_topic, payload)
    return {"accepted": True, "id": cur.lastrowid, "topic": topic, "autopilot_event_id": autopilot_event_id}


def main() -> None:
    import uvicorn
    uvicorn.run(app, host="127.0.0.1", port=int(os.getenv("CONNECTOR_GATEWAY_PORT", "8150")))


if __name__ == "__main__":
    main()

from __future__ import annotations
import os
from .http import request_json


class GenericVideoProvider:
    """Provider-agnostic paid render boundary.

    No vendor-specific API is invented here. A reviewed provider or internal sidecar
    must expose a JSON HTTP endpoint and is configured explicitly through env vars.
    """

    def status(self) -> dict:
        return {
            "configured": bool(os.getenv("AUTOPILOT_VIDEO_PROVIDER_URL")),
            "provider_name": os.getenv("AUTOPILOT_VIDEO_PROVIDER_NAME", "generic-http"),
            "cost_bearing": True,
            "execution_requires_approval": True,
        }

    def render(self, payload: dict) -> dict:
        url = os.getenv("AUTOPILOT_VIDEO_PROVIDER_URL", "").strip()
        if not url:
            raise RuntimeError("video render provider is not configured")
        token = os.getenv("AUTOPILOT_VIDEO_PROVIDER_TOKEN", "").strip()
        headers = {"Authorization": f"Bearer {token}"} if token else {}
        body = {
            "schema": "synergy.ai-video-render-request/v1",
            "provider": os.getenv("AUTOPILOT_VIDEO_PROVIDER_NAME", "generic-http"),
            "storyboard": payload.get("storyboard"),
            "prompt": payload.get("prompt", ""),
            "duration_sec": payload.get("duration_sec"),
            "aspect_ratio": payload.get("aspect_ratio", "9:16"),
            "callback_url": payload.get("callback_url"),
            "metadata": payload.get("metadata", {}),
        }
        result = request_json("POST", url, headers=headers, json_body=body)
        return {
            "schema": "synergy.ai-video-render-dispatch/v1",
            "provider": body["provider"],
            "provider_result": result,
            "financial_authority": "NONE",
        }

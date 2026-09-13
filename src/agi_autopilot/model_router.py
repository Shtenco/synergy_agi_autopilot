from __future__ import annotations
import json, os, urllib.request


LEGACY_PROVIDERS = {
    "local": ("", ""), "groq": ("GROQ_API_KEY", ""), "opencode-zen": ("OPENCODE_API_KEY", ""),
    "big-pickle": ("OPENCODE_API_KEY", ""), "deepseek": ("DEEPSEEK_API_KEY", ""),
    "openrouter": ("OPENROUTER_API_KEY", ""), "deepinfra": ("DEEPINFRA_API_KEY", ""),
    "openai": ("OPENAI_API_KEY", ""), "claude": ("ANTHROPIC_API_KEY", ""),
    "gemini": ("GEMINI_API_KEY", ""), "mistral": ("MISTRAL_API_KEY", ""), "grok": ("XAI_API_KEY", ""),
    "nvidia_nim": ("NVIDIA_NIM_API_KEY", ""), "together-ai": ("TOGETHER_API_KEY", ""),
    "cloudflare": ("CLOUDFLARE_API_TOKEN", ""), "yandex": ("YANDEX_API_KEY", ""), "gigachat": ("GIGACHAT_API_KEY", ""),
}


class TextModelRouter:
    """Portable OpenAI-compatible adapter. Historical provider names remain inventory metadata.
    No provider endpoint is guessed: base URL/model are explicit environment configuration.
    """
    def __init__(self):
        self.base_url = os.getenv("AUTOPILOT_LLM_BASE_URL", "").rstrip("/")
        self.api_key = os.getenv("AUTOPILOT_LLM_API_KEY", "")
        self.model = os.getenv("AUTOPILOT_LLM_MODEL", "")

    def status(self) -> dict:
        return {"configured": bool(self.base_url and self.model), "model": self.model or None, "legacy_provider_inventory": sorted(LEGACY_PROVIDERS)}

    def generate(self, system: str, user: str, max_tokens: int = 700) -> str | None:
        if not (self.base_url and self.model): return None
        data = json.dumps({"model": self.model, "messages": [{"role":"system","content":system},{"role":"user","content":user}], "max_tokens": max_tokens}).encode()
        headers = {"Content-Type":"application/json"}
        if self.api_key: headers["Authorization"] = f"Bearer {self.api_key}"
        req = urllib.request.Request(self.base_url + "/chat/completions", data=data, headers=headers, method="POST")
        with urllib.request.urlopen(req, timeout=45) as r:
            payload = json.loads(r.read().decode())
        return payload["choices"][0]["message"]["content"]

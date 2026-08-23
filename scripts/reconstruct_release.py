from pathlib import Path
import base64,hashlib,json
ROOT=Path(__file__).resolve().parents[1]
D=ROOT/"artifacts/release_carrier"
M=json.loads((D/"carrier_manifest_v1.json").read_text())
out=ROOT/M["archive_name"]
h=hashlib.sha256(); size=0
with out.open("wb") as w:
    for item in M["parts"]:
        b=base64.b64decode((ROOT/item["path"]).read_text().strip(),validate=True)
        assert len(b)==item["decoded_size"]
        assert hashlib.sha256(b).hexdigest()==item["decoded_sha256"]
        w.write(b); h.update(b); size+=len(b)
assert size==M["archive_size"] and h.hexdigest()==M["archive_sha256"]
print(out, size, h.hexdigest())

#!/usr/bin/env python3
"""Capture LIVE sample outputs for the site by reusing the live-run part of
docs/build_guide.py. Writes site/samples.json. Usage: python3 site/scripts/capture_samples.py"""
import datetime, json, os, sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
src = open(os.path.join(ROOT, "docs/build_guide.py")).read()
live = src.split("# --------------------------------------------------------------------------- html")[0]
sys.argv = [sys.argv[0]]
ns = {"__file__": os.path.join(ROOT, "docs/build_guide.py")}
exec(compile(live, "build_guide_live", "exec"), ns)
now = datetime.datetime.now(datetime.timezone(datetime.timedelta(hours=5, minutes=30)))
out = {
    "captured_at": now.strftime("%d %b %Y, %H:%M IST"),
    "server_info": ns["init"]["serverInfo"],
    "protocol_version": ns["init"]["protocolVersion"],
    "tools": [{"name": t["name"], "description": t["description"], "args": list(t["inputSchema"].get("properties", {}))} for t in ns["tools"]],
    "samples": [{"tool": n, "args": a, "output": o} for (n, a, o) in ns["samples"]],
    "errors": [ns["err"], ns["err2"]],
}
json.dump(out, open(os.path.join(ROOT, "site/samples.json"), "w"), indent=1, ensure_ascii=False)
print("captured", len(out["samples"]), "samples at", out["captured_at"])

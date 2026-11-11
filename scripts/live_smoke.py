#!/usr/bin/env python3
"""Drive the jlcpcb-mcp binary over stdio with raw JSON-RPC and print results.

Usage: python3 scripts/live_smoke.py [path/to/jlcpcb-mcp] [--full]
Hits the live JLCPCB / LCSC endpoints (network required).
"""
import json
import subprocess
import sys

binary = next((a for a in sys.argv[1:] if not a.startswith("--")), "target/release/jlcpcb-mcp")
full = "--full" in sys.argv

proc = subprocess.Popen([binary], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
_next_id = 0


def send(msg):
    proc.stdin.write(json.dumps(msg) + "\n")
    proc.stdin.flush()


def request(method, params=None):
    global _next_id
    _next_id += 1
    send({"jsonrpc": "2.0", "id": _next_id, "method": method, "params": params or {}})
    while True:
        line = proc.stdout.readline()
        if not line:
            raise SystemExit("server closed stdout")
        msg = json.loads(line)
        if msg.get("id") == _next_id:
            return msg


def call(name, args):
    resp = request("tools/call", {"name": name, "arguments": args})
    if "error" in resp:
        return {"jsonrpc_error": resp["error"]}
    result = resp["result"]
    payload = result.get("structuredContent")
    if payload is None:
        payload = {"text": [c.get("text") for c in result.get("content", [])]}
    return {"isError": result.get("isError", False), "data": payload}


init = request("initialize", {
    "protocolVersion": "2025-06-18",
    "capabilities": {},
    "clientInfo": {"name": "live-smoke", "version": "0"},
})
print("initialize ->", json.dumps(init["result"]["serverInfo"]), init["result"]["protocolVersion"])
send({"jsonrpc": "2.0", "method": "notifications/initialized"})

tools = request("tools/list")["result"]["tools"]
print("tools/list ->", [t["name"] for t in tools])


def show(title, out, keep=None):
    print(f"\n=== {title}")
    data = out.get("data")
    if keep and isinstance(data, dict) and not out.get("isError"):
        data = keep(data)
    print(json.dumps({"isError": out.get("isError"), "data": data}, indent=1, ensure_ascii=False)[: 6000 if full else 2500])


def brief(c):
    return {k: c.get(k) for k in ("lcsc", "mfr_part", "manufacturer", "package", "library_type", "jlc_stock", "unit_price_usd", "price_qty")}


show("search_components 10k 0402 resistor", call("search_components", {"query": "10k", "category": "Resistors", "package": "0402", "max_results": 5}),
     lambda d: {**{k: d[k] for k in ("filters", "total_matches", "returned")}, "results": [brief(c) for c in d["results"]]})
show("search_components basic_only 100nF 0402", call("search_components", {"query": "100nF", "package": "0402", "basic_only": True, "max_results": 5}),
     lambda d: {**{k: d[k] for k in ("filters", "total_matches", "returned")}, "results": [brief(c) for c in d["results"]]})
show("get_component_details C25744", call("get_component_details", {"lcsc": "C25744"}),
     lambda d: {k: d[k] for k in ("lcsc", "mfr_part", "manufacturer", "package", "category", "subcategory", "library_type", "jlc_assembly_stock", "lcsc_retail_stock", "jlc_unit_price_usd", "jlc_price_breaks", "lcsc_price_breaks", "datasheet_url", "image_url", "assembly_process", "sources", "warnings")})
show("suggest_alternatives C25531", call("suggest_alternatives", {"lcsc": "C25531", "max_results": 4}),
     lambda d: {"original": brief(d["original"]), "match_criteria": d["match_criteria"], "alternatives": [{**brief(a["component"]), "reasons": a["reasons"]} for a in d["alternatives"]], "notes": d["notes"]})
show("suggest_alternatives C6186 (LDO)", call("suggest_alternatives", {"lcsc": "C6186", "max_results": 3}),
     lambda d: {"original": brief(d["original"]), "match_criteria": d["match_criteria"], "alternatives": [{**brief(a["component"]), "reasons": a["reasons"]} for a in d["alternatives"]]})
show("list_categories (top level)", call("list_categories", {}),
     lambda d: {"total_categories": d["total_categories"], "first": [(c["name"], c["part_count"]) for c in d["categories"][:6]]})
show("list_categories search=mosfet", call("list_categories", {"search": "mosfet"}))
show("estimate_pcb_quote 100x80 2L x10", call("estimate_pcb_quote", {"width_mm": 100, "height_mm": 80, "quantity": 10, "layers": 2}))
show("service_status", call("service_status", {}))
show("error: bad part number", call("get_component_details", {"lcsc": "not-a-part"}))
show("error: unknown part", call("get_component_details", {"lcsc": "C999999999"}))
show("error: ambiguous category", call("search_components", {"query": "100nF", "category": "mlcc"}))
show("search basic+preferred, category by partial name", call("search_components", {"query": "AMS1117", "category": "LDO", "basic_only": True, "include_preferred": True, "max_results": 3}),
     lambda d: {**{k: d[k] for k in ("filters", "total_matches", "returned")}, "results": [brief(c) for c in d["results"]]})

proc.stdin.close()
proc.wait(timeout=10)
print("\nexit code", proc.returncode)

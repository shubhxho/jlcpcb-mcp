#!/usr/bin/env python3
"""Build docs/jlcpcb-mcp-guide.pdf.

Runs the release binary over stdio against the LIVE JLCPCB/LCSC endpoints,
embeds the real (trimmed) outputs in an HTML guide, and prints it to PDF with
headless Chrome/Chromium.

Usage: python3 docs/build_guide.py [path/to/jlcpcb-mcp]
"""
import datetime
import html
import json
import os
import shutil
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "target/release/jlcpcb-mcp")
OUT = os.path.join(ROOT, "docs/jlcpcb-mcp-guide.pdf")


# --------------------------------------------------------------------------- live run
class Client:
    def __init__(self, binary):
        self.p = subprocess.Popen([binary], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        self.i = 0

    def req(self, method, params=None):
        self.i += 1
        self.p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": self.i, "method": method, "params": params or {}}) + "\n")
        self.p.stdin.flush()
        while True:
            m = json.loads(self.p.stdout.readline())
            if m.get("id") == self.i:
                return m

    def note(self, method):
        self.p.stdin.write(json.dumps({"jsonrpc": "2.0", "method": method}) + "\n")
        self.p.stdin.flush()

    def call(self, name, args):
        r = self.req("tools/call", {"name": name, "arguments": args})["result"]
        if r.get("isError"):
            return {"isError": True, "message": r["content"][0]["text"]}
        return r["structuredContent"]

    def close(self):
        self.p.stdin.close()
        self.p.wait(timeout=10)


c = Client(BIN)
init = c.req("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "guide-builder", "version": "0"}})["result"]
c.note("notifications/initialized")
tools = c.req("tools/list")["result"]["tools"]


def brief(x):
    return {k: x.get(k) for k in ("lcsc", "mfr_part", "manufacturer", "package", "library_type", "jlc_stock", "unit_price_usd", "price_qty")}


samples = []

a = {"query": "10k", "category": "Resistors", "package": "0402", "max_results": 4}
r = c.call("search_components", a)
samples.append(("search_components", a, {"filters": r["filters"], "total_matches": r["total_matches"], "returned": r["returned"], "results": [brief(x) for x in r["results"]]}))

a = {"query": "100nF", "package": "0402", "basic_only": True, "max_results": 3}
r = c.call("search_components", a)
samples.append(("search_components", a, {"total_matches": r["total_matches"], "results": [brief(x) for x in r["results"]]}))

a = {"lcsc": "C25744"}
r = c.call("get_component_details", a)
keep = ("lcsc", "mfr_part", "manufacturer", "package", "category", "subcategory", "library_type", "jlc_assembly_stock",
        "lcsc_retail_stock", "price_qty", "jlc_unit_price_usd", "assembly_process", "moisture_sensitivity_level", "datasheet_url", "sources", "warnings")
d = {k: r.get(k) for k in keep}
d["jlc_price_breaks"] = r["jlc_price_breaks"][:3] + (["…"] if len(r["jlc_price_breaks"]) > 3 else [])
d["attributes"] = r["attributes"][:4] + (["…"] if len(r["attributes"]) > 4 else [])
samples.append(("get_component_details", a, d))

a = {"lcsc": "C25531", "max_results": 3}
r = c.call("suggest_alternatives", a)
samples.append(("suggest_alternatives", a, {"original": brief(r["original"]), "match_criteria": r["match_criteria"],
                                             "alternatives": [{**brief(x["component"]), "reasons": x["reasons"]} for x in r["alternatives"]]}))

a = {"search": "mosfet"}
r = c.call("list_categories", a)
samples.append(("list_categories", a, r))

a = {"width_mm": 100, "height_mm": 80, "quantity": 10, "layers": 2}
r = c.call("estimate_pcb_quote", a)
samples.append(("estimate_pcb_quote", a, {k: r[k] for k in ("is_official_quote", "method", "low_usd", "typical_usd", "high_usd", "per_board_typical_usd", "breakdown", "official_quote_url")}))

r = c.call("service_status", {})
samples.append(("service_status", {}, {k: r[k] for k in ("version", "jlcpcb_parts_api", "lcsc_api", "official_api_configured")}))

err = c.call("get_component_details", {"lcsc": "C999999999"})
err2 = c.call("search_components", {"query": "100nF", "category": "mlcc"})
c.close()

now = datetime.datetime.now(datetime.timezone(datetime.timedelta(hours=5, minutes=30)))
stamp = now.strftime("%d %b %Y, %H:%M IST")


# --------------------------------------------------------------------------- html
def compact(obj, depth=0, width=100):
    """Pretty JSON that keeps short objects/arrays on one line."""
    one = json.dumps(obj, ensure_ascii=False)
    if len(one) + 2 * depth <= width or not isinstance(obj, (dict, list)) or not obj:
        return one
    pad, end = "  " * (depth + 1), "  " * depth
    if isinstance(obj, dict):
        items = [f"{pad}{json.dumps(k, ensure_ascii=False)}: {compact(v, depth + 1, width)}" for k, v in obj.items()]
        return "{\n" + ",\n".join(items) + "\n" + end + "}"
    items = [pad + compact(v, depth + 1, width) for v in obj]
    return "[\n" + ",\n".join(items) + "\n" + end + "]"


def code(obj):
    text = obj if isinstance(obj, str) else compact(obj)
    return f"<pre><code>{html.escape(text)}</code></pre>"


def e(s):
    return html.escape(s)


tool_desc = {t["name"]: t["description"] for t in tools}
tool_args = {t["name"]: t["inputSchema"].get("properties", {}) for t in tools}
order = ["search_components", "get_component_details", "suggest_alternatives", "list_categories", "estimate_pcb_quote", "service_status"]

tool_sections = []
for name in order:
    rows = "".join(
        f"<tr><td><code>{e(k)}</code></td><td>{e(str(v.get('type', '') if not isinstance(v.get('type'), list) else '/'.join(v['type'])))}</td><td>{e(v.get('description', ''))}</td></tr>"
        for k, v in tool_args[name].items()
    ) or "<tr><td colspan=3><em>no arguments</em></td></tr>"
    exs = "".join(
        f"<p class='cap'>Live call: <code>{e(json.dumps(a, ensure_ascii=False))}</code></p>{code(out)}"
        for (n, a, out) in samples if n == name
    )
    tool_sections.append(f"""
<section class="tool">
<h3><code>{name}</code></h3>
<p>{e(tool_desc[name])}</p>
<table class="args"><tr><th>Argument</th><th>Type</th><th>Description</th></tr>{rows}</table>
{exs}
</section>""")

WIN_EXE = r"C:\Users\YOUR_USERNAME\.cargo\bin\jlcpcb-mcp.exe"


def cfg(cmd):
    return json.dumps({"mcpServers": {"jlcpcb": {"command": cmd, "args": []}}}, indent=2)

HTML = f"""<!doctype html><html><head><meta charset="utf-8"><title>jlcpcb-mcp guide</title>
<style>
@page {{ size: A4; margin: 18mm 16mm 18mm 16mm; }}
:root {{ --accent:#0b6e4f; --ink:#1d2433; --muted:#5b6475; --line:#dfe3ea; --code:#f5f7fa; }}
body {{ font-family: "Inter","Segoe UI","Helvetica Neue",Arial,sans-serif; color:var(--ink); font-size:10.2pt; line-height:1.5; }}
h1 {{ font-size:30pt; margin:0 0 6pt; letter-spacing:-0.5pt; }}
h2 {{ font-size:16pt; color:var(--accent); border-bottom:2px solid var(--accent); padding-bottom:3pt; margin-top:22pt; break-after:avoid; }}
h3 {{ font-size:12pt; margin:14pt 0 4pt; break-after:avoid; }}
code {{ font-family:"JetBrains Mono","DejaVu Sans Mono",Menlo,monospace; font-size:8.6pt; background:var(--code); padding:0 2pt; border-radius:2pt; }}
pre {{ background:var(--code); border:1px solid var(--line); border-left:3px solid var(--accent); border-radius:4pt; padding:7pt 9pt; white-space:pre-wrap; word-break:break-word; font-size:8pt; line-height:1.35; }}
pre code {{ background:none; padding:0; font-size:8pt; }}
table {{ border-collapse:collapse; width:100%; margin:6pt 0 10pt; font-size:9pt; }}
th,td {{ border:1px solid var(--line); padding:4pt 6pt; text-align:left; vertical-align:top; }}
th {{ background:#eef4f1; }}
.cover {{ height:250mm; display:flex; flex-direction:column; justify-content:center; break-after:page; }}
.cover .tag {{ color:var(--accent); font-weight:700; letter-spacing:2pt; text-transform:uppercase; font-size:10pt; }}
.cover .sub {{ font-size:14pt; color:var(--muted); max-width:150mm; }}
.cover .meta {{ margin-top:30pt; color:var(--muted); font-size:9.5pt; border-top:1px solid var(--line); padding-top:10pt; }}
.chips span {{ display:inline-block; border:1px solid var(--accent); color:var(--accent); border-radius:10pt; padding:1pt 8pt; margin:2pt 4pt 2pt 0; font-size:9pt; }}
.note {{ background:#fff7e6; border:1px solid #f0d7a1; border-radius:4pt; padding:6pt 9pt; }}
.cap {{ color:var(--muted); font-size:9pt; margin:8pt 0 2pt; }}
section.tool {{ break-inside:auto; }}
.toc li {{ margin:2pt 0; }}
</style></head><body>

<div class="cover">
  <div class="tag">Model Context Protocol server · Rust</div>
  <h1>jlcpcb-mcp</h1>
  <p class="sub">Find and evaluate JLCPCB / LCSC electronic components for PCB assembly from any MCP-capable AI assistant.</p>
  <div class="chips"><span>Live search</span><span>Basic / Preferred / Extended</span><span>Stock &amp; price breaks</span><span>Alternatives</span><span>No credentials needed</span></div>
  <div class="meta">Version {e(init['serverInfo']['version'])} · MIT License · github.com/shubhxho/jlcpcb-mcp<br>
  Sample outputs captured live from the JLCPCB and LCSC endpoints on {stamp}. Stock and prices change continuously.</div>
</div>

<h2>Contents</h2>
<ol class="toc"><li>Overview</li><li>Install</li><li>Client configuration per OS</li><li>Tool reference with live outputs</li><li>Errors</li><li>Data sources</li><li>PCB estimate model</li><li>Limitations</li></ol>

<h2>1. Overview</h2>
<p><b>jlcpcb-mcp</b> is a single Rust binary that speaks MCP over stdio (built on the official <code>rmcp</code> SDK). It gives an assistant six read-only tools to search JLCPCB's assembly parts library, inspect a part's live stock and pricing, find cheaper or better-stocked substitutes, browse categories and get a rough bare-PCB cost.</p>
<ul>
<li><b>Live data, no download.</b> Queries go to the same public endpoints that power jlcpcb.com/parts. There's no catalog to download, and responses are cached in memory for 5 minutes (24 h for categories).</li>
<li><b>Assembly-aware ranking.</b> Results are ordered Basic → Preferred Extended → Extended, then by JLCPCB assembly stock, then by unit price.</li>
<li><b>Two stock pools reported separately.</b> <code>jlc_assembly_stock</code> is what matters for PCBA. <code>lcsc_retail_stock</code> is LCSC's retail warehouse and is often 0 for parts JLCPCB has plenty of.</li>
<li><b>Optional official API.</b> Set <code>JLCPCB_APP_ID</code>, <code>JLCPCB_API_KEY</code> and <code>JLCPCB_API_SECRET</code> to add the official record to part details.</li>
</ul>
<p>The server negotiated protocol version <code>{e(init['protocolVersion'])}</code> in the run used for this guide and advertised <code>{len(tools)}</code> tools.</p>

<h2>2. Install</h2>
<p>Requires Rust 1.89+ (<code>rustup</code>).</p>
{code('''cargo install --git https://github.com/shubhxho/jlcpcb-mcp
# or from a checkout
git clone https://github.com/shubhxho/jlcpcb-mcp && cd jlcpcb-mcp
cargo install --path .
jlcpcb-mcp --version''')}
<p>The binary goes to <code>~/.cargo/bin/jlcpcb-mcp</code> (<code>%USERPROFILE%\\.cargo\\bin\\jlcpcb-mcp.exe</code> on Windows). Without a Rust toolchain, build an MCP Bundle with <code>scripts/build-mcpb.sh</code> on each target platform and open the <code>.mcpb</code> file in Claude Desktop.</p>

<h2>3. Client configuration per OS</h2>
<p>Copy-ready files are in <code>examples/</code>. If a config file already exists, merge the <code>"jlcpcb"</code> entry into its <code>mcpServers</code> object. Claude Desktop doesn't read your shell <code>PATH</code>, so its examples use an absolute path to the binary.</p>
<table><tr><th>Client / OS</th><th>Config file</th><th>Example in repo</th></tr>
<tr><td>Claude Desktop · macOS</td><td><code>~/Library/Application Support/Claude/claude_desktop_config.json</code></td><td><code>examples/claude-desktop/macos.claude_desktop_config.json</code></td></tr>
<tr><td>Claude Desktop · Windows</td><td><code>%APPDATA%\\Claude\\claude_desktop_config.json</code></td><td><code>examples/claude-desktop/windows.claude_desktop_config.json</code></td></tr>
<tr><td>Claude Desktop · Linux (community builds)</td><td><code>~/.config/Claude/claude_desktop_config.json</code></td><td><code>examples/claude-desktop/linux.claude_desktop_config.json</code></td></tr>
<tr><td>Cursor · project</td><td><code>&lt;project&gt;/.cursor/mcp.json</code></td><td><code>examples/cursor/project/.cursor/mcp.json</code></td></tr>
<tr><td>Cursor · global</td><td><code>~/.cursor/mcp.json</code></td><td><code>examples/cursor/global/mcp.json</code></td></tr>
<tr><td>Claude Code</td><td colspan=2><code>claude mcp add jlcpcb -- jlcpcb-mcp</code></td></tr></table>
<h3>macOS (Claude Desktop)</h3>{code(cfg('/Users/YOUR_USERNAME/.cargo/bin/jlcpcb-mcp'))}
<h3>Windows (Claude Desktop)</h3>{code(cfg(WIN_EXE))}
<h3>Linux (Claude Desktop community builds)</h3>{code(cfg('/home/YOUR_USERNAME/.cargo/bin/jlcpcb-mcp'))}
<h3>Cursor (project or global)</h3>{code(cfg('jlcpcb-mcp'))}
<p>One-click Cursor link (after installing the binary): <code>cursor://anysphere.cursor-deeplink/mcp/install?name=jlcpcb&amp;config=eyJjb21tYW5kIjoiamxjcGNiLW1jcCIsImFyZ3MiOltdfQ%3D%3D</code></p>
<h3>Environment variables</h3>
<table><tr><th>Variable</th><th>Default</th><th>Purpose</th></tr>
<tr><td><code>JLCPCB_APP_ID</code>, <code>JLCPCB_API_KEY</code>, <code>JLCPCB_API_SECRET</code></td><td>unset</td><td>Optional official API (all three required)</td></tr>
<tr><td><code>JLCPCB_MCP_TIMEOUT_SECS</code></td><td>20</td><td>Per-request timeout</td></tr>
<tr><td><code>JLCPCB_MCP_LOG</code></td><td>warn</td><td>Log filter; logs go to stderr</td></tr></table>

<h2>4. Tool reference with live outputs</h2>
<p class="note">The outputs below are real responses from the run on {stamp}, trimmed to the most useful fields for print. The full responses also include parameters, every price break, URLs and notes. Prices are USD component prices and exclude assembly fees.</p>
{''.join(tool_sections)}

<h2>5. Errors</h2>
<p>Tool failures come back as MCP tool errors (<code>isError: true</code>) with a message you can act on. Network failures, timeouts, HTTP 429/403 throttling (retried with backoff first), upstream rejections and format changes each get their own message. If JLCPCB or LCSC is down, <code>get_component_details</code> still returns the other source's data and adds a warning.</p>
<p class="cap">Live: unknown part number</p>{code(err)}
<p class="cap">Live: ambiguous category</p>{code(err2)}

<h2>6. Data sources</h2>
<table><tr><th>Endpoint</th><th>Used for</th><th>Auth</th></tr>
<tr><td><code>POST jlcpcb.com/api/overseas-pcb-order/v1/shoppingCart/smtGood/selectSmtComponentList/v2</code></td><td>Search, filters, category tree, alternatives</td><td>none</td></tr>
<tr><td><code>GET jlcpcb.com/api/overseas-pcb-order/v1/smtComponentShopCart/getComponentDetail</code></td><td>Exact part detail: assembly stock, price ladder, MSL, process</td><td>none</td></tr>
<tr><td><code>GET wmsc.lcsc.com/ftps/wm/product/detail</code></td><td>LCSC retail stock and price breaks</td><td>none</td></tr>
<tr><td><code>POST open.jlcpcb.com/overseas/openapi/component/getComponentDetailByCode</code></td><td>Official record (optional)</td><td>HMAC-SHA256</td></tr></table>
<p>The endpoint shapes were worked out from the public JLCPCB parts site and the open-source projects yaqwsx/jlcparts, Eyalm321/jlcpcb-mcp, peterb154/jlcpcb-search-mcp, mageoch/JLCPCB-MCP-Server and PatrickWalther/go-jlcpcb-parts.</p>

<h2>7. PCB estimate model</h2>
<p class="note"><b>Not an official quote.</b> JLCPCB has no public, unauthenticated PCB pricing API. <code>estimate_pcb_quote</code> is a parametric model whose coefficients are the author's assumptions, and it always returns <code>is_official_quote: false</code>. Get the real price at cart.jlcpcb.com/quote.</p>
<table><tr><th></th><th>1 L</th><th>2 L</th><th>4 L</th><th>6 L</th><th>8 L</th></tr>
<tr><td>≤5 pcs, ≤100×100 mm (flat)</td><td>$2</td><td>$2</td><td>$7</td><td>$30</td><td>$60</td></tr>
<tr><td>Otherwise: setup fee</td><td>$5</td><td>$5</td><td>$15</td><td>$45</td><td>$80</td></tr>
<tr><td>+ area cost per cm²</td><td>$0.008</td><td>$0.010</td><td>$0.025</td><td>$0.055</td><td>$0.085</td></tr></table>
<p>Volume factor: ×0.8 above 1 m², ×0.65 above 5 m². Non-1.6 mm thickness: +10 % (min $2). Lead-free HASL: +5 % (min $1). ENIG: +$15 + 10 %. Returned range: −25 % / +50 %. Shipping, taxes, assembly and components are excluded.</p>

<h2>8. Limitations</h2>
<ul>
<li>The JLCPCB and LCSC endpoints are undocumented and can change. If a format changes, the tools return a decode error rather than wrong data.</li>
<li>Official API support follows the community-documented signing scheme and hasn't been verified with real credentials. With invalid credentials the official API returns HTTP 401, the tool reports it as a warning, and the other sources still answer.</li>
<li>The package filter needs JLCPCB's exact package string (e.g. <code>SOT-23-3</code>, not <code>SOT23</code>).</li>
<li>Alternatives are rule-based. Check pinout, ratings and footprint in the datasheet before substituting.</li>
<li>Stock and prices are point-in-time values. The figures in this guide will be out of date when you read it.</li>
<li>An MCPB bundle contains a binary for one OS/arch, so build one per platform.</li>
</ul>
</body></html>"""

tmp = tempfile.mkdtemp()
src = os.path.join(tmp, "guide.html")
open(src, "w", encoding="utf-8").write(HTML)
chrome = next((shutil.which(b) for b in ("chromium", "chromium-browser", "google-chrome", "google-chrome-stable") if shutil.which(b)), None)
if not chrome:
    sys.exit("need chromium or google-chrome on PATH")
subprocess.run([chrome, "--headless=new", "--no-sandbox", "--disable-gpu", "--no-pdf-header-footer",
                f"--print-to-pdf={OUT}", "file://" + src], check=True, stderr=subprocess.DEVNULL)
shutil.copy(src, os.path.join(tmp, "keep.html"))
print("wrote", OUT, "(html:", src + ")")

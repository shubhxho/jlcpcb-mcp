#!/usr/bin/env python3
"""Generate site/index.html from site/samples.json (live outputs) and copy the PDF guide.
Usage: python3 site/scripts/build_site.py   (run capture_samples.py first to refresh samples)"""
import html, json, os, shutil

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SITE = os.path.join(ROOT, "site")
d = json.load(open(os.path.join(SITE, "samples.json")))
e = html.escape
REPO = "https://github.com/shubhxho/jlcpcb-mcp"
DEEPLINK = "cursor://anysphere.cursor-deeplink/mcp/install?name=jlcpcb&config=eyJjb21tYW5kIjoiamxjcGNiLW1jcCIsImFyZ3MiOltdfQ%3D%3D"


def compact(obj, depth=0, width=78):
    one = json.dumps(obj, ensure_ascii=False)
    if len(one) + 2 * depth <= width or not isinstance(obj, (dict, list)) or not obj:
        return one
    pad, end = "  " * (depth + 1), "  " * depth
    if isinstance(obj, dict):
        items = [f"{pad}{json.dumps(k, ensure_ascii=False)}: {compact(v, depth + 1, width)}" for k, v in obj.items()]
        return "{\n" + ",\n".join(items) + "\n" + end + "}"
    return "[\n" + ",\n".join(pad + compact(v, depth + 1, width) for v in obj) + "\n" + end + "]"


def ex(path):
    return open(os.path.join(ROOT, "examples", path)).read().strip()


TOOLS = [
    ("search_components", "Search", "Keyword search across the live JLCPCB assembly catalog with category, package, stock and Basic/Preferred filters. Results are ranked Basic → Preferred → Extended, then by stock."),
    ("get_component_details", "Details", "Everything about one LCSC part: JLCPCB assembly stock and price breaks, LCSC retail stock, library class, parameters, datasheet, MSL and assembly process."),
    ("suggest_alternatives", "Alternatives", "Cheaper, better-stocked or Basic-library substitutes in the same category, package and key value."),
    ("list_categories", "Categories", "Browse the live JLCPCB category tree with part counts, or find the exact category name to filter on."),
    ("estimate_pcb_quote", "PCB estimate", "Rough bare-PCB cost from size, layers, quantity and finish — an approximate model, clearly labelled as not an official quote."),
    ("service_status", "Status", "Uncached reachability and latency checks for the JLCPCB and LCSC endpoints, plus whether the optional official API is configured."),
]
args = {t["name"]: t["args"] for t in d["tools"]}

cards = []
for name, label, blurb in TOOLS:
    s = [x for x in d["samples"] if x["tool"] == name]
    samples = "".join(
        f"""<div class="call"><span class="k">call</span> <code>{e(json.dumps(x['args'], ensure_ascii=False))}</code></div>
<pre class="out"><code>{e(compact(x['output']))}</code></pre>""" for x in s[:1])
    argl = " ".join(f"<code>{e(a)}</code>" for a in args[name]) or "<em>no arguments</em>"
    cards.append(f"""<article class="tool" id="t-{name}">
  <header><span class="tag">{e(label)}</span><h3><code>{name}</code></h3></header>
  <p>{e(blurb)}</p>
  <p class="args"><span>Arguments</span> {argl}</p>
  {samples}
</article>""")

errs = "".join(f"<pre class='out err'><code>{e(compact(x))}</code></pre>" for x in d["errors"])

CONF = [
    ("claude-mac", "Claude Desktop · macOS", "~/Library/Application Support/Claude/claude_desktop_config.json", "claude-desktop/macos.claude_desktop_config.json"),
    ("claude-win", "Claude Desktop · Windows", r"%APPDATA%\Claude\claude_desktop_config.json", "claude-desktop/windows.claude_desktop_config.json"),
    ("claude-linux", "Claude Desktop · Linux*", "~/.config/Claude/claude_desktop_config.json", "claude-desktop/linux.claude_desktop_config.json"),
    ("cursor-project", "Cursor · project", "<your-project>/.cursor/mcp.json", "cursor/project/.cursor/mcp.json"),
    ("cursor-global", "Cursor · global", "~/.cursor/mcp.json", "cursor/global/mcp.json"),
]
tabs = "".join(f'<input type="radio" name="cfg" id="{i}"{" checked" if n == 0 else ""}><label for="{i}">{e(t)}</label>' for n, (i, t, _, _) in enumerate(CONF))
panes = "".join(f"""<div class="pane" data-for="{i}"><div class="path"><span>File</span> <code>{e(p)}</code><button class="copy" data-copy="{e(p)}">Copy path</button></div>
<pre><code>{e(ex(f))}</code></pre><p class="src">Source: <a href="{REPO}/blob/main/examples/{f}"><code>examples/{e(f)}</code></a></p></div>""" for (i, _, p, f) in CONF)
tab_css = "".join(f"#{i}:checked ~ .panes [data-for={i}]{{display:block}} #{i}:checked + label{{background:var(--acc);color:#04130c;border-color:var(--acc)}}" for (i, _, _, _) in CONF)

si = d["server_info"]
HTML = f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>jlcpcb-mcp — JLCPCB / LCSC parts for any MCP client</title>
<meta name="description" content="A Rust MCP server that lets AI assistants search JLCPCB/LCSC components live: stock, price breaks, Basic/Extended class, alternatives, categories and rough PCB estimates.">
<meta property="og:title" content="jlcpcb-mcp">
<meta property="og:description" content="Live JLCPCB / LCSC component search for any MCP-capable AI assistant.">
<link rel="icon" href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32'%3E%3Crect width='32' height='32' rx='7' fill='%2310b981'/%3E%3Cpath d='M9 10h14v12H9z' fill='none' stroke='%2304130c' stroke-width='2.4'/%3E%3Cpath d='M12 6v4M16 6v4M20 6v4M12 22v4M16 22v4M20 22v4' stroke='%2304130c' stroke-width='2.4'/%3E%3C/svg%3E">
<style>
:root{{--bg:#0a0d12;--bg2:#0f141b;--card:#121923;--line:#1f2a37;--tx:#e6edf3;--mut:#8b98a8;--acc:#10b981;--acc2:#34d399;--warn:#f59e0b;--mono:ui-monospace,SFMono-Regular,"JetBrains Mono",Menlo,Consolas,monospace}}
*{{box-sizing:border-box}}html{{scroll-behavior:smooth}}
body{{margin:0;background:var(--bg);color:var(--tx);font:16px/1.6 Inter,ui-sans-serif,system-ui,-apple-system,"Segoe UI",Roboto,sans-serif;-webkit-font-smoothing:antialiased}}
a{{color:var(--acc2);text-decoration:none}}a:hover{{text-decoration:underline}}
code{{font-family:var(--mono);font-size:.88em}}
.wrap{{max-width:1120px;margin:0 auto;padding:0 24px}}
nav{{position:sticky;top:0;z-index:10;backdrop-filter:blur(10px);background:rgba(10,13,18,.78);border-bottom:1px solid var(--line)}}
nav .wrap{{display:flex;align-items:center;gap:24px;height:58px}}
.brand{{display:flex;align-items:center;gap:10px;font-weight:700;color:var(--tx)}}
.brand i{{width:22px;height:22px;border-radius:6px;background:var(--acc);display:inline-block;box-shadow:0 0 18px rgba(16,185,129,.45)}}
nav .links{{display:flex;gap:20px;margin-left:auto;font-size:14px}}nav .links a{{color:var(--mut)}}nav .links a:hover{{color:var(--tx);text-decoration:none}}
.hero{{position:relative;padding:96px 0 72px;overflow:hidden}}
.hero:before{{content:"";position:absolute;inset:0;background:radial-gradient(600px 300px at 18% 10%,rgba(16,185,129,.18),transparent 70%),radial-gradient(500px 260px at 85% 30%,rgba(56,189,248,.10),transparent 70%);pointer-events:none}}
.hero:after{{content:"";position:absolute;inset:0;background-image:linear-gradient(var(--line) 1px,transparent 1px),linear-gradient(90deg,var(--line) 1px,transparent 1px);background-size:44px 44px;opacity:.25;mask-image:linear-gradient(#000,transparent 85%);-webkit-mask-image:linear-gradient(#000,transparent 85%);pointer-events:none}}
.hero .wrap{{position:relative;z-index:1;display:grid;grid-template-columns:minmax(0,1.05fr) minmax(0,.95fr);gap:48px;align-items:center}}
.eyebrow{{font:600 12px/1 var(--mono);letter-spacing:.14em;text-transform:uppercase;color:var(--acc2)}}
h1{{font-size:56px;line-height:1.05;margin:16px 0 18px;letter-spacing:-.03em}}
h1 span{{background:linear-gradient(90deg,var(--acc2),#38bdf8);-webkit-background-clip:text;background-clip:text;color:transparent}}
.lead{{font-size:19px;color:#c3cdd8;max-width:560px}}
.cta{{display:flex;flex-wrap:wrap;gap:12px;margin:28px 0 22px}}
.btn{{display:inline-flex;align-items:center;gap:8px;padding:11px 18px;border-radius:10px;font-weight:600;font-size:15px;border:1px solid var(--line);color:var(--tx);background:var(--card)}}
.btn:hover{{text-decoration:none;border-color:#2c3a4b}}
.btn.pri{{background:var(--acc);color:#04130c;border-color:var(--acc)}}.btn.pri:hover{{background:var(--acc2)}}
.chips{{display:flex;flex-wrap:wrap;gap:8px}}.chips span{{font-size:13px;color:var(--mut);border:1px solid var(--line);border-radius:999px;padding:4px 11px}}
.term{{background:#0b1016;border:1px solid var(--line);border-radius:14px;box-shadow:0 30px 80px rgba(0,0,0,.45);overflow:hidden}}
.term .bar{{display:flex;gap:7px;padding:12px 14px;border-bottom:1px solid var(--line);align-items:center}}
.term .bar b{{width:11px;height:11px;border-radius:50%;background:#2a3441}}.term .bar em{{margin-left:10px;font:12px var(--mono);color:var(--mut);font-style:normal}}
.term pre{{margin:0;padding:18px 18px;white-space:pre-wrap;overflow-wrap:anywhere;font:12.5px/1.6 var(--mono);color:#cfe3d8;overflow:auto}}
.term .p{{color:var(--acc2)}}.term .c{{color:#6b7a8c}}
section{{padding:72px 0;border-top:1px solid var(--line)}}
h2{{font-size:34px;letter-spacing:-.02em;margin:0 0 10px}}.sub{{color:var(--mut);margin:0 0 34px;max-width:720px}}
.tools{{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:20px}}
.tool{{background:var(--card);border:1px solid var(--line);border-radius:14px;padding:22px;display:flex;flex-direction:column;min-width:0}}
.tool header{{display:flex;align-items:center;gap:10px}}.tool h3{{margin:0;font-size:17px}}
.tag{{font:600 11px var(--mono);text-transform:uppercase;letter-spacing:.08em;color:var(--acc2);background:rgba(16,185,129,.1);border:1px solid rgba(16,185,129,.25);padding:3px 8px;border-radius:6px}}
.tool>p{{color:#c3cdd8;margin:12px 0 10px;font-size:15px}}
.args{{font-size:13px;color:var(--mut)}}.args span{{margin-right:6px}}.args code{{background:#0b1016;border:1px solid var(--line);border-radius:5px;padding:1px 6px;margin:0 3px 4px 0;display:inline-block;color:#cbd5e1}}
.call{{margin-top:10px;font-size:13px;color:var(--mut)}}.call .k{{font:600 11px var(--mono);color:var(--warn);text-transform:uppercase}}.call code{{color:#e2e8f0}}
pre{{white-space:pre-wrap;overflow-wrap:anywhere;background:#0b1016;border:1px solid var(--line);border-radius:10px;padding:14px 16px;overflow:auto;font:12.5px/1.55 var(--mono);color:#cfd8e3;margin:10px 0 0}}
pre.out{{max-height:340px}}pre.err code{{color:#fca5a5}}
.note{{font-size:14px;color:var(--mut);margin-top:18px}}
.grid2{{display:grid;grid-template-columns:1fr 1fr;gap:20px}}
.box{{background:var(--card);border:1px solid var(--line);border-radius:14px;padding:22px}}.box h3{{margin:0 0 6px;font-size:18px}}.box p{{color:#c3cdd8;font-size:15px;margin:6px 0}}
.badge{{display:inline-block;font:600 11px var(--mono);color:var(--warn);border:1px solid rgba(245,158,11,.35);background:rgba(245,158,11,.08);border-radius:6px;padding:2px 7px;margin-left:6px;vertical-align:middle}}
.cfg input{{display:none}}.cfg label{{display:inline-block;cursor:pointer;font-size:14px;padding:8px 14px;border:1px solid var(--line);border-radius:999px;margin:0 8px 10px 0;color:#c3cdd8;background:var(--card)}}
.pane{{display:none}}{tab_css}
.path{{display:flex;flex-wrap:wrap;align-items:center;gap:10px;margin-top:8px;font-size:14px;color:var(--mut)}}.path code{{color:var(--tx);background:#0b1016;border:1px solid var(--line);padding:4px 9px;border-radius:7px}}
.copy{{font:12px var(--mono);background:transparent;color:var(--mut);border:1px solid var(--line);border-radius:7px;padding:4px 9px;cursor:pointer}}.copy:hover{{color:var(--tx)}}
.src{{font-size:13px;color:var(--mut);margin:8px 0 0}}
table{{width:100%;border-collapse:collapse;font-size:14px}}td,th{{text-align:left;padding:10px 12px;border-bottom:1px solid var(--line);vertical-align:top}}th{{color:var(--mut);font-weight:600}}
.pdf{{display:flex;align-items:center;justify-content:space-between;gap:24px;background:linear-gradient(135deg,rgba(16,185,129,.12),rgba(56,189,248,.06));border:1px solid rgba(16,185,129,.3);border-radius:16px;padding:28px}}
.pdf h3{{margin:0 0 4px;font-size:22px}}.pdf p{{margin:0;color:#c3cdd8}}
ul.lim{{margin:0;padding-left:20px;color:#c3cdd8}}ul.lim li{{margin:6px 0}}
footer{{border-top:1px solid var(--line);padding:32px 0 48px;color:var(--mut);font-size:14px}}footer .wrap{{display:flex;flex-wrap:wrap;gap:16px;justify-content:space-between}}
@media (max-width:900px){{.hero .wrap,.tools,.grid2{{grid-template-columns:1fr}}h1{{font-size:42px}}nav .links{{display:none}}.pdf{{flex-direction:column;align-items:flex-start}}}}
</style>
</head>
<body>
<nav><div class="wrap"><a class="brand" href="#"><i></i>jlcpcb-mcp</a>
<div class="links"><a href="#tools">Tools</a><a href="#install">Install</a><a href="#configure">Configure</a><a href="#data">Data sources</a><a href="jlcpcb-mcp-guide.pdf">PDF guide</a><a href="{REPO}">GitHub</a></div></div></nav>

<header class="hero"><div class="wrap">
<div>
<div class="eyebrow">Model Context Protocol server · Rust</div>
<h1>JLCPCB &amp; LCSC parts, <span>live in your AI assistant</span></h1>
<p class="lead">jlcpcb-mcp lets Claude, Cursor or any MCP client search JLCPCB's assembly catalog in real time: stock, price breaks, Basic / Preferred / Extended class, datasheets, cheaper alternatives and rough PCB estimates. No API key required.</p>
<div class="cta"><a class="btn pri" href="#install">Install</a><a class="btn" href="{DEEPLINK}">Add to Cursor</a><a class="btn" href="jlcpcb-mcp-guide.pdf" download>Download PDF guide</a><a class="btn" href="{REPO}">GitHub</a></div>
<div class="chips"><span>6 read-only tools</span><span>stdio transport</span><span>single static binary</span><span>MIT licensed</span></div>
</div>
<div class="term"><div class="bar"><b></b><b></b><b></b><em>stdio · jlcpcb-mcp {e(si['version'])}</em></div>
<pre><span class="c"># tools/call search_components</span>
<span class="p">→</span> {e(json.dumps(d['samples'][1]['args']))}
<span class="p">←</span> {e(compact(d['samples'][1]['output'], 0, 60))}</pre></div>
</div></header>

<section id="tools"><div class="wrap">
<h2>Six tools</h2>
<p class="sub">Every output below is real: captured from the live JLCPCB and LCSC endpoints on {e(d['captured_at'])} (lists trimmed for space). Stock and prices change continuously.</p>
<div class="tools">{''.join(cards)}</div>
<h3 style="margin:40px 0 4px">Errors are tool results, not crashes</h3>
<p class="sub" style="margin-bottom:0">Bad input and missing parts come back as <code>isError</code> results with an actionable message (live outputs):</p>
<div class="grid2">{errs}</div>
</div></section>

<section id="install"><div class="wrap">
<h2>Install</h2>
<p class="sub">Requires a Rust toolchain (1.89+). Both commands put a <code>jlcpcb-mcp</code> binary in <code>~/.cargo/bin</code>.</p>
<div class="grid2">
<div class="box"><h3>From GitHub <span class="badge">once the repo is published</span></h3>
<p>Available as soon as the repository is public at <a href="{REPO}">github.com/shubhxho/jlcpcb-mcp</a>.</p>
<pre><code>cargo install --git {REPO}</code></pre></div>
<div class="box"><h3>From a local checkout</h3><p>Works today from the source tree.</p>
<pre><code>cd jlcpcb-mcp
cargo install --path .
jlcpcb-mcp --version   # jlcpcb-mcp {e(si['version'])}</code></pre></div>
</div>
<p class="note">Optional: set <code>JLCPCB_APP_ID</code>, <code>JLCPCB_API_KEY</code> and <code>JLCPCB_API_SECRET</code> to also query JLCPCB's official Open API. <code>JLCPCB_MCP_TIMEOUT_SECS</code> (default 20) and <code>JLCPCB_MCP_LOG</code> (default warn) tune behaviour.</p>
</div></section>

<section id="configure"><div class="wrap">
<h2>Configure your client</h2>
<p class="sub">Paste the snippet into the file shown, replace <code>YOUR_USERNAME</code>, then fully restart the app. Claude Desktop does not read your shell <code>PATH</code>, so its configs use an absolute path.</p>
<div class="cfg">{tabs}<div class="panes">{panes}</div></div>
<p class="note">* Anthropic does not ship an official Linux build of Claude Desktop; the path above is the one used by community builds.</p>
<div class="box" style="margin-top:24px"><h3>One-click install for Cursor</h3>
<p>With <code>jlcpcb-mcp</code> on your <code>PATH</code>, open this link to add the server to Cursor:</p>
<p><a class="btn pri" href="{DEEPLINK}" style="margin-top:8px">Add jlcpcb-mcp to Cursor</a></p>
<pre><code>{e(DEEPLINK)}</code></pre></div>
</div></section>

<section id="data"><div class="wrap">
<h2>Data sources &amp; limits</h2>
<p class="sub">Nothing is bundled or scraped into a local database: each call goes to the live endpoints below.</p>
<div class="box"><table>
<tr><th>Endpoint</th><th>Used for</th><th>Auth</th></tr>
<tr><td><code>jlcpcb.com … /selectSmtComponentList/v2</code></td><td>Search, category tree, alternatives</td><td>none</td></tr>
<tr><td><code>jlcpcb.com … /getComponentDetail</code></td><td>Assembly stock, price breaks, attributes</td><td>none</td></tr>
<tr><td><code>wmsc.lcsc.com/ftps/wm/product/detail</code></td><td>LCSC retail stock and prices, datasheet</td><td>none</td></tr>
<tr><td><code>open.jlcpcb.com … /getComponentDetailByCode</code></td><td>Official API (optional)</td><td>HMAC-SHA256</td></tr>
</table></div>
<div class="box" style="margin-top:20px"><h3>Limitations</h3><ul class="lim">
<li>The JLCPCB and LCSC web endpoints are undocumented and can change without notice.</li>
<li><code>estimate_pcb_quote</code> is an approximate parametric model, not a JLCPCB quote. Always confirm at <a href="https://cart.jlcpcb.com/quote">cart.jlcpcb.com/quote</a>.</li>
<li>Alternatives are rule-based (category, package, key value); check ratings and tolerances in the datasheet before substituting.</li>
<li>Stock and prices are a snapshot at call time.</li>
</ul></div>
</div></section>

<section id="guide"><div class="wrap">
<div class="pdf"><div><h3>The full guide, as a PDF</h3><p>Install, per-OS configs, the complete tool reference with live outputs, data sources and the PCB estimate model.</p></div>
<a class="btn pri" href="jlcpcb-mcp-guide.pdf" download>Download jlcpcb-mcp-guide.pdf</a></div>
</div></section>

<footer><div class="wrap"><span>jlcpcb-mcp {e(si['version'])} · MIT License © 2026 shubhxho</span>
<span><a href="{REPO}">github.com/shubhxho/jlcpcb-mcp</a> · Not affiliated with JLCPCB or LCSC.</span></div></footer>
<script>
document.querySelectorAll('.copy').forEach(b=>b.addEventListener('click',()=>{{navigator.clipboard&&navigator.clipboard.writeText(b.dataset.copy);b.textContent='Copied';setTimeout(()=>b.textContent='Copy path',1200)}}));
</script>
</body></html>
"""
open(os.path.join(SITE, "index.html"), "w").write(HTML)
shutil.copy(os.path.join(ROOT, "docs/jlcpcb-mcp-guide.pdf"), os.path.join(SITE, "jlcpcb-mcp-guide.pdf"))
print("wrote site/index.html", len(HTML), "bytes")

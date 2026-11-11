# jlcpcb-mcp

A [Model Context Protocol](https://modelcontextprotocol.io) server, written in Rust, that lets an AI
assistant (Claude Desktop, Cursor, VS Code, Zed, …) **find and evaluate electronic components for
PCB assembly at JLCPCB**: live parametric search, live stock and price breaks, Basic / Preferred /
Extended library class, datasheets, and cheaper or better-stocked substitutes.

- Single static-ish binary, stdio transport, built on the official Rust SDK [`rmcp`](https://crates.io/crates/rmcp).
- **No credentials and no catalog download needed.** Every query goes live to the same public
  endpoints that power <https://jlcpcb.com/parts>, so stock and prices are current.
- Optional official JLCPCB API support (`JLCPCB_APP_ID` / `JLCPCB_API_KEY` / `JLCPCB_API_SECRET`).
- Clear, actionable errors for network failures, timeouts, rate limiting and upstream format changes.

> **Unofficial.** This project is not affiliated with JLCPCB or LCSC. The public parts endpoints are
> undocumented and can change without notice; see [Caveats](#caveats).

## Install

Requires Rust 1.89+ (`rustup` recommended).

```sh
cargo install --git https://github.com/shubhxho/jlcpcb-mcp
```

or from a checkout:

```sh
git clone https://github.com/shubhxho/jlcpcb-mcp && cd jlcpcb-mcp
cargo install --path .
```

This puts `jlcpcb-mcp` in `~/.cargo/bin`. Run `jlcpcb-mcp --help` to check it is on your `PATH`.
The server speaks MCP over stdin/stdout; you normally don't run it by hand.

## Configure your MCP client

Use the absolute path to the binary if `~/.cargo/bin` isn't on the client's `PATH`
(e.g. `/Users/you/.cargo/bin/jlcpcb-mcp`, or `C:\\Users\\you\\.cargo\\bin\\jlcpcb-mcp.exe`).

### Claude Desktop

`~/Library/Application Support/Claude/claude_desktop_config.json` (macOS) or
`%APPDATA%\Claude\claude_desktop_config.json` (Windows):

```json
{
  "mcpServers": {
    "jlcpcb": {
      "command": "jlcpcb-mcp",
      "args": []
    }
  }
}
```

### Cursor

`~/.cursor/mcp.json` (global) or `.cursor/mcp.json` (per project):

```json
{
  "mcpServers": {
    "jlcpcb": {
      "command": "jlcpcb-mcp",
      "args": []
    }
  }
}
```

### Claude Code

```sh
claude mcp add jlcpcb -- jlcpcb-mcp
```

### With official API credentials (optional)

```json
{
  "mcpServers": {
    "jlcpcb": {
      "command": "jlcpcb-mcp",
      "env": {
        "JLCPCB_APP_ID": "your-app-id",
        "JLCPCB_API_KEY": "your-access-key",
        "JLCPCB_API_SECRET": "your-secret"
      }
    }
  }
}
```

### Environment variables

| Variable | Default | Purpose |
|---|---|---|
| `JLCPCB_APP_ID`, `JLCPCB_API_KEY`, `JLCPCB_API_SECRET` | unset | Enable the official JLCPCB API record in `get_component_details` (all three required). |
| `JLCPCB_MCP_TIMEOUT_SECS` | `20` | Per-request timeout. |
| `JLCPCB_MCP_LOG` | `warn` | Log filter (`info`, `debug`, …). Logs go to **stderr**. |
| `JLCPCB_MCP_JLC_BASE_URL` | `https://jlcpcb.com/api` | Override the JLCPCB endpoint root (proxies / testing). |
| `JLCPCB_MCP_LCSC_BASE_URL` | `https://wmsc.lcsc.com` | Override the LCSC endpoint root. |
| `JLCPCB_API_BASE_URL` | `https://open.jlcpcb.com` | Override the official API root. |

## Tools

All tools are read-only. Results are returned both as JSON text and as MCP `structuredContent`
(with an output schema).

### `search_components`

Live search with filters. Ranking: **Basic → Preferred Extended → Extended**, then **JLCPCB assembly
stock** (descending), then **unit price** at `quantity` (ascending). Internally it runs the
relevance query plus a dedicated Basic/Preferred query so Basic parts surface even when they aren't on
JLCPCB's first relevance page.

| Argument | Type | Notes |
|---|---|---|
| `query` | string | Value + type, MPN, or LCSC number: `"10k"`, `"100nF X7R"`, `"AMS1117-3.3"`, `"C25744"` |
| `category` | string | Category or sub-category (see `list_categories`); singular/plural and unique partial names work |
| `package` | string | Exact JLCPCB package name(s), comma-separated: `"0402"`, `"SOT-23-3,SOT-23"` |
| `basic_only` | bool | Only Basic parts (default `false`) |
| `include_preferred` | bool | With `basic_only`, also Preferred Extended parts |
| `min_stock` | int | Minimum JLCPCB assembly stock (default `1`; `0` includes out-of-stock) |
| `max_results` | int | 1–50, default 10 |
| `quantity` | int | Quantity used to pick the price tier (default 100) |

```json
{ "query": "10k", "category": "Resistors", "package": "0402", "max_results": 5 }
```

### `get_component_details`

Everything about one LCSC part number, merged from JLCPCB (assembly stock, assembly price breaks,
library class, MSL, assembly process, replacement suggestion) and LCSC (retail stock and price
breaks), plus the official API record when credentials are set.

```json
{ "lcsc": "C25744", "quantity": 1000 }
```

### `suggest_alternatives`

Substitutes in the same category, the same package (by default) and the same value — Resistance,
Capacitance, Inductance, Frequency or Output Voltage, depending on the part (for parts without a
value attribute it matches the MPN family, e.g. `AMS1117*`). Candidates must have **at least** the
original's voltage/power/current rating and **no worse** tolerance or ceramic dielectric (where
parseable), at least `min_stock` (default 100) in stock, and an actual improvement: Basic/Preferred
class, ≥5 % cheaper, or ≥2× the stock. Each suggestion lists its reasons.

```json
{ "lcsc": "C25531", "max_results": 5 }
```

Arguments: `lcsc`, `quantity`, `max_results` (1–25, default 8), `same_package` (default true),
`basic_or_preferred_only`, `min_stock`.

### `list_categories`

The live JLCPCB category tree with part counts.

```json
{ "search": "mosfet" }
```

Arguments: `parent`, `search`, `include_subcategories`.

### `estimate_pcb_quote`

A **rough, approximate** bare-board fabrication cost. JLCPCB has no public, unauthenticated pricing
API (its official `pcb/calculate` endpoint needs approved credentials *and* an uploaded Gerber), so
this is a documented parametric model and **never an official price**. The result always has
`"is_official_quote": false` and links to <https://cart.jlcpcb.com/quote>.

```json
{ "width_mm": 100, "height_mm": 80, "quantity": 10, "layers": 2, "thickness_mm": 1.6, "surface_finish": "hasl" }
```

#### PCB estimate model

All coefficients are the author's assumptions loosely shaped like JLCPCB's advertised low-volume
pricing. They are not published JLCPCB prices and may be stale. See `src/pcb.rs`.

| | 1 L | 2 L | 4 L | 6 L | 8 L |
|---|---|---|---|---|---|
| Small batch (≤5 pcs, ≤100×100 mm), flat | $2 | $2 | $7 | $30 | $60 |
| Otherwise: setup fee | $5 | $5 | $15 | $45 | $80 |
| … plus area cost per cm² of board | $0.008 | $0.010 | $0.025 | $0.055 | $0.085 |

- Area cost discount: ×0.8 above 1 m² total, ×0.65 above 5 m².
- Non-1.6 mm thickness: +10 % (min $2). Lead-free HASL: +5 % (min $1). ENIG: +$15 + 10 %.
- Range returned: −25 % / +50 % around the typical value.
- Excludes shipping, taxes, assembly, components, stencils and special processes.

### `service_status`

Probes the JLCPCB and LCSC endpoints (uncached), and reports whether official credentials are
configured and how many responses are cached.

## Example session

> **You:** Find me a 10k 0402 resistor that's a Basic part.
>
> The assistant calls `search_components {"query":"10k","category":"Resistors","package":"0402","basic_only":true}`
> and gets back `C25744` (UNI-ROYAL 0402WGF1002TCE, Basic, about 20 M in JLCPCB stock, $0.001 at qty 100).

## Data sources

| Source | Used for | Auth |
|---|---|---|
| `POST https://jlcpcb.com/api/overseas-pcb-order/v1/shoppingCart/smtGood/selectSmtComponentList/v2` | search, filters, category tree (`searchType: 3`), alternatives | none |
| `GET https://jlcpcb.com/api/overseas-pcb-order/v1/smtComponentShopCart/getComponentDetail?componentCode=C…` | exact part detail (assembly stock, price ladder, MSL, assembly process) | none |
| `GET https://wmsc.lcsc.com/ftps/wm/product/detail?productCode=C…` | LCSC retail stock and price breaks, extra parameters | none |
| `POST https://open.jlcpcb.com/overseas/openapi/component/getComponentDetailByCode` | official component record (optional) | HMAC-SHA256 (`JLCPCB_*` env vars) |

Endpoint shapes were worked out from the public JLCPCB parts site and from these open-source
projects: [yaqwsx/jlcparts](https://github.com/yaqwsx/jlcparts),
[Eyalm321/jlcpcb-mcp](https://github.com/Eyalm321/jlcpcb-mcp),
[peterb154/jlcpcb-search-mcp](https://github.com/peterb154/jlcpcb-search-mcp),
[mageoch/JLCPCB-MCP-Server](https://github.com/mageoch/JLCPCB-MCP-Server) (official-API signing scheme) and
[PatrickWalther/go-jlcpcb-parts](https://github.com/PatrickWalther/go-jlcpcb-parts) (search request fields).

**Why no local catalog?** The yaqwsx/jlcparts dataset is excellent but means a large first-run
download and a snapshot that ages; JLCPCB's own live search handles parametric filtering and
returns current stock, so this server stays download-free. Responses are cached in memory for
5 minutes (24 h for the category tree) and requests are spaced and retried with backoff.

## Caveats

- **Unofficial endpoints.** The public JLCPCB/LCSC endpoints are undocumented. If JLCPCB changes
  them, tools fail with a "could not understand the … response" error rather than returning wrong
  data. Please open an issue.
- **Two stock pools.** `jlc_stock` / `jlc_assembly_stock` is JLCPCB's assembly inventory, the number
  that matters for PCBA. `lcsc_retail_stock` is LCSC's retail warehouse and is often 0 for parts
  JLCPCB has plenty of.
- **Prices** are JLCPCB / LCSC component prices in USD. They exclude assembly fees, extended-part
  setup fees, shipping and taxes.
- **Package filter** must match JLCPCB's exact package string (`SOT-23-3`, not `SOT23`).
- **Alternatives** are heuristics. Always check the datasheet (pinout, ratings, footprint) before
  substituting.
- **Official API** support follows the community-documented signing scheme. It hasn't been
  verified against live credentials by the author; errors are passed through verbatim.
- **PCB estimates** are a model, not quotes.
- Be polite: the server rate-limits itself, but avoid issuing large bursts of searches.

## Development

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test              # offline: fixtures + local mock HTTP server + in-memory MCP session
cargo build --release
python3 scripts/live_smoke.py target/release/jlcpcb-mcp   # live end-to-end check over stdio (needs network)
```

Tests never touch the network. `tests/fixtures/` holds real responses captured from the live
endpoints (signed URLs scrubbed).

## License

MIT © shubhxho

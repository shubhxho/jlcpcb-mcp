//! Tool logic, independent of the MCP transport so it can be unit tested.

use std::collections::HashSet;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{ApiError, Result};
use crate::http::{HttpClient, HttpConfig};
use crate::jlc::{self, CategoryNode, JlcClient, LibraryFilter, SearchQuery};
use crate::lcsc::{self, LcscClient};
use crate::model::{
    Attribute, ComponentSummary, LibraryType, PriceBreak, normalize_lcsc, unit_price_at,
};
use crate::official::{self, Credentials, OfficialClient};
use crate::values::{dielectric_rank, parse_si, parse_tolerance_pct};

/// Runtime configuration (endpoints are overridable for testing / proxies).
#[derive(Debug, Clone)]
pub struct Config {
    pub jlc_base: String,
    pub lcsc_base: String,
    pub official_base: String,
    pub credentials: Option<Credentials>,
    pub http: HttpConfig,
}

impl Config {
    /// Read configuration from environment variables.
    pub fn from_env() -> Self {
        let get = |k: &str, d: &str| {
            std::env::var(k)
                .ok()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(|| d.to_string())
        };
        let mut http = HttpConfig::default();
        if let Some(secs) = std::env::var("JLCPCB_MCP_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|s| *s > 0)
        {
            http.timeout = std::time::Duration::from_secs(secs);
        }
        Self {
            jlc_base: get("JLCPCB_MCP_JLC_BASE_URL", jlc::DEFAULT_BASE),
            lcsc_base: get("JLCPCB_MCP_LCSC_BASE_URL", lcsc::DEFAULT_BASE),
            official_base: get("JLCPCB_API_BASE_URL", official::DEFAULT_BASE),
            credentials: Credentials::from_env(),
            http,
        }
    }
}

/// Default quantity used to pick a price tier.
pub const DEFAULT_PRICE_QTY: u64 = 100;
const DEFAULT_MAX_RESULTS: u32 = 10;
const MAX_RESULTS_CAP: u32 = 50;

/// The business-logic façade used by the MCP server.
#[derive(Clone)]
pub struct Service {
    http: Arc<HttpClient>,
    jlc: JlcClient,
    lcsc: LcscClient,
    official: Option<OfficialClient>,
}

// ---------------------------------------------------------------------------
// Tool argument / result types
// ---------------------------------------------------------------------------

/// Arguments of `search_components`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
pub struct SearchArgs {
    /// Free-text keyword: value + type ("10k resistor", "100nF"), MPN ("AMS1117-3.3"), or LCSC number ("C25744").
    #[serde(default)]
    pub query: Option<String>,
    /// Category or sub-category name (see `list_categories`), e.g. "Resistors" or "Multilayer Ceramic Capacitors MLCC - SMD/SMT". Partial names work if unambiguous.
    #[serde(default)]
    pub category: Option<String>,
    /// Exact JLCPCB package name(s), comma-separated, e.g. "0402" or "SOT-23-3,SOT-23".
    #[serde(default)]
    pub package: Option<String>,
    /// Only return Basic parts (no per-part extended setup fee). Default false.
    #[serde(default)]
    pub basic_only: Option<bool>,
    /// With `basic_only`, also include Preferred Extended parts. Default false.
    #[serde(default)]
    pub include_preferred: Option<bool>,
    /// Minimum JLCPCB assembly stock. Default 1 (in-stock only); use 0 to include out-of-stock parts.
    #[serde(default)]
    pub min_stock: Option<u64>,
    /// Maximum results to return (1-50, default 10).
    #[serde(default)]
    pub max_results: Option<u32>,
    /// Quantity used to pick the price tier for ranking (default 100).
    #[serde(default)]
    pub quantity: Option<u64>,
}

/// Result of `search_components`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchResult {
    /// Applied filters, after category resolution.
    pub filters: AppliedFilters,
    /// Total matches reported by JLCPCB for the main query.
    pub total_matches: i64,
    pub returned: usize,
    /// Ranked: Basic → Preferred → Extended, then stock (desc), then price (asc).
    pub results: Vec<ComponentSummary>,
    pub notes: Vec<String>,
}

/// Filters actually sent upstream.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct AppliedFilters {
    pub query: Option<String>,
    pub category: Option<String>,
    pub subcategory: Option<String>,
    pub packages: Vec<String>,
    pub library: String,
    pub min_stock: u64,
    pub price_qty: u64,
}

/// Arguments of `get_component_details`.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct DetailsArgs {
    /// LCSC / JLCPCB part number, e.g. "C25744" (the "C" is optional).
    pub lcsc: String,
    /// Quantity used to pick the headline price tier (default 100).
    #[serde(default)]
    pub quantity: Option<u64>,
}

/// Result of `get_component_details`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ComponentDetails {
    pub lcsc: String,
    pub mfr_part: Option<String>,
    pub manufacturer: Option<String>,
    pub package: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub subcategory: Option<String>,
    pub library_type: LibraryType,
    pub library_note: String,
    /// Live JLCPCB assembly stock (what matters for PCBA).
    pub jlc_assembly_stock: Option<i64>,
    /// Live LCSC retail stock (separate pool; 0 here does not block assembly).
    pub lcsc_retail_stock: Option<i64>,
    pub price_qty: u64,
    /// JLCPCB assembly unit price (USD) at `price_qty`.
    pub jlc_unit_price_usd: Option<f64>,
    pub jlc_price_breaks: Vec<PriceBreak>,
    /// LCSC retail price breaks (USD).
    pub lcsc_price_breaks: Vec<PriceBreak>,
    pub attributes: Vec<Attribute>,
    pub datasheet_url: Option<String>,
    pub image_url: Option<String>,
    pub jlcpcb_url: String,
    pub lcsc_url: Option<String>,
    pub assembly_process: Option<String>,
    pub moisture_sensitivity_level: Option<String>,
    pub min_order_qty: Option<i64>,
    pub reel_qty: Option<i64>,
    pub buyable: Option<bool>,
    pub not_buyable_reason: Option<String>,
    pub jlc_suggested_replacement: Option<String>,
    pub eccn: Option<String>,
    pub needs_xray: Option<bool>,
    /// Raw record from the official JLCPCB API, when credentials are configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub official_api: Option<Value>,
    /// Which upstream sources contributed.
    pub sources: Vec<String>,
    pub warnings: Vec<String>,
}

/// Arguments of `suggest_alternatives`.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct AlternativesArgs {
    /// LCSC part number to replace, e.g. "C25531".
    pub lcsc: String,
    /// Quantity used for price comparison (default 100).
    #[serde(default)]
    pub quantity: Option<u64>,
    /// Maximum alternatives (1-25, default 8).
    #[serde(default)]
    pub max_results: Option<u32>,
    /// Require the same package (default true).
    #[serde(default)]
    pub same_package: Option<bool>,
    /// Only suggest Basic / Preferred parts (default false).
    #[serde(default)]
    pub basic_or_preferred_only: Option<bool>,
    /// Minimum JLCPCB assembly stock for a candidate (default 100).
    #[serde(default)]
    pub min_stock: Option<u64>,
}

/// One suggested alternative.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Alternative {
    pub component: ComponentSummary,
    /// Why this part is suggested.
    pub reasons: Vec<String>,
    /// Unit-price change vs the original at `price_qty` (negative = cheaper).
    pub price_change_pct: Option<f64>,
}

/// Result of `suggest_alternatives`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AlternativesResult {
    pub original: ComponentSummary,
    /// How candidates were matched.
    pub match_criteria: Vec<String>,
    pub alternatives: Vec<Alternative>,
    pub notes: Vec<String>,
}

/// Arguments of `list_categories`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
pub struct CategoriesArgs {
    /// Only show this top-level category (and its sub-categories).
    #[serde(default)]
    pub parent: Option<String>,
    /// Case-insensitive substring filter on category / sub-category names, e.g. "mosfet".
    #[serde(default)]
    pub search: Option<String>,
    /// Include sub-categories for every parent (default: only when `parent` or `search` is set).
    #[serde(default)]
    pub include_subcategories: Option<bool>,
}

/// Result of `list_categories`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CategoriesResult {
    pub total_categories: usize,
    pub categories: Vec<CategoryOut>,
}

/// A category with optional sub-categories.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CategoryOut {
    pub name: String,
    pub id: i64,
    pub part_count: i64,
    pub subcategory_count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub subcategories: Vec<SubcategoryOut>,
}

/// A leaf category.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubcategoryOut {
    pub name: String,
    pub id: i64,
    pub part_count: i64,
}

/// Result of `service_status`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StatusResult {
    pub version: String,
    pub jlcpcb_parts_api: EndpointStatus,
    pub lcsc_api: EndpointStatus,
    pub official_api_configured: bool,
    pub cached_responses: usize,
    pub notes: Vec<String>,
}

/// Reachability of one upstream.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EndpointStatus {
    pub ok: bool,
    pub latency_ms: Option<u128>,
    pub detail: String,
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

/// Attributes that define "the same value" for alternatives.
const VALUE_ATTRS: &[&str] = &[
    "Resistance",
    "Capacitance",
    "Inductance",
    "Frequency",
    "Output Voltage",
];

/// Attributes where a candidate must be at least as high as the original.
const AT_LEAST_ATTRS: &[&str] = &[
    "Voltage Rating",
    "Voltage-Supply(Max)",
    "Power(Watts)",
    "Output Current",
    "Current Rating",
    "Voltage - Supply",
];

fn library_note(lib: LibraryType) -> String {
    match lib {
        LibraryType::Basic => "Basic part: loaded on JLCPCB feeders, no extended-part setup fee.",
        LibraryType::Preferred => "Preferred Extended part: extended part whose setup fee is waived for Economic PCBA.",
        LibraryType::Extended => "Extended part: JLCPCB charges a one-off feeder setup fee per unique extended part per order; prefer Basic/Preferred where possible.",
        LibraryType::Unknown => "Library class not reported.",
    }
    .to_string()
}

fn split_packages(raw: Option<&str>) -> Vec<String> {
    raw.unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Resolve a user-supplied category name against the tree.
/// Returns (parent, optional leaf).
pub fn resolve_category(tree: &[CategoryNode], input: &str) -> Result<(String, Option<String>)> {
    let needle = input.trim().to_lowercase();
    if needle.is_empty() {
        return Err(ApiError::InvalidInput("category is empty".into()));
    }
    for p in tree {
        if p.name.to_lowercase() == needle {
            return Ok((p.name.clone(), None));
        }
    }
    for p in tree {
        for c in &p.children {
            if c.name.to_lowercase() == needle {
                return Ok((p.name.clone(), Some(c.name.clone())));
            }
        }
    }
    // Singular/plural tolerance: "capacitor" -> "Capacitors".
    let toggled = match needle.strip_suffix('s') {
        Some(stem) => stem.to_string(),
        None => format!("{needle}s"),
    };
    for p in tree {
        if p.name.to_lowercase() == toggled {
            return Ok((p.name.clone(), None));
        }
        for c in &p.children {
            if c.name.to_lowercase() == toggled {
                return Ok((p.name.clone(), Some(c.name.clone())));
            }
        }
    }
    // A unique top-level substring match wins over leaf matches.
    let parents: Vec<&CategoryNode> = tree
        .iter()
        .filter(|p| p.name.to_lowercase().contains(&needle))
        .collect();
    if parents.len() == 1 {
        return Ok((parents[0].name.clone(), None));
    }
    let mut matches: Vec<(String, Option<String>)> = Vec::new();
    for p in tree {
        if p.name.to_lowercase().contains(&needle) {
            matches.push((p.name.clone(), None));
        }
        for c in &p.children {
            if c.name.to_lowercase().contains(&needle) {
                matches.push((p.name.clone(), Some(c.name.clone())));
            }
        }
    }
    match matches.len() {
        1 => Ok(matches.remove(0)),
        0 => Err(ApiError::NotFound(format!(
            "no JLCPCB category matches {input:?}; call list_categories (optionally with `search`) to see valid names"
        ))),
        n => {
            let names: Vec<String> = matches
                .iter()
                .take(12)
                .map(|(p, l)| match l {
                    Some(l) => format!("{l} (in {p})"),
                    None => p.clone(),
                })
                .collect();
            Err(ApiError::InvalidInput(format!(
                "category {input:?} is ambiguous ({n} matches). Use one of: {}",
                names.join("; ")
            )))
        }
    }
}

/// Merge rows, dedupe by LCSC code, filter by stock and rank.
pub fn merge_and_rank(
    groups: Vec<Vec<ComponentSummary>>,
    min_stock: u64,
    max: usize,
) -> Vec<ComponentSummary> {
    let mut seen = HashSet::new();
    let mut all: Vec<ComponentSummary> = groups
        .into_iter()
        .flatten()
        .filter(|c| seen.insert(c.lcsc.clone()))
        .filter(|c| c.jlc_stock >= min_stock as i64)
        .collect();
    all.sort_by(ComponentSummary::rank_cmp);
    all.truncate(max);
    all
}

/// Whether `cand` is at least as good as `orig` on the constrained attributes.
pub fn meets_constraints(orig: &ComponentSummary, cand: &ComponentSummary) -> bool {
    for name in AT_LEAST_ATTRS {
        if let (Some(o), Some(c)) = (orig.attr(name), cand.attr(name))
            && let (Some(o), Some(c)) = (parse_si(o), parse_si(c))
            && c + 1e-12 < o
        {
            return false;
        }
    }
    if let (Some(o), Some(c)) = (orig.attr("Tolerance"), cand.attr("Tolerance"))
        && let (Some(o), Some(c)) = (parse_tolerance_pct(o), parse_tolerance_pct(c))
        && c > o + 1e-12
    {
        return false;
    }
    if let (Some(o), Some(c)) = (
        orig.attr("Temperature Coefficient"),
        cand.attr("Temperature Coefficient"),
    ) && let (Some(o), Some(c)) = (dielectric_rank(o), dielectric_rank(c))
        && c < o
    {
        return false;
    }
    true
}

/// Explain why `cand` is an improvement over `orig`; empty = no improvement.
pub fn improvement_reasons(
    orig: &ComponentSummary,
    cand: &ComponentSummary,
) -> (Vec<String>, Option<f64>) {
    let mut reasons = Vec::new();
    if cand.library_type.rank() < orig.library_type.rank() {
        reasons.push(match cand.library_type {
            LibraryType::Basic => "Basic part (avoids the extended-part setup fee)".to_string(),
            LibraryType::Preferred => {
                "Preferred Extended (setup fee waived for Economic PCBA)".to_string()
            }
            _ => "better library class".to_string(),
        });
    }
    let pct = match (orig.unit_price_usd, cand.unit_price_usd) {
        (Some(o), Some(c)) if o > 0.0 => Some(((c - o) / o * 1000.0).round() / 10.0),
        _ => None,
    };
    if let Some(p) = pct.filter(|p| *p <= -5.0) {
        reasons.push(format!("{:.1}% cheaper at qty {}", -p, cand.price_qty));
    }
    if orig.jlc_stock <= 0 && cand.jlc_stock > 0 {
        reasons.push(format!(
            "in stock ({}) while original is out of stock",
            cand.jlc_stock
        ));
    } else if orig.jlc_stock > 0 && cand.jlc_stock >= orig.jlc_stock.saturating_mul(2) {
        reasons.push(format!(
            "{:.1}x the stock ({} vs {})",
            cand.jlc_stock as f64 / orig.jlc_stock as f64,
            cand.jlc_stock,
            orig.jlc_stock
        ));
    }
    (reasons, pct)
}

fn mpn_stem(mpn: &str) -> Option<String> {
    let stem: String = mpn
        .split(['-', '/', ' ', '_'])
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    (stem.len() >= 4).then_some(stem)
}

impl Service {
    /// Build from configuration.
    pub fn new(config: Config) -> Self {
        let http = Arc::new(HttpClient::new(config.http.clone()));
        let official = config
            .credentials
            .map(|c| OfficialClient::new(http.clone(), config.official_base.clone(), c));
        Self {
            jlc: JlcClient::new(http.clone(), config.jlc_base),
            lcsc: LcscClient::new(http.clone(), config.lcsc_base),
            official,
            http,
        }
    }

    /// Whether official API credentials are configured.
    pub fn official_configured(&self) -> bool {
        self.official.is_some()
    }

    /// `search_components`.
    pub async fn search(&self, args: SearchArgs) -> Result<SearchResult> {
        let query = args
            .query
            .as_deref()
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .map(str::to_string);
        let packages = split_packages(args.package.as_deref());
        if query.is_none() && args.category.as_deref().is_none_or(|c| c.trim().is_empty()) {
            return Err(ApiError::InvalidInput(
                "provide `query` and/or `category`".into(),
            ));
        }
        let max = args
            .max_results
            .unwrap_or(DEFAULT_MAX_RESULTS)
            .clamp(1, MAX_RESULTS_CAP);
        let qty = args.quantity.unwrap_or(DEFAULT_PRICE_QTY).max(1);
        let min_stock = args.min_stock.unwrap_or(1);
        let basic_only = args.basic_only.unwrap_or(false);
        let include_preferred = args.include_preferred.unwrap_or(false);

        let (parent, leaf) = match args.category.as_deref().filter(|c| !c.trim().is_empty()) {
            Some(c) => {
                let tree = self.jlc.categories().await?;
                let (p, l) = resolve_category(&tree, c)?;
                (Some(p), l)
            }
            None => (None, None),
        };

        let base = SearchQuery {
            keyword: query.clone(),
            parent_category: parent.clone(),
            leaf_category: leaf.clone(),
            packages: packages.clone(),
            attributes: vec![],
            library: LibraryFilter::Any,
            in_stock_only: min_stock > 0,
            min_stock: (min_stock > 1).then_some(min_stock),
            page_size: (max * 3).clamp(30, jlc::MAX_PAGE_SIZE),
            sort: None,
        };

        let mut notes = Vec::new();
        let library_label;
        let (total, groups) = if basic_only {
            let q = SearchQuery {
                library: if include_preferred {
                    LibraryFilter::BasicOrPreferred
                } else {
                    LibraryFilter::Basic
                },
                ..base.clone()
            };
            library_label = if include_preferred {
                "basic_or_preferred"
            } else {
                "basic"
            };
            let page = self.jlc.search(&q).await?;
            let rows = rows_to_summaries(page.rows, qty);
            (page.total, vec![rows])
        } else {
            library_label = "any";
            // Main relevance query + a dedicated Basic/Preferred query so Basic
            // parts surface even when they are not on the first relevance page.
            let basic_q = SearchQuery {
                library: LibraryFilter::BasicOrPreferred,
                page_size: 30,
                ..base.clone()
            };
            let (main, basic) = tokio::join!(self.jlc.search(&base), self.jlc.search(&basic_q));
            match (main, basic) {
                (Ok(m), Ok(b)) => (
                    m.total,
                    vec![
                        rows_to_summaries(b.rows, qty),
                        rows_to_summaries(m.rows, qty),
                    ],
                ),
                (Ok(m), Err(e)) => {
                    notes.push(format!(
                        "Basic-part query failed ({e}); ranking uses the main query only."
                    ));
                    (m.total, vec![rows_to_summaries(m.rows, qty)])
                }
                (Err(e), Ok(b)) => {
                    notes.push(format!(
                        "Main query failed ({e}); showing Basic/Preferred matches only."
                    ));
                    (b.total, vec![rows_to_summaries(b.rows, qty)])
                }
                (Err(e), Err(_)) => return Err(e),
            }
        };

        let results = merge_and_rank(groups, min_stock, max as usize);
        if results.is_empty() {
            if !packages.is_empty() {
                notes.push("No results. Package names must match JLCPCB exactly (e.g. \"SOT-23-3\", \"SOIC-8\", \"0402\"); try without `package` and read the package field of the results.".into());
            }
            if min_stock > 0 {
                notes.push("Only in-stock parts were searched; set min_stock=0 to include out-of-stock parts.".into());
            }
        }
        notes.push("jlc_stock is JLCPCB assembly stock. Prices are JLCPCB component prices in USD and exclude assembly/setup fees.".into());

        Ok(SearchResult {
            filters: AppliedFilters {
                query,
                category: parent,
                subcategory: leaf,
                packages,
                library: library_label.into(),
                min_stock,
                price_qty: qty,
            },
            total_matches: total,
            returned: results.len(),
            results,
            notes,
        })
    }

    /// `get_component_details`.
    pub async fn details(&self, args: DetailsArgs) -> Result<ComponentDetails> {
        let code = normalize_lcsc(&args.lcsc).ok_or_else(|| {
            ApiError::InvalidInput(format!(
                "{:?} is not an LCSC part number (expected e.g. \"C25744\"); use search_components to find one",
                args.lcsc
            ))
        })?;
        let qty = args.quantity.unwrap_or(DEFAULT_PRICE_QTY).max(1);

        let official_fut = async {
            match &self.official {
                Some(o) => Some(o.component_detail(&code).await),
                None => None,
            }
        };
        let (jlc_detail, jlc_row, lcsc_detail, official) = tokio::join!(
            self.jlc.detail(&code),
            self.jlc.search_row(&code),
            self.lcsc.detail(&code),
            official_fut
        );

        let mut sources = Vec::new();
        let mut warnings = Vec::new();

        let jlc = match jlc_detail {
            Ok(Some(rec)) => rec.into_detail(qty),
            Ok(None) => None,
            Err(e) => {
                warnings.push(format!("JLCPCB detail unavailable: {e}"));
                None
            }
        };
        let row = match jlc_row {
            Ok(r) => r,
            Err(e) => {
                warnings.push(format!("JLCPCB search lookup unavailable: {e}"));
                None
            }
        };
        let lcsc = match lcsc_detail {
            Ok(d) => d,
            Err(e) => {
                warnings.push(format!("LCSC retail data unavailable: {e}"));
                None
            }
        };
        let official_api = match official {
            Some(Ok(v)) => {
                if v.is_some() {
                    sources.push("JLCPCB official API".into());
                }
                v
            }
            Some(Err(e)) => {
                warnings.push(format!("Official API call failed: {e}"));
                None
            }
            None => None,
        };

        if jlc.is_none() && row.is_none() && lcsc.is_none() && official_api.is_none() {
            if warnings.is_empty() {
                return Err(ApiError::NotFound(format!(
                    "part {code} was not found at JLCPCB or LCSC"
                )));
            }
            return Err(ApiError::NotFound(format!(
                "could not load part {code}: {}",
                warnings.join("; ")
            )));
        }

        // Prefer the exact JLCPCB detail; fall back to the search row.
        let mut jlc_summary_extra = None;
        let summary = match jlc {
            Some(mut d) => {
                d.apply_preferred(row.as_ref().and_then(|r| r.preferred()));
                sources.push("JLCPCB parts detail".into());
                let s = d.summary.clone();
                jlc_summary_extra = Some(d);
                Some(s)
            }
            None => row.and_then(|r| {
                sources.push("JLCPCB parts search".into());
                r.into_summary(qty)
            }),
        };
        if lcsc.is_some() {
            sources.push("LCSC product detail".into());
        }
        if summary.is_none() {
            warnings.push("Part is not in the JLCPCB assembly library (LCSC retail data only); it may not be available for PCBA.".into());
        }

        let s = summary.as_ref();
        let l = lcsc.as_ref();
        let pick = |a: Option<String>, b: Option<String>| a.or(b);
        let library_type = s.map(|s| s.library_type).unwrap_or(LibraryType::Unknown);
        let attributes = match s {
            Some(s) if !s.attributes.is_empty() => s.attributes.clone(),
            _ => l.map(|l| l.attributes.clone()).unwrap_or_default(),
        };
        let ex = jlc_summary_extra.as_ref();
        Ok(ComponentDetails {
            lcsc: code.clone(),
            mfr_part: pick(
                s.and_then(|s| s.mfr_part.clone()),
                l.and_then(|l| l.mfr_part.clone()),
            ),
            manufacturer: pick(
                s.and_then(|s| s.manufacturer.clone()),
                l.and_then(|l| l.manufacturer.clone()),
            ),
            package: pick(
                s.and_then(|s| s.package.clone()),
                l.and_then(|l| l.package.clone()),
            ),
            description: pick(
                s.and_then(|s| s.description.clone()),
                l.and_then(|l| l.description.clone()),
            ),
            category: pick(
                s.and_then(|s| s.category.clone()),
                l.and_then(|l| l.category.clone()),
            ),
            subcategory: pick(
                s.and_then(|s| s.subcategory.clone()),
                l.and_then(|l| l.subcategory.clone()),
            ),
            library_type,
            library_note: library_note(library_type),
            jlc_assembly_stock: s.map(|s| s.jlc_stock),
            lcsc_retail_stock: l.and_then(|l| l.retail_stock),
            price_qty: qty,
            jlc_unit_price_usd: s.and_then(|s| unit_price_at(&s.price_breaks, qty)),
            jlc_price_breaks: s.map(|s| s.price_breaks.clone()).unwrap_or_default(),
            lcsc_price_breaks: l.map(|l| l.price_breaks.clone()).unwrap_or_default(),
            attributes,
            datasheet_url: pick(
                s.and_then(|s| s.datasheet_url.clone()),
                l.and_then(|l| l.datasheet_url.clone()),
            ),
            image_url: pick(
                ex.and_then(|e| e.image_url.clone()),
                l.and_then(|l| l.image_url.clone()),
            ),
            jlcpcb_url: s
                .map(|s| s.jlcpcb_url.clone())
                .unwrap_or_else(|| jlc::part_url(&code)),
            lcsc_url: ex
                .and_then(|e| e.lcsc_url.clone())
                .or_else(|| Some(format!("https://www.lcsc.com/product-detail/{code}.html"))),
            assembly_process: ex.and_then(|e| e.assembly_process.clone()),
            moisture_sensitivity_level: ex.and_then(|e| e.moisture_sensitivity_level.clone()),
            min_order_qty: ex.and_then(|e| e.min_order_qty),
            reel_qty: ex.and_then(|e| e.reel_qty),
            buyable: ex.map(|e| e.buyable && e.listed),
            not_buyable_reason: ex.and_then(|e| e.not_buyable_reason.clone()),
            jlc_suggested_replacement: s.and_then(|s| s.jlc_suggested_replacement.clone()),
            eccn: ex.and_then(|e| e.eccn.clone()),
            needs_xray: ex.and_then(|e| e.needs_xray),
            official_api,
            sources,
            warnings,
        })
    }

    /// `suggest_alternatives`.
    pub async fn alternatives(&self, args: AlternativesArgs) -> Result<AlternativesResult> {
        let code = normalize_lcsc(&args.lcsc).ok_or_else(|| {
            ApiError::InvalidInput(format!("{:?} is not an LCSC part number", args.lcsc))
        })?;
        let qty = args.quantity.unwrap_or(DEFAULT_PRICE_QTY).max(1);
        let max = args.max_results.unwrap_or(8).clamp(1, 25) as usize;
        let same_package = args.same_package.unwrap_or(true);
        let min_stock = args.min_stock.unwrap_or(100).max(1) as i64;

        let (detail, row) = tokio::join!(self.jlc.detail(&code), self.jlc.search_row(&code));
        let row = row.ok().flatten();
        let preferred = row.as_ref().and_then(|r| r.preferred());
        let original = match detail? {
            Some(rec) => {
                let mut d = rec.into_detail(qty).ok_or_else(|| {
                    ApiError::NotFound(format!("part {code} not found at JLCPCB"))
                })?;
                d.apply_preferred(preferred);
                d.summary
            }
            None => row.and_then(|r| r.into_summary(qty)).ok_or_else(|| {
                ApiError::NotFound(format!(
                    "part {code} not found in the JLCPCB assembly library"
                ))
            })?,
        };

        let mut criteria = Vec::new();
        let mut notes = Vec::new();
        let value_attrs: Vec<(String, Vec<String>)> = VALUE_ATTRS
            .iter()
            .filter_map(|n| {
                original
                    .attr(n)
                    .map(|v| (n.to_string(), vec![v.to_string()]))
            })
            .collect();

        let mut q = SearchQuery {
            parent_category: original.category.clone(),
            leaf_category: original.subcategory.clone(),
            packages: if same_package {
                original.package.clone().into_iter().collect()
            } else {
                vec![]
            },
            in_stock_only: true,
            page_size: 60,
            ..Default::default()
        };
        if let (Some(c), Some(sc)) = (&original.category, &original.subcategory) {
            criteria.push(format!("same category: {c} / {sc}"));
        }
        if same_package && let Some(p) = &original.package {
            criteria.push(format!("same package: {p}"));
        }
        if !value_attrs.is_empty() {
            for (n, v) in &value_attrs {
                criteria.push(format!("same {n}: {}", v.join("/")));
            }
            q.attributes = value_attrs;
        } else if let Some(stem) = original.mfr_part.as_deref().and_then(mpn_stem) {
            criteria.push(format!("manufacturer part number family: {stem}*"));
            notes.push("No standard value attribute for this category; matched by part-number family. Verify pinout and electrical specs before substituting.".into());
            q.keyword = Some(stem);
        } else {
            notes.push("Matched by category/package only; candidates may differ functionally. Verify before substituting.".into());
        }
        criteria.push("at least the same voltage/power/current rating, no worse tolerance or dielectric (where parseable)".into());

        let basic_q = SearchQuery {
            library: LibraryFilter::BasicOrPreferred,
            page_size: 30,
            ..q.clone()
        };
        let only_bp = args.basic_or_preferred_only.unwrap_or(false);
        let mut groups = Vec::new();
        if only_bp {
            groups.push(self.jlc.search(&basic_q).await?.rows);
        } else {
            let (a, b) = tokio::join!(self.jlc.search(&q), self.jlc.search(&basic_q));
            match (a, b) {
                (Err(e), Err(_)) => return Err(e),
                (a, b) => {
                    if let Ok(b) = b {
                        groups.push(b.rows);
                    }
                    if let Ok(a) = a {
                        groups.push(a.rows);
                    }
                }
            }
        }

        let mut seen = HashSet::new();
        seen.insert(original.lcsc.clone());
        let mut alts: Vec<Alternative> = groups
            .into_iter()
            .flatten()
            .filter_map(|r| r.into_summary(qty))
            .filter(|c| seen.insert(c.lcsc.clone()))
            .filter(|c| c.jlc_stock >= min_stock)
            .filter(|c| meets_constraints(&original, c))
            .filter_map(|c| {
                let (reasons, pct) = improvement_reasons(&original, &c);
                (!reasons.is_empty()).then_some(Alternative {
                    component: c,
                    reasons,
                    price_change_pct: pct,
                })
            })
            .collect();
        alts.sort_by(|a, b| {
            a.component
                .library_type
                .rank()
                .cmp(&b.component.library_type.rank())
                .then_with(|| {
                    crate::model::cmp_price(a.component.unit_price_usd, b.component.unit_price_usd)
                })
                .then_with(|| b.component.jlc_stock.cmp(&a.component.jlc_stock))
        });
        alts.truncate(max);

        if let Some(r) = &original.jlc_suggested_replacement {
            notes.push(format!(
                "JLCPCB lists {r} as the official replacement for this part."
            ));
        }
        if alts.is_empty() {
            notes.push("No better-stocked, cheaper or Basic-library part matched the criteria. Try same_package=false.".into());
        }
        notes.push("Always confirm the datasheet before substituting a part.".into());

        Ok(AlternativesResult {
            original,
            match_criteria: criteria,
            alternatives: alts,
            notes,
        })
    }

    /// `list_categories`.
    pub async fn categories(&self, args: CategoriesArgs) -> Result<CategoriesResult> {
        let tree = self.jlc.categories().await?;
        let search = args
            .search
            .as_deref()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty());
        let parent = args
            .parent
            .as_deref()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty());
        let with_children = args
            .include_subcategories
            .unwrap_or(parent.is_some() || search.is_some());

        let mut out = Vec::new();
        for p in &tree {
            if let Some(want) = &parent
                && !p.name.to_lowercase().contains(want.as_str())
            {
                continue;
            }
            let parent_hit = search
                .as_ref()
                .is_none_or(|s| p.name.to_lowercase().contains(s.as_str()));
            let subs: Vec<SubcategoryOut> = p
                .children
                .iter()
                .filter(|c| {
                    parent_hit
                        || search
                            .as_ref()
                            .is_none_or(|s| c.name.to_lowercase().contains(s.as_str()))
                })
                .map(|c| SubcategoryOut {
                    name: c.name.clone(),
                    id: c.id,
                    part_count: c.count,
                })
                .collect();
            if !parent_hit && subs.is_empty() {
                continue;
            }
            out.push(CategoryOut {
                name: p.name.clone(),
                id: p.id,
                part_count: p.count,
                subcategory_count: p.children.len(),
                subcategories: if with_children { subs } else { vec![] },
            });
        }
        if out.is_empty() && (parent.is_some() || search.is_some()) {
            return Err(ApiError::NotFound(
                "no categories match; call list_categories with no arguments to see all top-level categories".into(),
            ));
        }
        Ok(CategoriesResult {
            total_categories: out.len(),
            categories: out,
        })
    }

    /// `service_status`.
    pub async fn status(&self) -> StatusResult {
        let t = std::time::Instant::now();
        let jlc = match self.jlc.ping().await {
            Ok(()) => EndpointStatus {
                ok: true,
                latency_ms: Some(t.elapsed().as_millis()),
                detail: "reachable (live search probe)".into(),
            },
            Err(e) => EndpointStatus {
                ok: false,
                latency_ms: None,
                detail: e.to_string(),
            },
        };
        let t = std::time::Instant::now();
        let lcsc = match self.lcsc.ping().await {
            Ok(()) => EndpointStatus {
                ok: true,
                latency_ms: Some(t.elapsed().as_millis()),
                detail: "reachable".into(),
            },
            Err(e) => EndpointStatus {
                ok: false,
                latency_ms: None,
                detail: e.to_string(),
            },
        };
        StatusResult {
            version: env!("CARGO_PKG_VERSION").into(),
            jlcpcb_parts_api: jlc,
            lcsc_api: lcsc,
            official_api_configured: self.official.is_some(),
            cached_responses: self.http.cache_len().await,
            notes: vec![
                "No local catalog is used: every search is live against JLCPCB's public parts endpoints, with a 5-minute in-memory cache (24 h for categories).".into(),
                "Set JLCPCB_APP_ID, JLCPCB_API_KEY and JLCPCB_API_SECRET to also query the official JLCPCB API in get_component_details.".into(),
            ],
        }
    }
}

fn rows_to_summaries(rows: Vec<jlc::SearchRow>, qty: u64) -> Vec<ComponentSummary> {
    rows.into_iter()
        .filter_map(|r| r.into_summary(qty))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, children: &[&str]) -> CategoryNode {
        CategoryNode {
            name: name.into(),
            id: 1,
            count: 10,
            children: children
                .iter()
                .map(|c| CategoryNode {
                    name: (*c).into(),
                    id: 2,
                    count: 5,
                    children: vec![],
                })
                .collect(),
        }
    }

    fn tree() -> Vec<CategoryNode> {
        vec![
            node(
                "Capacitors",
                &[
                    "Multilayer Ceramic Capacitors MLCC - SMD/SMT",
                    "Multilayer Ceramic Capacitors MLCC - Leaded",
                    "Tantalum Capacitors",
                ],
            ),
            node("Resistors", &["Chip Resistor - Surface Mount"]),
        ]
    }

    #[test]
    fn resolves_parent_leaf_plural_and_substring() {
        let t = tree();
        assert_eq!(
            resolve_category(&t, "resistors").unwrap(),
            ("Resistors".into(), None)
        );
        assert_eq!(
            resolve_category(&t, "resistor").unwrap(),
            ("Resistors".into(), None)
        );
        assert_eq!(
            resolve_category(&t, "tantalum").unwrap(),
            ("Capacitors".into(), Some("Tantalum Capacitors".into()))
        );
        assert_eq!(
            resolve_category(&t, "Chip Resistor - Surface Mount").unwrap(),
            (
                "Resistors".into(),
                Some("Chip Resistor - Surface Mount".into())
            )
        );
        assert!(matches!(
            resolve_category(&t, "mlcc"),
            Err(ApiError::InvalidInput(m)) if m.contains("ambiguous")
        ));
        assert!(matches!(
            resolve_category(&t, "flux capacitor drive"),
            Err(ApiError::NotFound(_))
        ));
    }

    #[test]
    fn mpn_stem_extraction() {
        assert_eq!(mpn_stem("AMS1117-3.3").as_deref(), Some("AMS1117"));
        assert_eq!(mpn_stem("LM1/2"), None);
    }

    #[test]
    fn package_splitting() {
        assert_eq!(
            split_packages(Some("SOT-23-3, SOT-23 ,")),
            vec!["SOT-23-3".to_string(), "SOT-23".to_string()]
        );
        assert!(split_packages(None).is_empty());
    }
}

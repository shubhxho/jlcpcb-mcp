//! Client for the public (unauthenticated) JLCPCB parts endpoints used by
//! <https://jlcpcb.com/parts>. These are not a documented, versioned API and
//! may change without notice; every decode is defensive.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{ApiError, Result};
use crate::http::HttpClient;
use crate::model::{
    Attribute, ComponentSummary, LibraryType, PriceBreak, clean, de_opt_f64, de_opt_i64,
    sort_breaks, unit_price_at,
};

/// Service name used in error messages.
pub const SERVICE: &str = "JLCPCB parts API";
/// Default API root.
pub const DEFAULT_BASE: &str = "https://jlcpcb.com/api";

const SEARCH_PATH: &str = "/overseas-pcb-order/v1/shoppingCart/smtGood/selectSmtComponentList/v2";
const DETAIL_PATH: &str = "/overseas-pcb-order/v1/smtComponentShopCart/getComponentDetail";

const SEARCH_TTL: Duration = Duration::from_secs(5 * 60);
const DETAIL_TTL: Duration = Duration::from_secs(5 * 60);
const CATEGORY_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Maximum page size we ever request.
pub const MAX_PAGE_SIZE: u32 = 100;

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WirePrice {
    #[serde(default, deserialize_with = "de_opt_i64")]
    start_number: Option<i64>,
    #[serde(default, deserialize_with = "de_opt_i64")]
    end_number: Option<i64>,
    #[serde(default, deserialize_with = "de_opt_f64")]
    product_price: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
struct WireAttr {
    #[serde(default)]
    attribute_name_en: Option<String>,
    #[serde(default)]
    attribute_value_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireImage {
    #[serde(default)]
    product_big_image: Option<String>,
}

/// One row of a search response. NB: in search rows `firstSortName` is the
/// *leaf* category and `secondSortName` the *parent*.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRow {
    #[serde(default)]
    component_code: Option<String>,
    #[serde(default)]
    component_model_en: Option<String>,
    #[serde(default)]
    component_brand_en: Option<String>,
    #[serde(default)]
    component_specification_en: Option<String>,
    #[serde(default)]
    describe: Option<String>,
    #[serde(default)]
    component_library_type: Option<String>,
    #[serde(default)]
    preferred_component_flag: Option<bool>,
    #[serde(default, deserialize_with = "de_opt_i64")]
    stock_count: Option<i64>,
    #[serde(default)]
    component_prices: Option<Vec<WirePrice>>,
    #[serde(default)]
    first_sort_name: Option<String>,
    #[serde(default)]
    second_sort_name: Option<String>,
    #[serde(default)]
    component_type_en: Option<String>,
    #[serde(default)]
    data_manual_url: Option<String>,
    #[serde(default)]
    url_suffix: Option<String>,
    #[serde(default)]
    attributes: Option<Vec<WireAttr>>,
    #[serde(default)]
    component_alternatives_code: Option<String>,
}

/// The exact-detail record (`getComponentDetail`). Here `firstSortName` is
/// the *parent* category.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailRecord {
    #[serde(default)]
    pub component_code: Option<String>,
    #[serde(default)]
    component_model_en: Option<String>,
    #[serde(default)]
    component_brand_en: Option<String>,
    #[serde(default)]
    component_specification_en: Option<String>,
    #[serde(default)]
    describe: Option<String>,
    #[serde(default)]
    component_library_type: Option<String>,
    #[serde(default, deserialize_with = "de_opt_i64")]
    stock_count: Option<i64>,
    #[serde(default)]
    prices: Option<Vec<WirePrice>>,
    #[serde(default)]
    first_sort_name: Option<String>,
    #[serde(default)]
    second_sort_name: Option<String>,
    #[serde(default)]
    first_type_name_en: Option<String>,
    #[serde(default)]
    second_type_name_en: Option<String>,
    #[serde(default)]
    data_manual_url: Option<String>,
    #[serde(default)]
    component_image_url: Option<String>,
    #[serde(default)]
    image_list: Option<Vec<WireImage>>,
    #[serde(default)]
    lcsc_goods_url: Option<String>,
    #[serde(default)]
    attributes: Option<Vec<WireAttr>>,
    #[serde(default)]
    assembly_process: Option<String>,
    #[serde(default)]
    moisture_sensitivity_level_en: Option<String>,
    #[serde(default, deserialize_with = "de_opt_i64")]
    min_purchase_num: Option<i64>,
    #[serde(default, deserialize_with = "de_opt_i64")]
    encapsulation_number: Option<i64>,
    #[serde(default)]
    is_buy_component: Option<String>,
    #[serde(default)]
    no_buy_reason: Option<String>,
    #[serde(default)]
    component_alternatives_code: Option<String>,
    #[serde(default)]
    component_status: Option<String>,
    #[serde(default)]
    eccn_code: Option<String>,
    #[serde(default)]
    xray_flag: Option<bool>,
}

/// One node in the category tree.
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct CategoryNode {
    /// Category name (use as the `category` filter of `search_components`).
    #[serde(rename(deserialize = "sortName"))]
    pub name: String,
    /// Numeric JLCPCB category id.
    #[serde(rename(deserialize = "componentSortKeyId"), default)]
    pub id: i64,
    /// Number of parts in the category.
    #[serde(rename(deserialize = "componentCount"), default)]
    pub count: i64,
    /// Sub-categories (leaves).
    #[serde(
        rename(deserialize = "childSortList"),
        default,
        deserialize_with = "de_null_vec"
    )]
    pub children: Vec<CategoryNode>,
}

fn de_null_vec<'de, D, T>(d: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
}

// ---------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------

fn convert_prices(raw: Option<Vec<WirePrice>>) -> Vec<PriceBreak> {
    let mut out: Vec<PriceBreak> = raw
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| {
            let price = p.product_price?;
            if !(price.is_finite() && price > 0.0) {
                return None;
            }
            Some(PriceBreak {
                min_qty: p.start_number.unwrap_or(1).max(1) as u64,
                max_qty: p.end_number.filter(|n| *n > 0).map(|n| n as u64),
                unit_price_usd: price,
            })
        })
        .collect();
    sort_breaks(&mut out);
    out.dedup_by_key(|b| b.min_qty);
    out
}

fn convert_attrs(raw: Option<Vec<WireAttr>>) -> Vec<Attribute> {
    raw.unwrap_or_default()
        .into_iter()
        .filter_map(|a| {
            let name = clean(a.attribute_name_en)?;
            let value = clean(a.attribute_value_name)?;
            Some(Attribute { name, value })
        })
        .collect()
}

/// Public JLCPCB part page.
pub fn part_url(code: &str) -> String {
    format!("https://jlcpcb.com/partdetail/{code}")
}

impl SearchRow {
    /// Convert into the normalised summary, pricing at `qty`.
    pub fn into_summary(self, qty: u64) -> Option<ComponentSummary> {
        let lcsc = clean(self.component_code)?;
        let price_breaks = convert_prices(self.component_prices);
        let unit_price_usd = unit_price_at(&price_breaks, qty);
        let jlcpcb_url = match clean(self.url_suffix) {
            Some(suffix) => format!("https://jlcpcb.com/partdetail/{suffix}"),
            None => part_url(&lcsc),
        };
        Some(ComponentSummary {
            library_type: LibraryType::from_raw(
                self.component_library_type.as_deref(),
                self.preferred_component_flag,
            ),
            jlc_stock: self.stock_count.unwrap_or(0),
            mfr_part: clean(self.component_model_en),
            manufacturer: clean(self.component_brand_en),
            package: clean(self.component_specification_en),
            description: clean(self.describe),
            category: clean(self.second_sort_name),
            subcategory: clean(self.first_sort_name).or(clean(self.component_type_en)),
            price_qty: qty,
            unit_price_usd,
            price_breaks,
            attributes: convert_attrs(self.attributes),
            datasheet_url: clean(self.data_manual_url),
            jlcpcb_url,
            jlc_suggested_replacement: clean(self.component_alternatives_code),
            lcsc,
        })
    }

    /// Raw LCSC code of the row.
    pub fn code(&self) -> Option<&str> {
        self.component_code.as_deref()
    }

    /// Raw preferred flag.
    pub fn preferred(&self) -> Option<bool> {
        self.preferred_component_flag
    }
}

/// Normalised view of a [`DetailRecord`].
#[derive(Debug, Clone)]
pub struct JlcDetail {
    pub summary: ComponentSummary,
    pub image_url: Option<String>,
    pub lcsc_url: Option<String>,
    pub assembly_process: Option<String>,
    pub moisture_sensitivity_level: Option<String>,
    pub min_order_qty: Option<i64>,
    pub reel_qty: Option<i64>,
    pub buyable: bool,
    pub not_buyable_reason: Option<String>,
    pub listed: bool,
    pub eccn: Option<String>,
    pub needs_xray: Option<bool>,
    raw_library_type: Option<String>,
}

impl JlcDetail {
    /// Fill in the Preferred flag (the detail endpoint doesn't report it).
    pub fn apply_preferred(&mut self, preferred: Option<bool>) {
        self.summary.library_type =
            LibraryType::from_raw(self.raw_library_type.as_deref(), preferred);
    }
}

impl DetailRecord {
    /// Convert; returns `None` when the record is the all-null "not found" shape.
    pub fn into_detail(self, qty: u64) -> Option<JlcDetail> {
        let lcsc = clean(self.component_code)?;
        let price_breaks = convert_prices(self.prices);
        let unit_price_usd = unit_price_at(&price_breaks, qty);
        let image_url = self
            .image_list
            .unwrap_or_default()
            .into_iter()
            .find_map(|i| clean(i.product_big_image))
            .or(clean(self.component_image_url));
        let raw_library_type = self.component_library_type.clone();
        let summary = ComponentSummary {
            library_type: LibraryType::from_raw(self.component_library_type.as_deref(), None),
            jlc_stock: self.stock_count.unwrap_or(0),
            mfr_part: clean(self.component_model_en),
            manufacturer: clean(self.component_brand_en),
            package: clean(self.component_specification_en),
            description: clean(self.describe),
            category: clean(self.first_sort_name).or(clean(self.first_type_name_en)),
            subcategory: clean(self.second_sort_name).or(clean(self.second_type_name_en)),
            price_qty: qty,
            unit_price_usd,
            price_breaks,
            attributes: convert_attrs(self.attributes),
            datasheet_url: clean(self.data_manual_url),
            jlcpcb_url: part_url(&lcsc),
            jlc_suggested_replacement: clean(self.component_alternatives_code),
            lcsc,
        };
        Some(JlcDetail {
            summary,
            image_url,
            lcsc_url: clean(self.lcsc_goods_url),
            assembly_process: clean(self.assembly_process),
            moisture_sensitivity_level: clean(self.moisture_sensitivity_level_en),
            min_order_qty: self.min_purchase_num,
            reel_qty: self.encapsulation_number,
            buyable: self.is_buy_component.as_deref() != Some("0"),
            not_buyable_reason: clean(self.no_buy_reason),
            listed: self.component_status.as_deref() != Some("no"),
            eccn: clean(self.eccn_code),
            needs_xray: self.xray_flag,
            raw_library_type,
        })
    }
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

/// Library filter for a search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LibraryFilter {
    /// No filter.
    #[default]
    Any,
    /// Basic parts only.
    Basic,
    /// Basic or Preferred Extended.
    BasicOrPreferred,
}

/// Server-side sort.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    StockDesc,
    PriceAsc,
}

/// A parts search.
#[derive(Debug, Clone, Default)]
pub struct SearchQuery {
    pub keyword: Option<String>,
    /// Parent category name.
    pub parent_category: Option<String>,
    /// Leaf category name (requires `parent_category`).
    pub leaf_category: Option<String>,
    pub packages: Vec<String>,
    /// Exact attribute matches: (name, accepted values).
    pub attributes: Vec<(String, Vec<String>)>,
    pub library: LibraryFilter,
    pub in_stock_only: bool,
    pub min_stock: Option<u64>,
    pub page_size: u32,
    pub sort: Option<SortMode>,
}

impl SearchQuery {
    /// Build the wire request body.
    pub fn to_body(&self) -> Value {
        let mut body = json!({
            "currentPage": 1,
            "pageSize": self.page_size.clamp(1, MAX_PAGE_SIZE),
            "keyword": self.keyword.as_deref().map(str::trim).filter(|k| !k.is_empty()),
            "searchType": 2,
            "searchSource": "search",
            "componentLibraryType": Value::Null,
            "stockFlag": self.in_stock_only,
            "firstSortName": self.parent_category,
            "secondSortName": self.leaf_category,
            "componentBrandList": [],
            "componentSpecificationList": self.packages,
            "componentAttributeList": self
                .attributes
                .iter()
                .map(|(name, values)| json!({ name.clone(): values }))
                .collect::<Vec<_>>(),
            "paramList": [],
        });
        let obj = body.as_object_mut().expect("object");
        match self.library {
            LibraryFilter::Any => {}
            LibraryFilter::Basic => {
                obj.insert("componentLibTypes".into(), json!(["base"]));
            }
            LibraryFilter::BasicOrPreferred => {
                obj.insert("componentLibTypes".into(), json!(["base"]));
                obj.insert("preferredComponentFlag".into(), json!(true));
            }
        }
        if let Some(min) = self.min_stock.filter(|m| *m > 0) {
            obj.insert("startStockNumber".into(), json!(min));
        }
        match self.sort {
            Some(SortMode::StockDesc) => {
                obj.insert("sortMode".into(), json!("STOCK_SORT"));
                obj.insert("sortASC".into(), json!("DESC"));
            }
            Some(SortMode::PriceAsc) => {
                obj.insert("sortMode".into(), json!("PRICE_SORT"));
                obj.insert("sortASC".into(), json!("ASC"));
            }
            None => {}
        }
        body
    }
}

/// A page of search results.
#[derive(Debug, Clone)]
pub struct SearchPage {
    pub total: i64,
    pub rows: Vec<SearchRow>,
}

/// Unwrap the `{code, data, message}` envelope.
pub fn unwrap_envelope(service: &'static str, v: Value) -> Result<Value> {
    let code = v.get("code").and_then(Value::as_i64).unwrap_or(-1);
    if code != 200 {
        let message = v
            .get("message")
            .or_else(|| v.get("msg"))
            .and_then(Value::as_str)
            .unwrap_or("unknown error")
            .to_string();
        let message = if code == 101 {
            format!("{message} (the server could not process this query; try simpler filters)")
        } else {
            message
        };
        return Err(ApiError::Api {
            service,
            code,
            message,
        });
    }
    match v.get("data") {
        Some(Value::Null) | None => Ok(Value::Null),
        Some(d) => Ok(d.clone()),
    }
}

/// Parse a search response document.
pub fn parse_search(v: Value) -> Result<SearchPage> {
    let data = unwrap_envelope(SERVICE, v)?;
    let info = data
        .get("componentPageInfo")
        .cloned()
        .unwrap_or(Value::Null);
    let total = info.get("total").and_then(Value::as_i64).unwrap_or(0);
    let rows = match info.get("list") {
        Some(Value::Array(_)) => {
            serde_json::from_value(info["list"].clone()).map_err(|e| ApiError::Decode {
                service: SERVICE,
                message: e.to_string(),
            })?
        }
        _ => Vec::new(),
    };
    Ok(SearchPage { total, rows })
}

/// Parse a category-tree response document.
pub fn parse_categories(v: Value) -> Result<Vec<CategoryNode>> {
    let data = unwrap_envelope(SERVICE, v)?;
    match data.get("sortAndCountVoList") {
        Some(list @ Value::Array(_)) => {
            serde_json::from_value(list.clone()).map_err(|e| ApiError::Decode {
                service: SERVICE,
                message: e.to_string(),
            })
        }
        _ => Err(ApiError::Decode {
            service: SERVICE,
            message: "category tree missing from response".into(),
        }),
    }
}

/// Parse a detail response; `Ok(None)` when the part is unknown.
pub fn parse_detail(v: Value) -> Result<Option<DetailRecord>> {
    let data = unwrap_envelope(SERVICE, v)?;
    if data.is_null() {
        return Ok(None);
    }
    let rec: DetailRecord = serde_json::from_value(data).map_err(|e| ApiError::Decode {
        service: SERVICE,
        message: e.to_string(),
    })?;
    Ok(rec.component_code.is_some().then_some(rec))
}

/// Client for the public JLCPCB endpoints.
#[derive(Clone)]
pub struct JlcClient {
    http: Arc<HttpClient>,
    base: String,
}

impl JlcClient {
    pub fn new(http: Arc<HttpClient>, base: impl Into<String>) -> Self {
        Self {
            http,
            base: base.into().trim_end_matches('/').to_string(),
        }
    }

    /// Run a search.
    pub async fn search(&self, q: &SearchQuery) -> Result<SearchPage> {
        let url = format!("{}{}", self.base, SEARCH_PATH);
        let v = self
            .http
            .post_json(SERVICE, &url, &q.to_body(), Some(SEARCH_TTL))
            .await?;
        parse_search(v)
    }

    /// Full category tree with part counts.
    pub async fn categories(&self) -> Result<Vec<CategoryNode>> {
        let url = format!("{}{}", self.base, SEARCH_PATH);
        let body = json!({
            "currentPage": 1, "pageSize": 1, "keyword": Value::Null, "searchType": 3,
            "searchSource": "search", "componentLibraryType": Value::Null, "stockFlag": false,
            "componentBrandList": [], "componentSpecificationList": [],
            "componentAttributeList": [], "paramList": []
        });
        let v = self
            .http
            .post_json(SERVICE, &url, &body, Some(CATEGORY_TTL))
            .await?;
        parse_categories(v)
    }

    /// Exact detail for one part code (e.g. "C25744").
    pub async fn detail(&self, code: &str) -> Result<Option<DetailRecord>> {
        let url = format!("{}{}?componentCode={}", self.base, DETAIL_PATH, code);
        let v = self.http.get_json(SERVICE, &url, Some(DETAIL_TTL)).await?;
        parse_detail(v)
    }

    /// Uncached liveness probe.
    pub async fn ping(&self) -> Result<()> {
        let url = format!("{}{}", self.base, SEARCH_PATH);
        let q = SearchQuery {
            keyword: Some("C25744".into()),
            page_size: 1,
            ..Default::default()
        };
        parse_search(self.http.post_json(SERVICE, &url, &q.to_body(), None).await?).map(|_| ())
    }

    /// Find the search row for an exact part code (gives the Preferred flag).
    pub async fn search_row(&self, code: &str) -> Result<Option<SearchRow>> {
        let q = SearchQuery {
            keyword: Some(code.to_string()),
            page_size: 10,
            ..Default::default()
        };
        let page = self.search(&q).await?;
        Ok(page
            .rows
            .into_iter()
            .find(|r| r.code().is_some_and(|c| c.eq_ignore_ascii_case(code))))
    }
}

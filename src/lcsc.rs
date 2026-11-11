//! LCSC product-detail endpoint (`wmsc.lcsc.com`). Reports LCSC *retail*
//! stock and pricing, which is a different inventory pool from JLCPCB
//! assembly stock: an LCSC retail stock of 0 does not mean the part is
//! unavailable for JLCPCB assembly.

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use crate::error::{ApiError, Result};
use crate::http::HttpClient;
use crate::model::{Attribute, PriceBreak, clean, de_opt_f64, de_opt_i64, sort_breaks};

pub const SERVICE: &str = "LCSC product API";
pub const DEFAULT_BASE: &str = "https://wmsc.lcsc.com";
const DETAIL_PATH: &str = "/ftps/wm/product/detail";
const TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WirePrice {
    #[serde(default, deserialize_with = "de_opt_i64")]
    ladder: Option<i64>,
    #[serde(default, deserialize_with = "de_opt_f64")]
    usd_price: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireParam {
    #[serde(default)]
    param_name_en: Option<String>,
    #[serde(default)]
    param_value_en: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireDetail {
    #[serde(default)]
    product_code: Option<String>,
    #[serde(default)]
    product_model: Option<String>,
    #[serde(default)]
    brand_name_en: Option<String>,
    #[serde(default)]
    encap_standard: Option<String>,
    #[serde(default)]
    product_intro_en: Option<String>,
    #[serde(default)]
    parent_catalog_name: Option<String>,
    #[serde(default)]
    catalog_name: Option<String>,
    #[serde(default, deserialize_with = "de_opt_i64")]
    stock_number: Option<i64>,
    #[serde(default)]
    product_price_list: Option<Vec<WirePrice>>,
    #[serde(default, rename = "paramVOList")]
    param_vo_list: Option<Vec<WireParam>>,
    #[serde(default)]
    pdf_url: Option<String>,
    #[serde(default)]
    product_images: Option<Vec<String>>,
    #[serde(default, deserialize_with = "de_opt_i64")]
    min_buy_number: Option<i64>,
}

/// Normalised LCSC detail.
#[derive(Debug, Clone, PartialEq)]
pub struct LcscDetail {
    pub lcsc: String,
    pub mfr_part: Option<String>,
    pub manufacturer: Option<String>,
    pub package: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub subcategory: Option<String>,
    pub retail_stock: Option<i64>,
    pub price_breaks: Vec<PriceBreak>,
    pub attributes: Vec<Attribute>,
    pub datasheet_url: Option<String>,
    pub image_url: Option<String>,
    pub min_buy_qty: Option<i64>,
}

/// Parse a wmsc response; `Ok(None)` when the part is unknown.
pub fn parse_detail(v: Value) -> Result<Option<LcscDetail>> {
    let code = v.get("code").and_then(Value::as_i64).unwrap_or(-1);
    if code != 200 {
        return Err(ApiError::Api {
            service: SERVICE,
            code,
            message: v
                .get("msg")
                .or_else(|| v.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("unknown error")
                .to_string(),
        });
    }
    let result = match v.get("result") {
        None | Some(Value::Null) => return Ok(None),
        Some(r) => r.clone(),
    };
    let w: WireDetail = serde_json::from_value(result).map_err(|e| ApiError::Decode {
        service: SERVICE,
        message: e.to_string(),
    })?;
    let Some(lcsc) = clean(w.product_code) else {
        return Ok(None);
    };
    let mut price_breaks: Vec<PriceBreak> = w
        .product_price_list
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| {
            let price = p.usd_price.filter(|x| x.is_finite() && *x > 0.0)?;
            Some(PriceBreak {
                min_qty: p.ladder.unwrap_or(1).max(1) as u64,
                max_qty: None,
                unit_price_usd: price,
            })
        })
        .collect();
    sort_breaks(&mut price_breaks);
    let attributes = w
        .param_vo_list
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| {
            Some(Attribute {
                name: clean(p.param_name_en)?,
                value: clean(p.param_value_en)?,
            })
        })
        .collect();
    Ok(Some(LcscDetail {
        lcsc,
        mfr_part: clean(w.product_model),
        manufacturer: clean(w.brand_name_en),
        package: clean(w.encap_standard),
        description: clean(w.product_intro_en),
        category: clean(w.parent_catalog_name),
        subcategory: clean(w.catalog_name),
        retail_stock: w.stock_number,
        price_breaks,
        attributes,
        datasheet_url: clean(w.pdf_url),
        image_url: w
            .product_images
            .unwrap_or_default()
            .into_iter()
            .find_map(|s| clean(Some(s))),
        min_buy_qty: w.min_buy_number,
    }))
}

/// LCSC client.
#[derive(Clone)]
pub struct LcscClient {
    http: Arc<HttpClient>,
    base: String,
}

impl LcscClient {
    pub fn new(http: Arc<HttpClient>, base: impl Into<String>) -> Self {
        Self {
            http,
            base: base.into().trim_end_matches('/').to_string(),
        }
    }

    /// Uncached liveness probe.
    pub async fn ping(&self) -> Result<()> {
        let url = format!("{}{}?productCode=C25744", self.base, DETAIL_PATH);
        parse_detail(self.http.get_json(SERVICE, &url, None).await?).map(|_| ())
    }

    /// Fetch LCSC retail details for a part code.
    pub async fn detail(&self, code: &str) -> Result<Option<LcscDetail>> {
        let url = format!("{}{}?productCode={}", self.base, DETAIL_PATH, code);
        let v = self.http.get_json(SERVICE, &url, Some(TTL)).await?;
        parse_detail(v)
    }
}

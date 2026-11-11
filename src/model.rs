//! Normalised, source-independent output types returned by the tools.

use std::cmp::Ordering;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

/// JLCPCB assembly library class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LibraryType {
    /// Basic part: pre-loaded on JLCPCB's pick-and-place feeders, no per-part setup fee.
    Basic,
    /// Preferred Extended part: extended part with the setup fee waived for Economic PCBA.
    Preferred,
    /// Extended part: JLCPCB charges a per-unique-part feeder setup fee.
    Extended,
    /// Unknown / not reported.
    Unknown,
}

impl LibraryType {
    /// Map JLCPCB's raw `componentLibraryType` + `preferredComponentFlag`.
    pub fn from_raw(raw: Option<&str>, preferred: Option<bool>) -> Self {
        match raw.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
            Some("base") => LibraryType::Basic,
            Some("expand") if preferred == Some(true) => LibraryType::Preferred,
            Some("expand") => LibraryType::Extended,
            _ => LibraryType::Unknown,
        }
    }

    /// Sort rank: Basic first.
    pub fn rank(self) -> u8 {
        match self {
            LibraryType::Basic => 0,
            LibraryType::Preferred => 1,
            LibraryType::Extended => 2,
            LibraryType::Unknown => 3,
        }
    }
}

/// One quantity price break.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PriceBreak {
    /// Minimum quantity for this unit price.
    pub min_qty: u64,
    /// Maximum quantity for this tier (absent = no upper bound).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_qty: Option<u64>,
    /// Unit price in USD.
    pub unit_price_usd: f64,
}

/// Unit price applicable to `qty` (falls back to the first tier when `qty`
/// is below the lowest break). `breaks` need not be sorted.
pub fn unit_price_at(breaks: &[PriceBreak], qty: u64) -> Option<f64> {
    let mut sorted: Vec<&PriceBreak> = breaks.iter().collect();
    sorted.sort_by_key(|b| b.min_qty);
    let mut price = sorted.first().map(|b| b.unit_price_usd)?;
    for b in sorted {
        if qty >= b.min_qty {
            price = b.unit_price_usd;
        }
    }
    Some(price)
}

/// Sort price breaks ascending by quantity (upstream sometimes shuffles them).
pub fn sort_breaks(breaks: &mut [PriceBreak]) {
    breaks.sort_by_key(|b| b.min_qty);
}

/// A named parameter / attribute.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Attribute {
    pub name: String,
    pub value: String,
}

/// A compact search result.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ComponentSummary {
    /// LCSC / JLCPCB part number, e.g. "C25744".
    pub lcsc: String,
    pub mfr_part: Option<String>,
    pub manufacturer: Option<String>,
    pub package: Option<String>,
    pub description: Option<String>,
    /// Top-level category, e.g. "Resistors".
    pub category: Option<String>,
    /// Leaf category, e.g. "Chip Resistor - Surface Mount".
    pub subcategory: Option<String>,
    pub library_type: LibraryType,
    /// Stock available to JLCPCB assembly (the number that matters for PCBA).
    pub jlc_stock: i64,
    /// Quantity the `unit_price_usd` refers to.
    pub price_qty: u64,
    /// JLCPCB unit price (USD) at `price_qty`.
    pub unit_price_usd: Option<f64>,
    pub price_breaks: Vec<PriceBreak>,
    /// Key electrical parameters.
    pub attributes: Vec<Attribute>,
    pub datasheet_url: Option<String>,
    pub jlcpcb_url: String,
    /// JLCPCB's own suggested replacement part, if it lists one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jlc_suggested_replacement: Option<String>,
}

impl ComponentSummary {
    /// Ranking used everywhere: Basic → Preferred → Extended, then stock
    /// (descending), then unit price (ascending, unknown last).
    pub fn rank_cmp(a: &Self, b: &Self) -> Ordering {
        a.library_type
            .rank()
            .cmp(&b.library_type.rank())
            .then_with(|| b.jlc_stock.cmp(&a.jlc_stock))
            .then_with(|| cmp_price(a.unit_price_usd, b.unit_price_usd))
            .then_with(|| a.lcsc.cmp(&b.lcsc))
    }

    /// Look up an attribute value by (case-insensitive) name.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(name))
            .map(|a| a.value.as_str())
    }
}

/// Compare optional prices, `None` sorts last.
pub fn cmp_price(a: Option<f64>, b: Option<f64>) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Normalise "C25744", "c25744", "25744", " C25744 " → "C25744".
pub fn normalize_lcsc(raw: &str) -> Option<String> {
    let s = raw.trim();
    let digits = s.strip_prefix(['C', 'c']).unwrap_or(s);
    if !digits.is_empty() && digits.len() <= 12 && digits.chars().all(|c| c.is_ascii_digit()) {
        Some(format!("C{digits}"))
    } else {
        None
    }
}

/// Deserialize a number that upstream may send as a JSON number, a numeric
/// string, or null.
pub fn de_opt_f64<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    let v = Option::<serde_json::Value>::deserialize(d)?;
    Ok(match v {
        Some(serde_json::Value::Number(n)) => n.as_f64(),
        Some(serde_json::Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    })
}

/// Deserialize an integer that may arrive as number, string or null.
pub fn de_opt_i64<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    let v = Option::<serde_json::Value>::deserialize(d)?;
    Ok(match v {
        Some(serde_json::Value::Number(n)) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Some(serde_json::Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    })
}

/// Treat empty strings / "None" / "-" as absent.
pub fn clean(s: Option<String>) -> Option<String> {
    s.map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty() && v != "None" && v != "null" && v != "-")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(lcsc: &str, lib: LibraryType, stock: i64, price: Option<f64>) -> ComponentSummary {
        ComponentSummary {
            lcsc: lcsc.into(),
            mfr_part: None,
            manufacturer: None,
            package: None,
            description: None,
            category: None,
            subcategory: None,
            library_type: lib,
            jlc_stock: stock,
            price_qty: 1,
            unit_price_usd: price,
            price_breaks: vec![],
            attributes: vec![],
            datasheet_url: None,
            jlcpcb_url: String::new(),
            jlc_suggested_replacement: None,
        }
    }

    #[test]
    fn ranking_basic_then_stock_then_price() {
        let mut v = [
            summary("C1", LibraryType::Extended, 1_000_000, Some(0.001)),
            summary("C2", LibraryType::Basic, 10, Some(0.5)),
            summary("C3", LibraryType::Basic, 5000, Some(0.5)),
            summary("C4", LibraryType::Preferred, 1, None),
            summary("C5", LibraryType::Basic, 5000, Some(0.1)),
        ];
        v.sort_by(ComponentSummary::rank_cmp);
        let order: Vec<&str> = v.iter().map(|s| s.lcsc.as_str()).collect();
        assert_eq!(order, ["C5", "C3", "C2", "C4", "C1"]);
    }

    #[test]
    fn library_type_mapping() {
        assert_eq!(
            LibraryType::from_raw(Some("base"), None),
            LibraryType::Basic
        );
        assert_eq!(
            LibraryType::from_raw(Some("expand"), Some(true)),
            LibraryType::Preferred
        );
        assert_eq!(
            LibraryType::from_raw(Some("expand"), Some(false)),
            LibraryType::Extended
        );
        assert_eq!(LibraryType::from_raw(None, None), LibraryType::Unknown);
    }

    #[test]
    fn price_lookup() {
        let breaks = vec![
            PriceBreak {
                min_qty: 1000,
                max_qty: None,
                unit_price_usd: 0.5,
            },
            PriceBreak {
                min_qty: 1,
                max_qty: Some(99),
                unit_price_usd: 1.0,
            },
            PriceBreak {
                min_qty: 100,
                max_qty: Some(999),
                unit_price_usd: 0.8,
            },
        ];
        assert_eq!(unit_price_at(&breaks, 1), Some(1.0));
        assert_eq!(unit_price_at(&breaks, 150), Some(0.8));
        assert_eq!(unit_price_at(&breaks, 5000), Some(0.5));
        assert_eq!(unit_price_at(&[], 5), None);
    }

    #[test]
    fn lcsc_normalisation() {
        assert_eq!(normalize_lcsc("C25744").as_deref(), Some("C25744"));
        assert_eq!(normalize_lcsc(" c25744 ").as_deref(), Some("C25744"));
        assert_eq!(normalize_lcsc("25744").as_deref(), Some("C25744"));
        assert_eq!(normalize_lcsc("AMS1117"), None);
        assert_eq!(normalize_lcsc("C"), None);
    }
}

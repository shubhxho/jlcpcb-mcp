//! Rough bare-PCB fabrication cost model.
//!
//! JLCPCB does not offer an unauthenticated price-calculation API (its
//! official `pcb/calculate` endpoint needs approved API credentials *and* an
//! uploaded Gerber). This module is therefore an **approximate model** with
//! every coefficient documented below. It is not a JLCPCB quote and its
//! coefficients are the author's assumptions, not published JLCPCB prices.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{ApiError, Result};

/// URL of the official instant-quote page.
pub const OFFICIAL_QUOTE_URL: &str = "https://cart.jlcpcb.com/quote";

/// Surface finish options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceFinish {
    /// HASL with lead (cheapest).
    #[default]
    Hasl,
    /// Lead-free HASL.
    LeadFreeHasl,
    /// ENIG (immersion gold).
    Enig,
}

/// Input to the estimator.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct PcbSpec {
    /// Board width in mm.
    pub width_mm: f64,
    /// Board height in mm.
    pub height_mm: f64,
    /// Number of boards.
    #[serde(default = "default_qty")]
    pub quantity: u32,
    /// Copper layers: 1, 2, 4, 6 or 8.
    #[serde(default = "default_layers")]
    pub layers: u8,
    /// Board thickness in mm (1.6 is standard).
    #[serde(default = "default_thickness")]
    pub thickness_mm: f64,
    /// Surface finish.
    #[serde(default)]
    pub surface_finish: SurfaceFinish,
}

fn default_qty() -> u32 {
    5
}
fn default_layers() -> u8 {
    2
}
fn default_thickness() -> f64 {
    1.6
}

/// Itemised estimate.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PcbEstimate {
    /// Always false: this is a model, not an official quote.
    pub is_official_quote: bool,
    pub method: String,
    pub low_usd: f64,
    pub typical_usd: f64,
    pub high_usd: f64,
    pub per_board_typical_usd: f64,
    pub board_area_cm2: f64,
    pub total_area_m2: f64,
    pub breakdown: Vec<LineItem>,
    pub assumptions: Vec<String>,
    pub excluded: Vec<String>,
    pub official_quote_url: String,
    pub disclaimer: String,
}

/// One cost component.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LineItem {
    pub item: String,
    pub usd: f64,
}

// --- Model coefficients (assumptions; see README "PCB estimate model") -----

/// Promotional-style small-batch price for ≤5 boards ≤100×100 mm, by layers.
fn small_batch_base(layers: u8) -> f64 {
    match layers {
        1 | 2 => 2.0,
        4 => 7.0,
        6 => 30.0,
        _ => 60.0, // 8 layers
    }
}

/// Engineering/setup fee once outside the small-batch envelope.
fn setup_fee(layers: u8) -> f64 {
    match layers {
        1 | 2 => 5.0,
        4 => 15.0,
        6 => 45.0,
        _ => 80.0,
    }
}

/// Material+process cost per cm² of board, by layers.
fn area_rate_per_cm2(layers: u8) -> f64 {
    match layers {
        1 => 0.008,
        2 => 0.010,
        4 => 0.025,
        6 => 0.055,
        _ => 0.085,
    }
}

/// Volume discount on the area cost by total panel area.
fn volume_factor(total_m2: f64) -> f64 {
    if total_m2 >= 5.0 {
        0.65
    } else if total_m2 >= 1.0 {
        0.8
    } else {
        1.0
    }
}

const SMALL_BATCH_MAX_QTY: u32 = 5;
const SMALL_BATCH_MAX_MM: f64 = 100.0;
const LOW_FACTOR: f64 = 0.75;
const HIGH_FACTOR: f64 = 1.5;

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// Validate and estimate.
pub fn estimate(spec: &PcbSpec) -> Result<PcbEstimate> {
    let PcbSpec {
        width_mm,
        height_mm,
        quantity,
        layers,
        thickness_mm,
        surface_finish,
    } = *spec;
    if !(width_mm.is_finite() && height_mm.is_finite()) || width_mm < 5.0 || height_mm < 5.0 {
        return Err(ApiError::InvalidInput(
            "width_mm and height_mm must be at least 5 mm".into(),
        ));
    }
    if width_mm > 600.0 || height_mm > 600.0 {
        return Err(ApiError::InvalidInput(
            "boards larger than 600 mm on a side are outside this model; use the official quote page".into(),
        ));
    }
    if !matches!(layers, 1 | 2 | 4 | 6 | 8) {
        return Err(ApiError::InvalidInput(
            "layers must be 1, 2, 4, 6 or 8".into(),
        ));
    }
    if quantity == 0 || quantity > 100_000 {
        return Err(ApiError::InvalidInput(
            "quantity must be between 1 and 100000".into(),
        ));
    }
    if !(0.4..=3.2).contains(&thickness_mm) {
        return Err(ApiError::InvalidInput(
            "thickness_mm must be between 0.4 and 3.2".into(),
        ));
    }

    let area_cm2 = width_mm * height_mm / 100.0;
    let total_m2 = area_cm2 * quantity as f64 / 10_000.0;
    let mut items = Vec::new();

    let small_batch = quantity <= SMALL_BATCH_MAX_QTY
        && width_mm <= SMALL_BATCH_MAX_MM
        && height_mm <= SMALL_BATCH_MAX_MM;
    let bare = if small_batch {
        let v = small_batch_base(layers);
        items.push(LineItem {
            item: format!("small-batch base price ({layers}L, ≤5 pcs, ≤100×100 mm)"),
            usd: v,
        });
        v
    } else {
        let setup = setup_fee(layers);
        let area_cost =
            area_rate_per_cm2(layers) * area_cm2 * quantity as f64 * volume_factor(total_m2);
        items.push(LineItem {
            item: format!("engineering/setup ({layers}L)"),
            usd: setup,
        });
        items.push(LineItem {
            item: format!(
                "area cost {:.1} cm² × {quantity} @ ${:.3}/cm² × volume factor {:.2}",
                area_cm2,
                area_rate_per_cm2(layers),
                volume_factor(total_m2)
            ),
            usd: round2(area_cost),
        });
        setup + area_cost
    };

    let mut extras = 0.0;
    if (thickness_mm - 1.6).abs() > 1e-6 {
        let v = (bare * 0.10).max(2.0);
        items.push(LineItem {
            item: format!("non-standard thickness {thickness_mm} mm (+10%, min $2)"),
            usd: round2(v),
        });
        extras += v;
    }
    match surface_finish {
        SurfaceFinish::Hasl => {}
        SurfaceFinish::LeadFreeHasl => {
            let v = (bare * 0.05).max(1.0);
            items.push(LineItem {
                item: "lead-free HASL (+5%, min $1)".into(),
                usd: round2(v),
            });
            extras += v;
        }
        SurfaceFinish::Enig => {
            let v = 15.0 + bare * 0.10;
            items.push(LineItem {
                item: "ENIG ($15 + 10%)".into(),
                usd: round2(v),
            });
            extras += v;
        }
    }

    let typical = bare + extras;
    Ok(PcbEstimate {
        is_official_quote: false,
        method: "approximate parametric model (not a JLCPCB quote)".into(),
        low_usd: round2(typical * LOW_FACTOR),
        typical_usd: round2(typical),
        high_usd: round2(typical * HIGH_FACTOR),
        per_board_typical_usd: round2(typical / quantity as f64),
        board_area_cm2: round2(area_cm2),
        total_area_m2: (total_m2 * 10_000.0).round() / 10_000.0,
        breakdown: items,
        assumptions: vec![
            "Coefficients are the server author's rough assumptions loosely shaped like JLCPCB's advertised low-volume pricing; they are NOT published JLCPCB prices and may be stale.".into(),
            "Standard specs: FR-4, 1 oz outer copper, green solder mask, white silkscreen, ≥0.3 mm drills, no castellations/impedance control/gold fingers.".into(),
            "≤5 boards up to 100×100 mm use a flat small-batch price; anything larger uses setup fee + per-area cost with volume discounts at 1 m² and 5 m².".into(),
            "Range is −25% / +50% around the typical value.".into(),
        ],
        excluded: vec![
            "shipping, taxes and customs".into(),
            "PCB assembly (SMT), components, stencils".into(),
            "build-time upgrades, colour changes, special processes".into(),
        ],
        official_quote_url: OFFICIAL_QUOTE_URL.into(),
        disclaimer: "Approximate model only. Get the real price from the official JLCPCB quote page before ordering.".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(w: f64, h: f64, q: u32, l: u8) -> PcbSpec {
        PcbSpec {
            width_mm: w,
            height_mm: h,
            quantity: q,
            layers: l,
            thickness_mm: 1.6,
            surface_finish: SurfaceFinish::Hasl,
        }
    }

    #[test]
    fn small_batch_two_layer() {
        let e = estimate(&spec(50.0, 50.0, 5, 2)).unwrap();
        assert!(!e.is_official_quote);
        assert_eq!(e.typical_usd, 2.0);
        assert!(e.low_usd < e.typical_usd && e.typical_usd < e.high_usd);
    }

    #[test]
    fn more_layers_and_area_cost_more() {
        let two = estimate(&spec(120.0, 80.0, 50, 2)).unwrap();
        let four = estimate(&spec(120.0, 80.0, 50, 4)).unwrap();
        let bigger = estimate(&spec(160.0, 100.0, 50, 2)).unwrap();
        assert!(four.typical_usd > two.typical_usd);
        assert!(bigger.typical_usd > two.typical_usd);
    }

    #[test]
    fn volume_discount_lowers_per_board_cost() {
        let small = estimate(&spec(100.0, 100.0, 20, 2)).unwrap();
        let large = estimate(&spec(100.0, 100.0, 1000, 2)).unwrap();
        assert!(large.per_board_typical_usd < small.per_board_typical_usd);
    }

    #[test]
    fn options_add_cost() {
        let base = estimate(&spec(100.0, 100.0, 10, 2)).unwrap();
        let mut s = spec(100.0, 100.0, 10, 2);
        s.surface_finish = SurfaceFinish::Enig;
        s.thickness_mm = 1.0;
        let fancy = estimate(&s).unwrap();
        assert!(fancy.typical_usd > base.typical_usd + 15.0);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(estimate(&spec(1.0, 50.0, 5, 2)).is_err());
        assert!(estimate(&spec(50.0, 50.0, 5, 3)).is_err());
        assert!(estimate(&spec(50.0, 50.0, 0, 2)).is_err());
        let mut s = spec(50.0, 50.0, 5, 2);
        s.thickness_mm = 5.0;
        assert!(estimate(&s).is_err());
    }
}

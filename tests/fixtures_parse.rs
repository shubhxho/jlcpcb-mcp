//! Parsing tests against real (scrubbed) upstream responses captured in
//! `tests/fixtures`. No network access.

use jlcpcb_mcp::jlc::{self, LibraryFilter, SearchQuery};
use jlcpcb_mcp::model::{ComponentSummary, LibraryType};
use jlcpcb_mcp::{ApiError, lcsc};
use serde_json::Value;

fn fixture(name: &str) -> Value {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(&path).expect(&path)).expect("valid json")
}

#[test]
fn parses_search_page() {
    let page = jlc::parse_search(fixture("search_10k_0402.json")).unwrap();
    assert!(page.total > 0);
    assert!(!page.rows.is_empty());
    let rows: Vec<ComponentSummary> = page
        .rows
        .into_iter()
        .filter_map(|r| r.into_summary(100))
        .collect();
    for c in &rows {
        assert!(c.lcsc.starts_with('C'));
        assert_eq!(c.package.as_deref(), Some("0402"));
        assert_eq!(c.category.as_deref(), Some("Resistors"));
        assert_eq!(
            c.subcategory.as_deref(),
            Some("Chip Resistor - Surface Mount")
        );
        assert!(c.jlc_stock > 0);
        assert!(!c.price_breaks.is_empty());
        // breaks are sorted ascending
        assert!(
            c.price_breaks
                .windows(2)
                .all(|w| w[0].min_qty < w[1].min_qty)
        );
        assert!(c.jlcpcb_url.starts_with("https://jlcpcb.com/partdetail/"));
    }
    let basic = rows.iter().find(|c| c.lcsc == "C25744").expect("C25744");
    assert_eq!(basic.library_type, LibraryType::Basic);
    assert_eq!(basic.attr("Resistance"), Some("10kΩ"));
}

#[test]
fn parses_basic_filtered_page() {
    let page = jlc::parse_search(fixture("search_10k_0402_basic.json")).unwrap();
    for r in page.rows {
        let s = r.into_summary(1).unwrap();
        assert!(matches!(
            s.library_type,
            LibraryType::Basic | LibraryType::Preferred
        ));
    }
}

#[test]
fn parses_detail_record() {
    let rec = jlc::parse_detail(fixture("detail_C6186.json"))
        .unwrap()
        .expect("found");
    let d = rec.into_detail(100).unwrap();
    assert_eq!(d.summary.lcsc, "C6186");
    assert_eq!(d.summary.mfr_part.as_deref(), Some("AMS1117-3.3"));
    assert_eq!(d.summary.package.as_deref(), Some("SOT-223"));
    assert_eq!(d.summary.category.as_deref(), Some("Power Management (PMIC)"));
    assert_eq!(d.summary.library_type, LibraryType::Basic);
    assert_eq!(d.summary.attr("Output Voltage"), Some("3.3V"));
    assert_eq!(d.assembly_process.as_deref(), Some("SMT"));
    assert!(d.buyable);
    assert!(d.summary.datasheet_url.is_some());
    assert!(d.summary.unit_price_usd.unwrap() > 0.0);
}

#[test]
fn not_found_detail_is_none() {
    assert!(
        jlc::parse_detail(fixture("detail_not_found.json"))
            .unwrap()
            .is_none()
    );
    assert!(
        lcsc::parse_detail(fixture("lcsc_not_found.json"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn parses_category_tree() {
    let tree = jlc::parse_categories(fixture("categories.json")).unwrap();
    assert!(!tree.is_empty());
    let caps = tree.iter().find(|c| c.name == "Capacitors").unwrap();
    assert!(caps.count > 0);
    assert!(caps.children.iter().all(|c| c.count > 0 && c.id > 0));
}

#[test]
fn parses_lcsc_detail() {
    let d = lcsc::parse_detail(fixture("lcsc_C6186.json"))
        .unwrap()
        .unwrap();
    assert_eq!(d.lcsc, "C6186");
    assert_eq!(d.package.as_deref(), Some("SOT-223"));
    assert!(d.retail_stock.is_some());
    assert!(!d.price_breaks.is_empty());
    assert!(!d.attributes.is_empty());
}

#[test]
fn api_error_envelope() {
    let err = jlc::parse_search(fixture("search_error_101.json")).unwrap_err();
    match err {
        ApiError::Api { code, message, .. } => {
            assert_eq!(code, 101);
            assert!(message.contains("try simpler filters"));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn search_body_shape() {
    let q = SearchQuery {
        keyword: Some(" 10k ".into()),
        parent_category: Some("Resistors".into()),
        packages: vec!["0402".into()],
        attributes: vec![("Resistance".into(), vec!["10kΩ".into()])],
        library: LibraryFilter::BasicOrPreferred,
        in_stock_only: true,
        min_stock: Some(500),
        page_size: 500,
        ..Default::default()
    };
    let b = q.to_body();
    assert_eq!(b["keyword"], "10k");
    assert_eq!(b["pageSize"], 100, "page size is capped");
    assert_eq!(b["firstSortName"], "Resistors");
    assert_eq!(b["componentSpecificationList"][0], "0402");
    assert_eq!(b["componentAttributeList"][0]["Resistance"][0], "10kΩ");
    assert_eq!(b["componentLibTypes"][0], "base");
    assert_eq!(b["preferredComponentFlag"], true);
    assert_eq!(b["startStockNumber"], 500);
    assert_eq!(b["stockFlag"], true);

    let plain = SearchQuery {
        keyword: Some("x".into()),
        page_size: 10,
        ..Default::default()
    }
    .to_body();
    assert!(plain.get("componentLibTypes").is_none());
    assert!(plain.get("startStockNumber").is_none());
}

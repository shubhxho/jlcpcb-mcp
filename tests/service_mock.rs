//! Service-level tests against a local mock HTTP server (wiremock) serving
//! captured fixtures. No real network access.

use std::time::Duration;

use jlcpcb_mcp::http::HttpConfig;
use jlcpcb_mcp::model::LibraryType;
use jlcpcb_mcp::service::{AlternativesArgs, CategoriesArgs, DetailsArgs, SearchArgs};
use jlcpcb_mcp::{ApiError, Config, Service};
use serde_json::{Value, json};
use wiremock::matchers::{body_partial_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SEARCH: &str = "/overseas-pcb-order/v1/shoppingCart/smtGood/selectSmtComponentList/v2";
const DETAIL: &str = "/overseas-pcb-order/v1/smtComponentShopCart/getComponentDetail";
const LCSC: &str = "/ftps/wm/product/detail";

fn fixture(name: &str) -> Value {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(&path).expect(&path)).unwrap()
}

fn service(server: &MockServer, http: HttpConfig) -> Service {
    Service::new(Config {
        jlc_base: server.uri(),
        lcsc_base: server.uri(),
        official_base: server.uri(),
        credentials: None,
        http,
    })
}

fn ok(v: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(v)
}

fn empty_search() -> Value {
    json!({"code":200,"data":{"componentPageInfo":{"total":0,"list":[]}}})
}

#[tokio::test]
async fn search_ranks_basic_first_and_respects_limits() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(SEARCH))
        .and(body_partial_json(json!({"componentLibTypes":["base"]})))
        .respond_with(ok(fixture("search_10k_0402_basic.json")))
        .with_priority(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(SEARCH))
        .and(body_partial_json(
            json!({"keyword":"10k","componentSpecificationList":["0402"],"stockFlag":true}),
        ))
        .respond_with(ok(fixture("search_10k_0402.json")))
        .with_priority(2)
        .expect(1)
        .mount(&server)
        .await;

    let svc = service(&server, HttpConfig::for_tests());
    let res = svc
        .search(SearchArgs {
            query: Some("10k".into()),
            package: Some("0402".into()),
            max_results: Some(4),
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(res.returned, 4);
    assert!(res.total_matches > 4);
    assert_eq!(res.results[0].lcsc, "C25744");
    assert_eq!(res.results[0].library_type, LibraryType::Basic);
    // no duplicates even though C25744 is in both upstream pages
    let mut codes: Vec<_> = res.results.iter().map(|c| c.lcsc.clone()).collect();
    codes.dedup();
    assert_eq!(codes.len(), 4);
    // non-basic tail is sorted by stock descending
    let tail: Vec<i64> = res.results[1..].iter().map(|c| c.jlc_stock).collect();
    assert!(tail.windows(2).all(|w| w[0] >= w[1]), "{tail:?}");
    assert_eq!(res.filters.packages, vec!["0402".to_string()]);
}

#[tokio::test]
async fn basic_only_sends_library_filter_and_category_is_resolved() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(SEARCH))
        .and(body_partial_json(json!({"searchType":3})))
        .respond_with(ok(fixture("categories.json")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(SEARCH))
        .and(body_partial_json(
            json!({"firstSortName":"Capacitors","componentLibTypes":["base"]}),
        ))
        .respond_with(ok(empty_search()))
        .expect(1)
        .mount(&server)
        .await;

    let svc = service(&server, HttpConfig::for_tests());
    let res = svc
        .search(SearchArgs {
            query: Some("100nF".into()),
            category: Some("capacitor".into()), // singular, case-insensitive
            package: Some("0402".into()),
            basic_only: Some(true),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(res.filters.category.as_deref(), Some("Capacitors"));
    assert_eq!(res.filters.library, "basic");
    assert_eq!(res.returned, 0);
    assert!(res.notes.iter().any(|n| n.contains("Package names")));
}

#[tokio::test]
async fn search_requires_query_or_category() {
    let server = MockServer::start().await;
    let svc = service(&server, HttpConfig::for_tests());
    let err = svc.search(SearchArgs::default()).await.unwrap_err();
    assert!(matches!(err, ApiError::InvalidInput(_)));
}

#[tokio::test]
async fn details_merge_jlc_and_lcsc() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(DETAIL))
        .and(query_param("componentCode", "C6186"))
        .respond_with(ok(fixture("detail_C6186.json")))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(SEARCH))
        .respond_with(ok(fixture("search_C6186.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(LCSC))
        .and(query_param("productCode", "C6186"))
        .respond_with(ok(fixture("lcsc_C6186.json")))
        .mount(&server)
        .await;

    let svc = service(&server, HttpConfig::for_tests());
    let d = svc
        .details(DetailsArgs {
            lcsc: "c6186".into(),
            quantity: Some(1),
        })
        .await
        .unwrap();
    assert_eq!(d.lcsc, "C6186");
    assert_eq!(d.mfr_part.as_deref(), Some("AMS1117-3.3"));
    assert_eq!(d.library_type, LibraryType::Basic);
    assert!(d.jlc_assembly_stock.unwrap() > 0);
    assert!(d.lcsc_retail_stock.is_some());
    assert!(!d.jlc_price_breaks.is_empty());
    assert!(!d.lcsc_price_breaks.is_empty());
    assert_eq!(
        d.jlc_unit_price_usd,
        Some(d.jlc_price_breaks[0].unit_price_usd)
    );
    assert!(d.datasheet_url.is_some());
    assert_eq!(d.sources.len(), 2, "{:?}", d.sources);
    assert!(d.warnings.is_empty(), "{:?}", d.warnings);
}

#[tokio::test]
async fn details_survive_lcsc_outage_with_warning() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(DETAIL))
        .respond_with(ok(fixture("detail_C6186.json")))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(SEARCH))
        .respond_with(ok(empty_search()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(LCSC))
        .respond_with(ResponseTemplate::new(503).set_body_string("down"))
        .mount(&server)
        .await;

    let svc = service(&server, HttpConfig::for_tests());
    let d = svc
        .details(DetailsArgs {
            lcsc: "C6186".into(),
            quantity: None,
        })
        .await
        .unwrap();
    assert!(d.lcsc_retail_stock.is_none());
    assert!(
        d.warnings
            .iter()
            .any(|w| w.contains("LCSC") && w.contains("503"))
    );
}

#[tokio::test]
async fn details_unknown_part_is_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(DETAIL))
        .respond_with(ok(fixture("detail_not_found.json")))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(SEARCH))
        .respond_with(ok(empty_search()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(LCSC))
        .respond_with(ok(fixture("lcsc_not_found.json")))
        .mount(&server)
        .await;
    let svc = service(&server, HttpConfig::for_tests());
    let err = svc
        .details(DetailsArgs {
            lcsc: "C999999999".into(),
            quantity: None,
        })
        .await
        .unwrap_err();
    assert!(matches!(err, ApiError::NotFound(ref m) if m.contains("C999999999")));

    let err = svc
        .details(DetailsArgs {
            lcsc: "AMS1117".into(),
            quantity: None,
        })
        .await
        .unwrap_err();
    assert!(matches!(err, ApiError::InvalidInput(_)));
}

#[tokio::test]
async fn rate_limit_is_retried_then_reported_clearly() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(SEARCH))
        .respond_with(ResponseTemplate::new(429))
        .expect(4) // 2 parallel queries x (1 try + 1 retry)
        .mount(&server)
        .await;
    let svc = service(&server, HttpConfig::for_tests());
    let err = svc
        .search(SearchArgs {
            query: Some("x".into()),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert!(matches!(err, ApiError::RateLimited { status: 429, .. }));
    assert!(err.to_string().contains("Wait about a minute"));
}

#[tokio::test]
async fn upstream_rejection_is_reported() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(SEARCH))
        .respond_with(ok(fixture("search_error_101.json")))
        .mount(&server)
        .await;
    let svc = service(&server, HttpConfig::for_tests());
    let err = svc
        .search(SearchArgs {
            query: Some("x".into()),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert!(matches!(err, ApiError::Api { code: 101, .. }), "{err:?}");
}

#[tokio::test]
async fn timeouts_are_reported() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(SEARCH))
        .respond_with(ok(empty_search()).set_delay(Duration::from_secs(3)))
        .mount(&server)
        .await;
    let http = HttpConfig {
        timeout: Duration::from_secs(1),
        retries: 0,
        ..HttpConfig::for_tests()
    };
    let svc = service(&server, http);
    let err = svc
        .search(SearchArgs {
            query: Some("x".into()),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert!(matches!(err, ApiError::Timeout { .. }), "{err:?}");
}

#[tokio::test]
async fn malformed_json_is_a_decode_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(SEARCH))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>blocked</html>"))
        .mount(&server)
        .await;
    let svc = service(&server, HttpConfig::for_tests());
    let err = svc
        .search(SearchArgs {
            query: Some("x".into()),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert!(matches!(err, ApiError::Decode { .. }), "{err:?}");
}

#[tokio::test]
async fn responses_are_cached() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(SEARCH))
        .and(body_partial_json(json!({"searchType":3})))
        .respond_with(ok(fixture("categories.json")))
        .expect(1)
        .mount(&server)
        .await;
    let svc = service(&server, HttpConfig::for_tests());
    for _ in 0..3 {
        let r = svc.categories(CategoriesArgs::default()).await.unwrap();
        assert!(r.total_categories > 0);
        assert!(r.categories[0].subcategories.is_empty());
    }
    let r = svc
        .categories(CategoriesArgs {
            search: Some("aluminum".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(r.categories.len(), 1);
    assert!(
        r.categories[0]
            .subcategories
            .iter()
            .all(|s| s.name.to_lowercase().contains("aluminum"))
    );
}

fn resistor_row(code: &str, lib: &str, stock: i64, price: f64, tol: &str, power: &str) -> Value {
    json!({
        "componentCode": code, "componentModelEn": format!("MPN-{code}"),
        "componentBrandEn": "Acme", "componentSpecificationEn": "0402",
        "describe": "10kΩ resistor", "componentLibraryType": lib,
        "preferredComponentFlag": false, "stockCount": stock,
        "componentPrices": [{"startNumber": 1, "endNumber": -1, "productPrice": price}],
        "firstSortName": "Chip Resistor - Surface Mount", "secondSortName": "Resistors",
        "attributes": [
            {"attribute_name_en": "Resistance", "attribute_value_name": "10kΩ"},
            {"attribute_name_en": "Tolerance", "attribute_value_name": tol},
            {"attribute_name_en": "Power(Watts)", "attribute_value_name": power}
        ]
    })
}

#[tokio::test]
async fn alternatives_apply_constraints_and_explain() {
    let server = MockServer::start().await;
    let original = json!({"code":200,"data":{
        "componentCode":"C100","componentModelEn":"ORIG","componentBrandEn":"Acme",
        "componentSpecificationEn":"0402","describe":"10kΩ","componentLibraryType":"expand",
        "stockCount":1000,"prices":[{"startNumber":1,"endNumber":-1,"productPrice":0.01}],
        "firstSortName":"Resistors","secondSortName":"Chip Resistor - Surface Mount",
        "attributes":[
            {"attribute_name_en":"Resistance","attribute_value_name":"10kΩ"},
            {"attribute_name_en":"Tolerance","attribute_value_name":"±1%"},
            {"attribute_name_en":"Power(Watts)","attribute_value_name":"62.5mW"}]
    }});
    Mock::given(method("GET"))
        .and(path(DETAIL))
        .respond_with(ok(original))
        .mount(&server)
        .await;
    let rows = json!({"code":200,"data":{"componentPageInfo":{"total":6,"list":[
        resistor_row("C100", "expand", 1000, 0.01, "±1%", "62.5mW"),   // itself
        resistor_row("C1", "base", 5_000_000, 0.002, "±1%", "62.5mW"), // basic+cheap: keep
        resistor_row("C2", "expand", 500, 0.02, "±1%", "62.5mW"),      // worse: drop
        resistor_row("C3", "base", 900_000, 0.001, "±5%", "62.5mW"),   // worse tolerance: drop
        resistor_row("C4", "expand", 90_000, 0.001, "±1%", "31.25mW"), // lower power: drop
        resistor_row("C5", "expand", 10, 0.001, "±1%", "62.5mW"),      // too little stock: drop
        resistor_row("C6", "expand", 9_000, 0.005, "±0.5%", "100mW"),  // cheaper + better: keep
    ]}}});
    Mock::given(method("POST"))
        .and(path(SEARCH))
        .and(body_partial_json(json!({
            "firstSortName":"Resistors",
            "secondSortName":"Chip Resistor - Surface Mount",
            "componentSpecificationList":["0402"],
            "componentAttributeList":[{"Resistance":["10kΩ"]}]
        })))
        .respond_with(ok(rows))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(SEARCH))
        .respond_with(ok(empty_search()))
        .with_priority(10)
        .mount(&server)
        .await;

    let svc = service(&server, HttpConfig::for_tests());
    let res = svc
        .alternatives(AlternativesArgs {
            lcsc: "C100".into(),
            quantity: Some(10),
            max_results: None,
            same_package: None,
            basic_or_preferred_only: None,
            min_stock: None,
        })
        .await
        .unwrap();
    let codes: Vec<&str> = res
        .alternatives
        .iter()
        .map(|a| a.component.lcsc.as_str())
        .collect();
    assert_eq!(codes, ["C1", "C6"]);
    let c1 = &res.alternatives[0];
    assert!(c1.reasons.iter().any(|r| r.contains("Basic")));
    assert!(c1.reasons.iter().any(|r| r.contains("cheaper")));
    assert_eq!(c1.price_change_pct, Some(-80.0));
    assert!(res.match_criteria.iter().any(|c| c.contains("10kΩ")));
}

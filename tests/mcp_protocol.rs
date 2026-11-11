//! End-to-end MCP protocol test: drives the real rmcp server over an
//! in-memory duplex pipe with raw JSON-RPC lines (no network).

use jlcpcb_mcp::http::HttpConfig;
use jlcpcb_mcp::{Config, JlcpcbServer, Service};
use rmcp::ServiceExt;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn initialize_list_and_call_over_jsonrpc() {
    let (client_io, server_io) = tokio::io::duplex(1 << 20);
    let (server_read, server_write) = tokio::io::split(server_io);
    let svc = Service::new(Config {
        // Unroutable: these tests must not touch the network.
        jlc_base: "http://127.0.0.1:9".into(),
        lcsc_base: "http://127.0.0.1:9".into(),
        official_base: "http://127.0.0.1:9".into(),
        credentials: None,
        http: HttpConfig::for_tests(),
    });
    let server = tokio::spawn(async move {
        let running = JlcpcbServer::new(svc)
            .serve((server_read, server_write))
            .await
            .expect("serve");
        let _ = running.waiting().await;
    });

    let (read, mut write) = tokio::io::split(client_io);
    let mut lines = BufReader::new(read).lines();
    let mut id = 0;
    let mut rpc = async |method: &str, params: Value| -> Value {
        id += 1;
        let msg = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
        write
            .write_all(format!("{msg}\n").as_bytes())
            .await
            .unwrap();
        loop {
            let line = lines.next_line().await.unwrap().expect("server closed");
            let v: Value = serde_json::from_str(&line).unwrap();
            if v["id"] == id {
                return v;
            }
        }
    };

    let init = rpc(
        "initialize",
        json!({"protocolVersion":"2025-06-18","capabilities":{},
               "clientInfo":{"name":"test","version":"0"}}),
    )
    .await;
    assert_eq!(init["result"]["serverInfo"]["name"], "jlcpcb-mcp");
    assert!(init["result"]["capabilities"]["tools"].is_object());

    let list = rpc("tools/list", json!({})).await;
    let mut names: Vec<String> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "estimate_pcb_quote",
            "get_component_details",
            "list_categories",
            "search_components",
            "service_status",
            "suggest_alternatives"
        ]
    );
    for t in list["result"]["tools"].as_array().unwrap() {
        assert!(t["inputSchema"]["type"] == "object", "{t}");
        assert_eq!(t["annotations"]["readOnlyHint"], true);
    }

    // Offline tool: works without network.
    let est = rpc(
        "tools/call",
        json!({"name":"estimate_pcb_quote",
               "arguments":{"width_mm":50,"height_mm":50,"quantity":5,"layers":2}}),
    )
    .await;
    let sc = &est["result"]["structuredContent"];
    assert_eq!(est["result"]["isError"], false);
    assert_eq!(sc["is_official_quote"], false);
    assert!(sc["typical_usd"].as_f64().unwrap() > 0.0);

    // Validation errors surface as tool errors, not protocol errors.
    let bad = rpc(
        "tools/call",
        json!({"name":"estimate_pcb_quote",
               "arguments":{"width_mm":50,"height_mm":50,"layers":3}}),
    )
    .await;
    assert_eq!(bad["result"]["isError"], true);

    // Network failure surfaces as a readable tool error.
    let net = rpc(
        "tools/call",
        json!({"name":"get_component_details","arguments":{"lcsc":"C25744"}}),
    )
    .await;
    assert_eq!(net["result"]["isError"], true);
    let text = net["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("C25744"), "{text}");

    drop(rpc);
    server.abort();
}

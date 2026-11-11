//! MCP server wiring (rmcp tool router).

use rmcp::{
    Json, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{Implementation, ServerCapabilities, ServerConfig},
    tool, tool_handler, tool_router,
};

use crate::pcb::{self, PcbEstimate, PcbSpec};
use crate::service::{
    AlternativesArgs, AlternativesResult, CategoriesArgs, CategoriesResult, ComponentDetails,
    DetailsArgs, SearchArgs, SearchResult, Service, StatusResult,
};

const INSTRUCTIONS: &str = "Find and evaluate electronic components for JLCPCB PCB assembly. \
Typical flow: search_components (filter by category/package, basic_only to avoid extended-part fees) → \
get_component_details for live stock, price breaks, datasheet → suggest_alternatives for cheaper, \
better-stocked or Basic-library substitutes. Use list_categories to discover exact category names. \
jlc_stock / jlc_assembly_stock is the JLCPCB assembly inventory; lcsc_retail_stock is a separate LCSC \
retail pool. estimate_pcb_quote is an approximate model, never an official JLCPCB price.";

/// The MCP server.
#[derive(Clone)]
pub struct JlcpcbServer {
    service: Service,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl JlcpcbServer {
    /// Create a server around a [`Service`].
    pub fn new(service: Service) -> Self {
        Self {
            service,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Search JLCPCB/LCSC parts live by keyword and filters. Results are ranked Basic first, then Preferred, then Extended, then by JLCPCB assembly stock (desc), then unit price (asc). Example: {\"query\": \"10k\", \"category\": \"Resistors\", \"package\": \"0402\", \"basic_only\": true}.",
        annotations(
            title = "Search components",
            read_only_hint = true,
            open_world_hint = true
        )
    )]
    async fn search_components(
        &self,
        Parameters(args): Parameters<SearchArgs>,
    ) -> Result<Json<SearchResult>, String> {
        self.service
            .search(args)
            .await
            .map(Json)
            .map_err(|e| e.to_string())
    }

    #[tool(
        description = "Full live details for one LCSC part number (e.g. \"C25744\"): JLCPCB assembly stock and price breaks, LCSC retail stock and price breaks, Basic/Preferred/Extended class, parameters, datasheet URL, image, MSL and assembly process.",
        annotations(
            title = "Component details",
            read_only_hint = true,
            open_world_hint = true
        )
    )]
    async fn get_component_details(
        &self,
        Parameters(args): Parameters<DetailsArgs>,
    ) -> Result<Json<ComponentDetails>, String> {
        self.service
            .details(args)
            .await
            .map(Json)
            .map_err(|e| e.to_string())
    }

    #[tool(
        description = "Suggest cheaper, better-stocked or Basic-library substitutes for an LCSC part: same category, same package (by default) and same value (resistance/capacitance/inductance/frequency/output voltage), with ratings at least as good. Each suggestion lists its reasons.",
        annotations(
            title = "Suggest alternatives",
            read_only_hint = true,
            open_world_hint = true
        )
    )]
    async fn suggest_alternatives(
        &self,
        Parameters(args): Parameters<AlternativesArgs>,
    ) -> Result<Json<AlternativesResult>, String> {
        self.service
            .alternatives(args)
            .await
            .map(Json)
            .map_err(|e| e.to_string())
    }

    #[tool(
        description = "List JLCPCB component categories with live part counts. With no arguments returns top-level categories; use `parent` or `search` (e.g. \"mosfet\") to see sub-categories. Names can be passed as `category` to search_components.",
        annotations(
            title = "List categories",
            read_only_hint = true,
            open_world_hint = true
        )
    )]
    async fn list_categories(
        &self,
        Parameters(args): Parameters<CategoriesArgs>,
    ) -> Result<Json<CategoriesResult>, String> {
        self.service
            .categories(args)
            .await
            .map(Json)
            .map_err(|e| e.to_string())
    }

    #[tool(
        description = "ROUGH bare-PCB fabrication cost estimate from size, layers, quantity, thickness and finish. This is an approximate parametric model with documented assumptions, NOT an official JLCPCB quote (JLCPCB has no public pricing API); always confirm at https://cart.jlcpcb.com/quote.",
        annotations(
            title = "Estimate PCB cost (approximate)",
            read_only_hint = true,
            open_world_hint = false
        )
    )]
    async fn estimate_pcb_quote(
        &self,
        Parameters(spec): Parameters<PcbSpec>,
    ) -> Result<Json<PcbEstimate>, String> {
        pcb::estimate(&spec).map(Json).map_err(|e| e.to_string())
    }

    #[tool(
        description = "Check that the live JLCPCB and LCSC endpoints are reachable, whether official API credentials are configured, and cache size. Useful when other tools report network errors.",
        annotations(
            title = "Service status",
            read_only_hint = true,
            open_world_hint = true
        )
    )]
    async fn service_status(&self) -> Json<StatusResult> {
        Json(self.service.status().await)
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for JlcpcbServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("jlcpcb-mcp", env!("CARGO_PKG_VERSION"))
                    .with_title("JLCPCB / LCSC parts"),
            )
            .with_instructions(INSTRUCTIONS)
    }
}

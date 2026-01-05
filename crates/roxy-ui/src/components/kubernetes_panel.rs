//! Kubernetes Panel Component for Roxy UI
//!
//! This component renders a visual graph of Kubernetes traffic flow using a custom
//! node graph element with bezier curve edges connecting the nodes.

use gpui::prelude::*;
use gpui::*;
use std::collections::HashSet;

use crate::components::node_graph::{
    EdgeStyle, GraphEdge, GraphLayout, GraphNode, NodeGraph, NodeGraphBuilder,
    NodeId as GraphNodeId,
};
use crate::theme::{colors, font_size, spacing, Theme};

// =============================================================================
// Data Types
// =============================================================================

/// Kubernetes Ingress resource
#[derive(Debug, Clone)]
pub struct K8sIngress {
    pub name: String,
    pub namespace: String,
    pub hosts: Vec<String>,
    pub ingress_class: Option<String>,
}

/// Kubernetes Gateway resource (Gateway API)
#[derive(Debug, Clone)]
pub struct K8sGateway {
    pub name: String,
    pub namespace: String,
    pub gateway_class: Option<String>,
    pub listeners: Vec<K8sGatewayListener>,
}

/// Gateway listener configuration
#[derive(Debug, Clone)]
pub struct K8sGatewayListener {
    pub name: String,
    pub hostname: Option<String>,
    pub port: u16,
    pub protocol: String,
}

/// Parent reference in an HTTPRoute (points to a Gateway)
#[derive(Debug, Clone)]
pub struct K8sParentRef {
    pub name: String,
    pub namespace: Option<String>,
    pub section_name: Option<String>,
}

/// Kubernetes HTTPRoute resource (Gateway API)
#[derive(Debug, Clone)]
pub struct K8sHttpRoute {
    pub name: String,
    pub namespace: String,
    pub hostnames: Vec<String>,
    pub paths: Vec<String>,
    pub parent_refs: Vec<K8sParentRef>,
    pub backend_refs: Vec<K8sBackendRef>,
}

/// Backend reference in an HTTPRoute
#[derive(Debug, Clone)]
pub struct K8sBackendRef {
    pub service_name: String,
    pub namespace: String,
    pub port: u16,
    pub weight: Option<u32>,
}

/// Kubernetes Service
#[derive(Debug, Clone)]
pub struct K8sService {
    pub name: String,
    pub namespace: String,
    pub service_type: String,
    pub ports: Vec<K8sServicePort>,
    pub ready_endpoints: u32,
    pub total_endpoints: u32,
}

/// Service port definition
#[derive(Debug, Clone)]
pub struct K8sServicePort {
    pub name: Option<String>,
    pub port: u16,
    pub target_port: u16,
    pub protocol: String,
}

/// Port forward from Roxy to a K8s service
#[derive(Debug, Clone)]
pub struct PortForwardInfo {
    pub service_dns: String,
    pub namespace: String,
    pub service_name: String,
    pub remote_port: u16,
    pub local_port: u16,
    pub active: bool,
}

/// HTTPRoute info (legacy - kept for compatibility)
#[derive(Debug, Clone)]
pub struct HttpRouteInfo {
    pub name: String,
    pub namespace: String,
    pub hostnames: Vec<String>,
    pub paths: Vec<String>,
    pub backends: Vec<BackendRef>,
}

/// Backend reference (legacy)
#[derive(Debug, Clone)]
pub struct BackendRef {
    pub name: String,
    pub namespace: Option<String>,
    pub port: u16,
    pub weight: u32,
}

// =============================================================================
// Panel Props
// =============================================================================

/// Properties for the Kubernetes Panel
#[derive(Clone, Default)]
pub struct KubernetesPanelProps {
    pub gateways: Vec<K8sGateway>,
    pub ingresses: Vec<K8sIngress>,
    pub http_routes: Vec<K8sHttpRoute>,
    pub services: Vec<K8sService>,
    pub port_forwards: Vec<PortForwardInfo>,
    pub selected_namespace: Option<String>,
    pub width: f32,
    pub height: f32,
    pub scroll_handle: ScrollHandle,
}

// =============================================================================
// Panel Component
// =============================================================================

/// Kubernetes Panel component
pub struct KubernetesPanel {
    props: KubernetesPanelProps,
    theme: Theme,
}

impl KubernetesPanel {
    pub fn new(props: KubernetesPanelProps) -> Self {
        Self {
            props,
            theme: Theme::dark(),
        }
    }

    pub fn render(&self) -> impl IntoElement {
        let scroll_handle = self.props.scroll_handle.clone();

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(colors::BASE))
            .child(self.render_header())
            .child(
                div()
                    .id("k8s-flow-content")
                    .flex_1()
                    .overflow_scroll()
                    .track_scroll(&scroll_handle)
                    .p(spacing::LG)
                    .child(self.render_flow_diagram()),
            )
    }

    fn render_header(&self) -> impl IntoElement {
        let ns_label = self
            .props
            .selected_namespace
            .as_ref()
            .map(|ns| format!("Namespace: {}", ns))
            .unwrap_or_else(|| "All Namespaces".to_string());

        div()
            .flex()
            .items_center()
            .justify_between()
            .px(spacing::MD)
            .py(spacing::SM)
            .border_b_1()
            .border_color(rgb(colors::SURFACE_0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing::SM)
                    .child(
                        div()
                            .text_size(font_size::LG)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(self.theme.text_primary)
                            .child("Traffic Flow"),
                    )
                    .child(
                        div()
                            .px(spacing::XS)
                            .py(spacing::XXS)
                            .rounded(px(4.0))
                            .bg(rgb(colors::SURFACE_0))
                            .text_size(font_size::SM)
                            .text_color(self.theme.text_secondary)
                            .child(ns_label),
                    ),
            )
            .child(self.render_legend())
    }

    fn render_legend(&self) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(spacing::MD)
            .child(self.render_legend_item("Traffic", colors::SAPPHIRE, false))
            .child(self.render_legend_item("Port Forward", colors::TEAL, true))
    }

    fn render_legend_item(&self, label: &str, color: u32, is_dotted: bool) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(spacing::XS)
            .child(if is_dotted {
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .child(div().w(px(4.0)).h(px(2.0)).bg(rgb(color)))
                    .child(div().w(px(4.0)).h(px(2.0)).bg(rgb(color)))
                    .child(div().w(px(4.0)).h(px(2.0)).bg(rgb(color)))
                    .into_any_element()
            } else {
                div()
                    .w(px(20.0))
                    .h(px(2.0))
                    .rounded(px(1.0))
                    .bg(rgb(color))
                    .into_any_element()
            })
            .child(
                div()
                    .text_size(font_size::XS)
                    .text_color(self.theme.text_muted)
                    .child(label.to_string()),
            )
    }

    fn render_flow_diagram(&self) -> impl IntoElement {
        let has_ingresses = !self.props.ingresses.is_empty();
        let has_routes = !self.props.http_routes.is_empty();
        let has_services = !self.props.services.is_empty();
        let has_port_forwards = !self.props.port_forwards.is_empty();
        let has_data = has_ingresses || has_routes || has_services || has_port_forwards;

        if !has_data {
            return self.render_empty_state().into_any_element();
        }

        // Build the node graph
        let graph = self.build_graph();

        // Render using the node graph component
        crate::components::node_graph::node_graph(graph).into_any_element()
    }

    /// Build the node graph from Kubernetes resources
    ///
    /// Layout columns:
    /// - Column 0: Sources (Internet, Roxy)
    /// - Column 1: Gateways (Gateway API) or Ingresses (legacy)
    /// - Column 2: HTTPRoutes
    /// - Column 3: Services
    fn build_graph(&self) -> NodeGraph {
        let layout = GraphLayout {
            node_width: 180.0,
            node_height: 64.0,
            column_spacing: 100.0,
            row_spacing: 16.0,
            padding: 24.0,
        };

        let mut builder = NodeGraphBuilder::with_layout(layout);
        let mut added_services: HashSet<String> = HashSet::new();
        let mut added_gateways: HashSet<String> = HashSet::new();

        // Determine layout mode based on what data we have
        let has_gateways = !self.props.gateways.is_empty();
        let has_ingresses = !self.props.ingresses.is_empty();
        let has_routes = !self.props.http_routes.is_empty();
        let has_port_forwards = !self.props.port_forwards.is_empty();
        let has_routing = has_gateways || has_ingresses || has_routes;

        // Column layout:
        // - With routing: Internet(0) -> Gateway/Ingress(1) -> HTTPRoute(2) -> Service(3)
        // - Without routing: Internet(0) -> Service(1)
        let service_column = if has_routing { 3 } else { 1 };
        let mut row_counters: [usize; 5] = [0, 0, 0, 0, 0];

        // Column 0: Add Internet node only if we have gateways or ingresses (actual entry points)
        if has_gateways || has_ingresses {
            builder = builder.node(
                GraphNode::new(GraphNodeId::Internet, "Internet", "🌐", colors::SAPPHIRE)
                    .with_sublabel("External Traffic")
                    .at(0, row_counters[0]),
            );
            row_counters[0] += 1;
        }

        // Column 0: Add Roxy node if we have port forwards (always one row below Internet with a gap)
        if has_port_forwards {
            // Skip a row to create visual gap between Internet and Roxy
            row_counters[0] += 1;

            builder = builder.node(
                GraphNode::new(GraphNodeId::Roxy, "Roxy", "🦊", colors::TEAL)
                    .with_sublabel("Port Forwards")
                    .at(0, row_counters[0]),
            );

            // Add dashed edge from Internet to Roxy (always exists when both are present)
            if has_gateways || has_ingresses {
                builder = builder.edge(
                    GraphEdge::new(GraphNodeId::Internet, GraphNodeId::Roxy)
                        .with_style(EdgeStyle::dashed(colors::TEAL)),
                );
            }

            row_counters[0] += 1;
        }

        // Column 1: Gateways (Gateway API)
        for gateway in &self.props.gateways {
            if let Some(ref ns) = self.props.selected_namespace {
                if &gateway.namespace != ns {
                    continue;
                }
            }

            let gateway_id = GraphNodeId::Gateway(gateway.namespace.clone(), gateway.name.clone());
            let gateway_key = gateway_id.key();

            // Build sublabel from listeners
            let sublabel = gateway
                .listeners
                .first()
                .map(|l| l.hostname.clone().unwrap_or_else(|| format!(":{}", l.port)))
                .unwrap_or_else(|| "*".to_string());

            builder = builder.node(
                GraphNode::new(
                    gateway_id.clone(),
                    gateway.name.clone(),
                    "🌉",
                    colors::MAUVE,
                )
                .with_sublabel(sublabel)
                .in_namespace(gateway.namespace.clone())
                .at(1, row_counters[1]),
            );
            added_gateways.insert(gateway_key);

            // Edge from Internet to Gateway
            builder = builder.edge(
                GraphEdge::new(GraphNodeId::Internet, gateway_id)
                    .with_style(EdgeStyle::solid(colors::SAPPHIRE)),
            );

            row_counters[1] += 1;
        }

        // Column 1: Ingresses (legacy - only if no gateways)
        if !has_gateways {
            for ingress in &self.props.ingresses {
                if let Some(ref ns) = self.props.selected_namespace {
                    if &ingress.namespace != ns {
                        continue;
                    }
                }

                let host = ingress
                    .hosts
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "*".to_string());

                builder = builder.node(
                    GraphNode::new(
                        GraphNodeId::Ingress(ingress.name.clone()),
                        ingress.name.clone(),
                        "🚪",
                        colors::MAUVE,
                    )
                    .with_sublabel(host)
                    .in_namespace(ingress.namespace.clone())
                    .at(1, row_counters[1]),
                );

                // Edge from Internet to Ingress
                builder = builder.edge(
                    GraphEdge::new(
                        GraphNodeId::Internet,
                        GraphNodeId::Ingress(ingress.name.clone()),
                    )
                    .with_style(EdgeStyle::solid(colors::SAPPHIRE)),
                );

                row_counters[1] += 1;
            }
        }

        // Column 2: HTTPRoutes
        for route in &self.props.http_routes {
            if let Some(ref ns) = self.props.selected_namespace {
                if &route.namespace != ns {
                    continue;
                }
            }

            let path = route
                .paths
                .first()
                .cloned()
                .unwrap_or_else(|| "/*".to_string());

            let route_id = GraphNodeId::Route(route.namespace.clone(), route.name.clone());

            builder = builder.node(
                GraphNode::new(route_id.clone(), route.name.clone(), "🔀", colors::LAVENDER)
                    .with_sublabel(path)
                    .in_namespace(route.namespace.clone())
                    .at(2, row_counters[2]),
            );

            // Connect HTTPRoute to its parent Gateway(s) via parentRefs
            let mut connected_to_parent = false;
            for parent_ref in &route.parent_refs {
                let parent_ns = parent_ref
                    .namespace
                    .clone()
                    .unwrap_or_else(|| route.namespace.clone());
                let gateway_id = GraphNodeId::Gateway(parent_ns, parent_ref.name.clone());
                let gateway_key = gateway_id.key();

                if added_gateways.contains(&gateway_key) {
                    builder = builder.edge(
                        GraphEdge::new(gateway_id, route_id.clone())
                            .with_style(EdgeStyle::solid(colors::SAPPHIRE)),
                    );
                    connected_to_parent = true;
                }
            }

            // Fallback: connect to Ingress or Internet if no gateway parent found
            if !connected_to_parent {
                if let Some(ingress) = self.props.ingresses.first() {
                    builder = builder.edge(
                        GraphEdge::new(
                            GraphNodeId::Ingress(ingress.name.clone()),
                            route_id.clone(),
                        )
                        .with_style(EdgeStyle::solid(colors::SAPPHIRE)),
                    );
                } else if has_gateways {
                    // Connect to first gateway as fallback
                    if let Some(gateway) = self.props.gateways.first() {
                        builder = builder.edge(
                            GraphEdge::new(
                                GraphNodeId::Gateway(
                                    gateway.namespace.clone(),
                                    gateway.name.clone(),
                                ),
                                route_id.clone(),
                            )
                            .with_style(EdgeStyle::solid(colors::SAPPHIRE)),
                        );
                    }
                } else {
                    builder = builder.edge(
                        GraphEdge::new(GraphNodeId::Internet, route_id.clone())
                            .with_style(EdgeStyle::solid(colors::SAPPHIRE)),
                    );
                }
            }

            // Edges from Route to Services (backendRefs)
            for backend in &route.backend_refs {
                let svc_id =
                    GraphNodeId::Service(backend.namespace.clone(), backend.service_name.clone());
                builder = builder.edge(
                    GraphEdge::new(route_id.clone(), svc_id)
                        .with_label(format!(":{}", backend.port))
                        .with_style(EdgeStyle::solid(colors::SAPPHIRE)),
                );
            }

            row_counters[2] += 1;
        }

        // Column 3 (or 1): Services
        for service in &self.props.services {
            if let Some(ref ns) = self.props.selected_namespace {
                if &service.namespace != ns {
                    continue;
                }
            }

            let svc_id = GraphNodeId::Service(service.namespace.clone(), service.name.clone());
            let port_str = service
                .ports
                .first()
                .map(|p| format!(":{}", p.port))
                .unwrap_or_default();

            let svc_key = svc_id.key();
            builder = builder.node(
                GraphNode::new(svc_id.clone(), service.name.clone(), "📦", colors::GREEN)
                    .with_sublabel(format!("{}{}", service.namespace, port_str))
                    .with_status(service.ready_endpoints, service.total_endpoints)
                    .in_namespace(service.namespace.clone())
                    .at(service_column, row_counters[service_column]),
            );
            added_services.insert(svc_key.clone());

            // Services are only connected via Gateway -> HTTPRoute -> Service flow
            // No direct Internet -> Service connections
            // (Port forwards from Roxy are added separately below)

            row_counters[service_column] += 1;
        }

        // Add port forward edges (from Roxy to Services)
        for pf in &self.props.port_forwards {
            if let Some(ref ns) = self.props.selected_namespace {
                if &pf.namespace != ns {
                    continue;
                }
            }

            let svc_id = GraphNodeId::Service(pf.namespace.clone(), pf.service_name.clone());
            let svc_key = svc_id.key();

            // Ensure service node exists
            if !added_services.contains(&svc_key) {
                builder = builder.node(
                    GraphNode::new(svc_id.clone(), pf.service_name.clone(), "📦", colors::TEAL)
                        .with_sublabel(format!("{}:{}", pf.namespace, pf.remote_port))
                        .in_namespace(pf.namespace.clone())
                        .at(service_column, row_counters[service_column]),
                );
                added_services.insert(svc_key);
                row_counters[service_column] += 1;
            }

            builder = builder.edge(
                GraphEdge::new(GraphNodeId::Roxy, svc_id)
                    .with_label(format!(":{} → :{}", pf.local_port, pf.remote_port))
                    .with_style(EdgeStyle::dashed(colors::TEAL)),
            );
        }

        builder.build()
    }

    fn render_empty_state(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .h(px(300.0))
            .gap(spacing::MD)
            .child(div().text_size(px(48.0)).child("☸"))
            .child(
                div()
                    .text_size(font_size::LG)
                    .text_color(self.theme.text_muted)
                    .child("No Kubernetes resources found"),
            )
            .child(
                div()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_muted)
                    .max_w(px(400.0))
                    .text_center()
                    .child("Select a namespace to view the traffic flow graph."),
            )
    }
}

// =============================================================================
// Convenience Functions
// =============================================================================

/// Convenience function to render a kubernetes panel
pub fn kubernetes_panel(props: KubernetesPanelProps) -> impl IntoElement {
    KubernetesPanel::new(props).render()
}

/// Port forwards list component (legacy)
pub fn port_forwards_list(forwards: &[PortForwardInfo]) -> impl IntoElement {
    let theme = Theme::dark();

    if forwards.is_empty() {
        return div()
            .p(spacing::MD)
            .text_size(font_size::SM)
            .text_color(theme.text_muted)
            .child("No active port forwards")
            .into_any_element();
    }

    div()
        .flex()
        .flex_col()
        .gap(spacing::XS)
        .children(forwards.iter().map(|pf| {
            let status_color = if pf.active {
                rgb(colors::GREEN)
            } else {
                rgb(colors::OVERLAY_0)
            };

            div()
                .flex()
                .items_center()
                .px(spacing::SM)
                .py(spacing::XS)
                .bg(rgb(colors::SURFACE_0))
                .rounded(px(4.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(spacing::SM)
                        .child(div().size(px(8.0)).rounded(px(4.0)).bg(status_color))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(font_size::SM)
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.text_primary)
                                        .child(pf.service_dns.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(font_size::XS)
                                        .text_color(theme.text_muted)
                                        .child(format!(
                                            "localhost:{} → :{}",
                                            pf.local_port, pf.remote_port
                                        )),
                                ),
                        ),
                )
        }))
        .into_any_element()
}

/// HTTP routes list component (legacy)
pub fn http_routes_list(routes: &[HttpRouteInfo]) -> impl IntoElement {
    let theme = Theme::dark();

    if routes.is_empty() {
        return div()
            .p(spacing::MD)
            .text_size(font_size::SM)
            .text_color(theme.text_muted)
            .child("No HTTPRoute resources found")
            .into_any_element();
    }

    div()
        .flex()
        .flex_col()
        .gap(spacing::SM)
        .children(routes.iter().map(|route| {
            div()
                .flex()
                .flex_col()
                .gap(spacing::XXS)
                .p(spacing::SM)
                .bg(rgb(colors::SURFACE_0))
                .rounded(px(4.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(font_size::SM)
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.text_primary)
                                .child(route.name.clone()),
                        )
                        .child(
                            div()
                                .text_size(font_size::XS)
                                .text_color(theme.text_muted)
                                .child(route.namespace.clone()),
                        ),
                )
                .child(
                    div()
                        .text_size(font_size::XS)
                        .text_color(rgb(colors::SAPPHIRE))
                        .child(route.hostnames.join(", ")),
                )
        }))
        .into_any_element()
}

/// Flow diagram component (legacy)
pub fn flow_diagram(props: &KubernetesPanelProps) -> impl IntoElement {
    KubernetesPanel::new(props.clone()).render()
}

//! Kubernetes Panel Component for Roxy UI
//!
//! This component renders a visual overview of Kubernetes resources:
//! - Active port forwards from Roxy to K8s services
//! - HTTPRoute resources showing external traffic routing
//!
//! The visualization is a graph flowing left to right, similar to a mermaid diagram.

use gpui::prelude::*;
use gpui::*;

use crate::theme::{colors, dimensions, font_size, spacing, Theme};

/// Represents a node in the Kubernetes graph
#[derive(Debug, Clone, PartialEq)]
pub enum NodeType {
    /// External traffic source (Internet)
    Internet,
    /// Roxy proxy
    Roxy,
    /// A Kubernetes Gateway
    Gateway { name: String },
    /// A Kubernetes Service
    Service { name: String, namespace: String },
    /// A Kubernetes Pod
    Pod { name: String, namespace: String },
    /// External service (database, etc.)
    External { name: String },
}

impl NodeType {
    pub fn label(&self) -> String {
        match self {
            NodeType::Internet => "Internet".to_string(),
            NodeType::Roxy => "Roxy".to_string(),
            NodeType::Gateway { name } => format!("Gateway\n{}", name),
            NodeType::Service { name, namespace } => format!("{}\n{}", name, namespace),
            NodeType::Pod { name, .. } => name.clone(),
            NodeType::External { name } => name.clone(),
        }
    }

    pub fn short_label(&self) -> String {
        match self {
            NodeType::Internet => "Internet".to_string(),
            NodeType::Roxy => "Roxy".to_string(),
            NodeType::Gateway { name } => name.clone(),
            NodeType::Service { name, .. } => name.clone(),
            NodeType::Pod { name, .. } => name.clone(),
            NodeType::External { name } => name.clone(),
        }
    }
}

/// Represents an edge/connection in the graph
#[derive(Debug, Clone)]
pub struct GraphEdge {
    /// Source node index
    pub from: usize,
    /// Target node index
    pub to: usize,
    /// Label for the edge (e.g., port number, path)
    pub label: Option<String>,
    /// Whether this connection is active
    pub active: bool,
}

/// Represents a port forward from Roxy
#[derive(Debug, Clone)]
pub struct PortForwardInfo {
    /// The K8s service DNS name
    pub service_dns: String,
    /// The namespace
    pub namespace: String,
    /// The service name
    pub service_name: String,
    /// Remote port
    pub remote_port: u16,
    /// Local port
    pub local_port: u16,
    /// Whether currently active
    pub active: bool,
}

/// Represents an HTTPRoute resource
#[derive(Debug, Clone)]
pub struct HttpRouteInfo {
    /// Route name
    pub name: String,
    /// Namespace
    pub namespace: String,
    /// Hostnames this route matches
    pub hostnames: Vec<String>,
    /// Path matches
    pub paths: Vec<String>,
    /// Backend service references
    pub backends: Vec<BackendRef>,
}

/// Backend reference in an HTTPRoute
#[derive(Debug, Clone)]
pub struct BackendRef {
    /// Service name
    pub name: String,
    /// Service namespace (if different from route)
    pub namespace: Option<String>,
    /// Port
    pub port: u16,
    /// Weight for traffic splitting
    pub weight: u32,
}

/// Properties for the Kubernetes Panel
#[derive(Clone, Default)]
pub struct KubernetesPanelProps {
    /// Active port forwards
    pub port_forwards: Vec<PortForwardInfo>,
    /// HTTPRoute resources
    pub http_routes: Vec<HttpRouteInfo>,
    /// Panel width
    pub width: f32,
    /// Panel height
    pub height: f32,
    /// Scroll handle
    pub scroll_handle: ScrollHandle,
}

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
        div()
            .flex()
            .flex_col()
            .w(px(self.props.width))
            .h(px(self.props.height))
            .bg(rgb(colors::BASE))
            .child(self.render_header())
            .child(self.render_content())
    }

    fn render_header(&self) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .px(spacing::MD)
            .py(spacing::SM)
            .border_b_1()
            .border_color(rgb(colors::SURFACE_0))
            .child(
                div().flex().items_center().gap(spacing::SM).child(
                    div()
                        .text_size(font_size::LG)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(self.theme.text_primary)
                        .child("Kubernetes Overview"),
                ),
            )
            .child(
                div()
                    .flex()
                    .gap(spacing::SM)
                    .child(self.render_legend_item("Port Forward", colors::TEAL))
                    .child(self.render_legend_item("HTTPRoute", colors::MAUVE)),
            )
    }

    fn render_legend_item(&self, label: &str, color: u32) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(spacing::XXS)
            .child(div().size(px(8.0)).rounded(px(2.0)).bg(rgb(color)))
            .child(
                div()
                    .text_size(font_size::XS)
                    .text_color(self.theme.text_muted)
                    .child(label.to_string()),
            )
    }

    fn render_content(&self) -> impl IntoElement {
        let scroll_handle = self.props.scroll_handle.clone();

        div()
            .id("k8s-content")
            .flex_1()
            .overflow_scroll()
            .track_scroll(&scroll_handle)
            .p(spacing::MD)
            .child(self.render_graph())
    }

    fn render_graph(&self) -> impl IntoElement {
        // Build the graph layout
        let mut columns: Vec<Vec<GraphNode>> = vec![vec![], vec![], vec![]];

        // Column 0: Sources (Internet, Roxy)
        if !self.props.http_routes.is_empty() {
            columns[0].push(GraphNode {
                node_type: NodeType::Internet,
                color: colors::SAPPHIRE,
            });
        }

        if !self.props.port_forwards.is_empty() {
            columns[0].push(GraphNode {
                node_type: NodeType::Roxy,
                color: colors::TEAL,
            });
        }

        // Column 1: Gateways (for HTTPRoutes) - we can skip this for simplicity
        // Column 2: Services

        // Add port forward targets
        for pf in &self.props.port_forwards {
            columns[2].push(GraphNode {
                node_type: NodeType::Service {
                    name: pf.service_name.clone(),
                    namespace: pf.namespace.clone(),
                },
                color: colors::TEAL,
            });
        }

        // Add HTTPRoute backends
        for route in &self.props.http_routes {
            for backend in &route.backends {
                let namespace = backend
                    .namespace
                    .clone()
                    .unwrap_or_else(|| route.namespace.clone());

                // Check if already added
                let exists = columns[2].iter().any(|n| {
                    matches!(&n.node_type, NodeType::Service { name, namespace: ns }
                        if name == &backend.name && ns == &namespace)
                });

                if !exists {
                    columns[2].push(GraphNode {
                        node_type: NodeType::Service {
                            name: backend.name.clone(),
                            namespace,
                        },
                        color: colors::MAUVE,
                    });
                }
            }
        }

        // Render the graph
        div()
            .flex()
            .gap(px(80.0))
            .items_start()
            .children(
                columns
                    .iter()
                    .enumerate()
                    .map(|(col_idx, nodes)| self.render_column(col_idx, nodes)),
            )
            .child(self.render_connections())
    }

    fn render_column(&self, _col_idx: usize, nodes: &[GraphNode]) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(spacing::LG)
            .children(nodes.iter().map(|node| self.render_node(node)))
    }

    fn render_node(&self, node: &GraphNode) -> impl IntoElement {
        let (icon, label, sublabel) = match &node.node_type {
            NodeType::Internet => ("🌐", "Internet".to_string(), None),
            NodeType::Roxy => ("🦊", "Roxy".to_string(), Some("SOCKS5 :1080".to_string())),
            NodeType::Gateway { name } => ("🚪", name.clone(), Some("Gateway".to_string())),
            NodeType::Service { name, namespace } => ("📦", name.clone(), Some(namespace.clone())),
            NodeType::Pod { name, namespace } => ("🔷", name.clone(), Some(namespace.clone())),
            NodeType::External { name } => ("🔗", name.clone(), None),
        };

        div()
            .flex()
            .items_center()
            .gap(spacing::SM)
            .px(spacing::MD)
            .py(spacing::SM)
            .min_w(px(150.0))
            .bg(rgb(colors::SURFACE_0))
            .border_1()
            .border_color(rgb(node.color))
            .rounded(dimensions::BORDER_RADIUS)
            .child(div().text_size(font_size::XL).child(icon.to_string()))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(font_size::MD)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(self.theme.text_primary)
                            .child(label),
                    )
                    .when_some(sublabel, |this, sub| {
                        this.child(
                            div()
                                .text_size(font_size::XS)
                                .text_color(self.theme.text_muted)
                                .child(sub),
                        )
                    }),
            )
    }

    fn render_connections(&self) -> impl IntoElement {
        // Render connection lines as a separate layer
        // In GPUI, we'll represent connections as text labels between nodes
        // A full SVG-based graph would require more complex rendering

        div().absolute().top_0().left_0().w_full().h_full().child(
            div()
                .flex()
                .flex_col()
                .gap(spacing::XS)
                .p(spacing::SM)
                .children(self.props.port_forwards.iter().map(|pf| {
                    self.render_connection_label(
                        "Roxy",
                        &format!("{}.{}", pf.service_name, pf.namespace),
                        &format!(":{} → :{}", pf.local_port, pf.remote_port),
                        colors::TEAL,
                        pf.active,
                    )
                }))
                .children(self.props.http_routes.iter().flat_map(|route| {
                    route.backends.iter().map(|backend| {
                        let ns = backend
                            .namespace
                            .clone()
                            .unwrap_or_else(|| route.namespace.clone());
                        let paths = route.paths.join(", ");
                        self.render_connection_label(
                            &route.hostnames.join(", "),
                            &format!("{}.{}", backend.name, ns),
                            &format!("{} → :{}", paths, backend.port),
                            colors::MAUVE,
                            true,
                        )
                    })
                })),
        )
    }

    fn render_connection_label(
        &self,
        from: &str,
        to: &str,
        label: &str,
        color: u32,
        active: bool,
    ) -> impl IntoElement {
        let opacity = if active { 1.0 } else { 0.5 };

        div()
            .flex()
            .items_center()
            .gap(spacing::SM)
            .opacity(opacity)
            .child(
                div()
                    .text_size(font_size::SM)
                    .text_color(self.theme.text_secondary)
                    .child(from.to_string()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing::XXS)
                    .child(div().w(px(40.0)).h(px(2.0)).bg(rgb(color)))
                    .child(
                        div()
                            .text_size(font_size::XS)
                            .text_color(rgb(color))
                            .child(label.to_string()),
                    )
                    .child(div().w(px(20.0)).h(px(2.0)).bg(rgb(color)))
                    .child(
                        div()
                            .text_size(font_size::MD)
                            .text_color(rgb(color))
                            .child("→"),
                    ),
            )
            .child(
                div()
                    .text_size(font_size::SM)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(self.theme.text_primary)
                    .child(to.to_string()),
            )
    }
}

/// Internal graph node representation
#[derive(Clone)]
struct GraphNode {
    node_type: NodeType,
    color: u32,
}

/// Convenience function to render the Kubernetes panel
pub fn kubernetes_panel(props: KubernetesPanelProps) -> impl IntoElement {
    KubernetesPanel::new(props).render()
}

/// Section component for grouping related items
pub struct KubernetesSection {
    title: String,
    theme: Theme,
}

impl KubernetesSection {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            theme: Theme::dark(),
        }
    }

    pub fn render(&self, children: impl IntoElement) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(spacing::SM)
            .child(
                div()
                    .text_size(font_size::SM)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(self.theme.text_muted)
                    .child(self.title.clone()),
            )
            .child(children)
    }
}

/// Port forwards list component
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
                .justify_between()
                .px(spacing::SM)
                .py(spacing::XS)
                .bg(rgb(colors::SURFACE_0))
                .rounded(dimensions::BORDER_RADIUS)
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

/// HTTP routes list component
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
                .rounded(dimensions::BORDER_RADIUS)
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
                .child(div().flex().flex_col().gap(spacing::XXXS).children(
                    route.backends.iter().map(|backend| {
                        div()
                            .flex()
                            .items_center()
                            .gap(spacing::XS)
                            .child(
                                div()
                                    .text_size(font_size::XS)
                                    .text_color(theme.text_muted)
                                    .child("→"),
                            )
                            .child(
                                div()
                                    .text_size(font_size::XS)
                                    .text_color(theme.text_secondary)
                                    .child(format!("{}:{}", backend.name, backend.port)),
                            )
                    }),
                ))
        }))
        .into_any_element()
}

/// Flow diagram component - renders a left-to-right flow
pub fn flow_diagram(props: &KubernetesPanelProps) -> impl IntoElement {
    let theme = Theme::dark();

    div()
        .flex()
        .flex_col()
        .gap(spacing::LG)
        // Port forwards section
        .when(!props.port_forwards.is_empty(), |this| {
            this.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(spacing::SM)
                    .child(
                        div()
                            .text_size(font_size::SM)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text_muted)
                            .child("PORT FORWARDS"),
                    )
                    .children(props.port_forwards.iter().map(|pf| {
                        render_flow_row(
                            "🦊 Roxy",
                            &format!("📦 {}", pf.service_dns),
                            &format!("SOCKS5 :{} → :{}", pf.local_port, pf.remote_port),
                            colors::TEAL,
                            pf.active,
                        )
                    })),
            )
        })
        // HTTP routes section
        .when(!props.http_routes.is_empty(), |this| {
            this.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(spacing::SM)
                    .child(
                        div()
                            .text_size(font_size::SM)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text_muted)
                            .child("HTTP ROUTES"),
                    )
                    .children(props.http_routes.iter().flat_map(|route| {
                        route.backends.iter().map(|backend| {
                            let ns = backend
                                .namespace
                                .clone()
                                .unwrap_or_else(|| route.namespace.clone());
                            render_flow_row(
                                &format!(
                                    "🌐 {}",
                                    route.hostnames.first().unwrap_or(&"*".to_string())
                                ),
                                &format!("📦 {}.{}", backend.name, ns),
                                &format!("{} → :{}", route.paths.join(", "), backend.port),
                                colors::MAUVE,
                                true,
                            )
                        })
                    })),
            )
        })
        // Empty state
        .when(
            props.port_forwards.is_empty() && props.http_routes.is_empty(),
            |this| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .h(px(100.0))
                        .text_size(font_size::SM)
                        .text_color(theme.text_muted)
                        .child("No Kubernetes resources detected"),
                )
            },
        )
}

fn render_flow_row(
    from: &str,
    to: &str,
    label: &str,
    color: u32,
    active: bool,
) -> impl IntoElement {
    let theme = Theme::dark();
    let opacity = if active { 1.0 } else { 0.5 };

    div()
        .flex()
        .items_center()
        .gap(spacing::MD)
        .opacity(opacity)
        .px(spacing::SM)
        .py(spacing::XS)
        .bg(rgb(colors::SURFACE_0))
        .rounded(dimensions::BORDER_RADIUS)
        // From node
        .child(
            div()
                .flex()
                .items_center()
                .min_w(px(120.0))
                .px(spacing::SM)
                .py(spacing::XS)
                .bg(rgb(colors::MANTLE))
                .border_1()
                .border_color(rgb(color))
                .rounded(dimensions::BORDER_RADIUS)
                .child(
                    div()
                        .text_size(font_size::SM)
                        .text_color(theme.text_primary)
                        .child(from.to_string()),
                ),
        )
        // Arrow with label
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(spacing::XXXS)
                .child(
                    div()
                        .text_size(font_size::XS)
                        .text_color(rgb(color))
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .child(div().w(px(60.0)).h(px(2.0)).bg(rgb(color)))
                        .child(
                            div()
                                .text_size(font_size::MD)
                                .text_color(rgb(color))
                                .child("→"),
                        ),
                ),
        )
        // To node
        .child(
            div()
                .flex()
                .items_center()
                .min_w(px(200.0))
                .px(spacing::SM)
                .py(spacing::XS)
                .bg(rgb(colors::MANTLE))
                .border_1()
                .border_color(rgb(color))
                .rounded(dimensions::BORDER_RADIUS)
                .child(
                    div()
                        .text_size(font_size::SM)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.text_primary)
                        .child(to.to_string()),
                ),
        )
}

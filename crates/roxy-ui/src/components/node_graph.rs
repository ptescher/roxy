//! Node Graph Component for GPUI
//!
//! A component that renders a node graph with bezier curve edges connecting nodes.
//! Uses div-based rendering with absolute positioning for proper alignment.

use gpui::prelude::*;
use gpui::*;
use std::collections::HashMap;

use crate::theme::{colors, font_size, spacing};

// =============================================================================
// Graph Data Types
// =============================================================================

/// Unique identifier for a node in the graph
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NodeId {
    /// External traffic source
    Internet,
    /// Local Roxy instance
    Roxy,
    /// Kubernetes Gateway (namespace, name)
    Gateway(String, String),
    /// Kubernetes Ingress
    Ingress(String),
    /// HTTPRoute (namespace, name)
    Route(String, String),
    /// Kubernetes Service (namespace, name)
    Service(String, String),
    /// Custom node type
    Custom(String),
}

impl NodeId {
    /// Get a unique string key for this node
    pub fn key(&self) -> String {
        match self {
            NodeId::Internet => "internet".to_string(),
            NodeId::Roxy => "roxy".to_string(),
            NodeId::Gateway(ns, name) => format!("gateway:{}:{}", ns, name),
            NodeId::Ingress(name) => format!("ingress:{}", name),
            NodeId::Route(ns, name) => format!("route:{}:{}", ns, name),
            NodeId::Service(ns, name) => format!("service:{}:{}", ns, name),
            NodeId::Custom(id) => format!("custom:{}", id),
        }
    }
}

/// A node in the graph
#[derive(Debug, Clone)]
pub struct GraphNode {
    /// Unique identifier
    pub id: NodeId,
    /// Display label
    pub label: String,
    /// Secondary label (e.g., hostname, port)
    pub sublabel: Option<String>,
    /// Icon emoji or character
    pub icon: String,
    /// Primary color for the node border/accent
    pub color: u32,
    /// Status as (ready, total) - for services
    pub status: Option<(u32, u32)>,
    /// Column index for layout (0 = leftmost)
    pub column: usize,
    /// Row index within the column
    pub row: usize,
    /// Optional namespace grouping
    pub namespace: Option<String>,
}

impl GraphNode {
    /// Create a new graph node
    pub fn new(id: NodeId, label: impl Into<String>, icon: impl Into<String>, color: u32) -> Self {
        Self {
            id,
            label: label.into(),
            sublabel: None,
            icon: icon.into(),
            color,
            status: None,
            column: 0,
            row: 0,
            namespace: None,
        }
    }

    /// Set the sublabel
    pub fn with_sublabel(mut self, sublabel: impl Into<String>) -> Self {
        self.sublabel = Some(sublabel.into());
        self
    }

    /// Set the status
    pub fn with_status(mut self, ready: u32, total: u32) -> Self {
        self.status = Some((ready, total));
        self
    }

    /// Set the position
    pub fn at(mut self, column: usize, row: usize) -> Self {
        self.column = column;
        self.row = row;
        self
    }

    /// Set the namespace
    pub fn in_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }
}

/// Style for an edge
#[derive(Debug, Clone)]
pub struct EdgeStyle {
    /// Line color
    pub color: u32,
    /// Whether the line is dashed
    pub dashed: bool,
}

impl Default for EdgeStyle {
    fn default() -> Self {
        Self {
            color: colors::OVERLAY_1,
            dashed: false,
        }
    }
}

impl EdgeStyle {
    /// Create a solid edge style
    pub fn solid(color: u32) -> Self {
        Self {
            color,
            dashed: false,
        }
    }

    /// Create a dashed edge style
    pub fn dashed(color: u32) -> Self {
        Self {
            color,
            dashed: true,
        }
    }
}

/// An edge connecting two nodes
#[derive(Debug, Clone)]
pub struct GraphEdge {
    /// Source node ID
    pub from: NodeId,
    /// Target node ID
    pub to: NodeId,
    /// Optional label on the edge
    pub label: Option<String>,
    /// Edge style
    pub style: EdgeStyle,
}

impl GraphEdge {
    /// Create a new edge
    pub fn new(from: NodeId, to: NodeId) -> Self {
        Self {
            from,
            to,
            label: None,
            style: EdgeStyle::default(),
        }
    }

    /// Set the edge label
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Set the edge style
    pub fn with_style(mut self, style: EdgeStyle) -> Self {
        self.style = style;
        self
    }
}

/// Layout configuration for the graph
#[derive(Debug, Clone)]
pub struct GraphLayout {
    /// Width of each node
    pub node_width: f32,
    /// Height of each node
    pub node_height: f32,
    /// Horizontal spacing between columns
    pub column_spacing: f32,
    /// Vertical spacing between rows
    pub row_spacing: f32,
    /// Padding around the graph
    pub padding: f32,
}

impl Default for GraphLayout {
    fn default() -> Self {
        Self {
            node_width: 180.0,
            node_height: 64.0,
            column_spacing: 80.0,
            row_spacing: 16.0,
            padding: 24.0,
        }
    }
}

// =============================================================================
// Node Graph Container
// =============================================================================

/// The main node graph container
#[derive(Debug, Clone, Default)]
pub struct NodeGraph {
    /// All nodes in the graph
    pub nodes: HashMap<String, GraphNode>,
    /// All edges in the graph
    pub edges: Vec<GraphEdge>,
    /// Layout configuration
    pub layout: GraphLayout,
}

impl NodeGraph {
    /// Create a new empty graph
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a graph with custom layout
    pub fn with_layout(layout: GraphLayout) -> Self {
        Self {
            layout,
            ..Default::default()
        }
    }

    /// Add a node to the graph
    pub fn add_node(&mut self, node: GraphNode) {
        self.nodes.insert(node.id.key(), node);
    }

    /// Add an edge to the graph
    pub fn add_edge(&mut self, edge: GraphEdge) {
        self.edges.push(edge);
    }

    /// Get a node by ID
    pub fn get_node(&self, id: &NodeId) -> Option<&GraphNode> {
        self.nodes.get(&id.key())
    }

    /// Check if graph is empty
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Get the number of columns in the graph
    pub fn column_count(&self) -> usize {
        self.nodes
            .values()
            .map(|n| n.column)
            .max()
            .map(|c| c + 1)
            .unwrap_or(0)
    }

    /// Get max rows per column
    pub fn max_rows(&self) -> usize {
        self.nodes
            .values()
            .map(|n| n.row)
            .max()
            .map(|r| r + 1)
            .unwrap_or(0)
    }

    /// Calculate total width needed
    pub fn total_width(&self) -> f32 {
        let cols = self.column_count();
        if cols == 0 {
            return 0.0;
        }
        self.layout.padding * 2.0
            + (cols as f32) * self.layout.node_width
            + ((cols.saturating_sub(1)) as f32) * self.layout.column_spacing
    }

    /// Calculate total height needed
    pub fn total_height(&self) -> f32 {
        let rows = self.max_rows();
        if rows == 0 {
            return 0.0;
        }
        self.layout.padding * 2.0
            + (rows as f32) * self.layout.node_height
            + ((rows.saturating_sub(1)) as f32) * self.layout.row_spacing
    }
}

// =============================================================================
// Node Graph Rendering Functions
// =============================================================================

/// Render a complete node graph
pub fn node_graph(graph: NodeGraph) -> impl IntoElement {
    if graph.is_empty() {
        return div()
            .flex()
            .items_center()
            .justify_center()
            .h(px(200.0))
            .child(
                div()
                    .text_size(font_size::SM)
                    .text_color(rgb(colors::SUBTEXT_0))
                    .child("No resources to display"),
            )
            .into_any_element();
    }

    let total_width = graph.total_width();
    let total_height = graph.total_height();

    // Build the graph content using absolute positioning for everything
    // This ensures edges and nodes are in the same coordinate system
    div()
        .id("node-graph-content")
        .relative()
        .w(px(total_width.max(600.0)))
        .min_h(px(total_height.max(300.0)))
        // Render edges first (behind nodes)
        .children(render_edges(&graph))
        // Render nodes with absolute positioning
        .children(render_nodes(&graph))
        .into_any_element()
}

/// Render all nodes with absolute positioning
fn render_nodes(graph: &NodeGraph) -> Vec<AnyElement> {
    let layout = &graph.layout;

    graph
        .nodes
        .values()
        .map(|node| {
            // Calculate absolute position
            let x =
                layout.padding + (node.column as f32) * (layout.node_width + layout.column_spacing);
            let y = layout.padding + (node.row as f32) * (layout.node_height + layout.row_spacing);

            render_node_at(graph, node, x, y).into_any_element()
        })
        .collect()
}

/// Render a single node at an absolute position
fn render_node_at(graph: &NodeGraph, node: &GraphNode, x: f32, y: f32) -> impl IntoElement {
    let border_color = Hsla::from(rgb(node.color)).opacity(0.6);
    let layout = &graph.layout;

    let status_indicator = node.status.map(|(ready, total)| {
        let color = if ready > 0 {
            rgb(colors::GREEN)
        } else {
            rgb(colors::RED)
        };
        (color, format!("{}/{}", ready, total))
    });

    div()
        .id(ElementId::Name(node.id.key().into()))
        .absolute()
        .left(px(x))
        .top(px(y))
        .w(px(layout.node_width))
        .h(px(layout.node_height))
        .flex()
        .items_center()
        .gap(spacing::XS)
        .px(spacing::SM)
        .py(spacing::XS)
        .rounded(px(8.0))
        .bg(rgb(colors::SURFACE_0))
        .border_1()
        .border_color(border_color)
        .shadow_sm()
        .child(
            div()
                .text_size(px(20.0))
                .flex_shrink_0()
                .child(node.icon.clone()),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .overflow_hidden()
                .child(
                    div()
                        .text_size(font_size::SM)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(colors::TEXT))
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(node.label.clone()),
                )
                .when_some(node.sublabel.clone(), |el, sub| {
                    el.child(
                        div()
                            .text_size(font_size::XS)
                            .text_color(rgb(colors::SUBTEXT_0))
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(sub),
                    )
                })
                .when_some(status_indicator, |el, (color, text)| {
                    el.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(spacing::XXXS)
                            .mt(px(2.0))
                            .child(div().size(px(6.0)).rounded(px(3.0)).bg(color))
                            .child(
                                div()
                                    .text_size(font_size::XS)
                                    .text_color(rgb(colors::SUBTEXT_1))
                                    .child(text),
                            ),
                    )
                }),
        )
}

/// Render edge connectors between nodes
fn render_edges(graph: &NodeGraph) -> Vec<AnyElement> {
    let mut elements = Vec::new();
    let layout = &graph.layout;

    for edge in &graph.edges {
        let from_node = match graph.get_node(&edge.from) {
            Some(n) => n,
            None => continue,
        };
        let to_node = match graph.get_node(&edge.to) {
            Some(n) => n,
            None => continue,
        };

        // Only render edges going forward (left to right)
        if from_node.column >= to_node.column {
            continue;
        }

        // Calculate from position (right edge of source node, vertically centered)
        // X = padding + (column * (node_width + spacing)) + node_width
        // Y = padding + (row * (node_height + row_spacing)) + node_height/2
        let from_x = layout.padding
            + (from_node.column as f32) * (layout.node_width + layout.column_spacing)
            + layout.node_width;
        let from_y = layout.padding
            + (from_node.row as f32) * (layout.node_height + layout.row_spacing)
            + layout.node_height / 2.0;

        // Calculate to position (left edge of target node, vertically centered)
        let to_x =
            layout.padding + (to_node.column as f32) * (layout.node_width + layout.column_spacing);
        let to_y = layout.padding
            + (to_node.row as f32) * (layout.node_height + layout.row_spacing)
            + layout.node_height / 2.0;

        // Create a visual edge connector
        let edge_element = render_edge_line(from_x, from_y, to_x, to_y, &edge.style);
        elements.push(edge_element.into_any_element());
    }

    elements
}

/// A bezier curve edge element that uses PathBuilder for smooth rendering
struct BezierEdge {
    from: Point<Pixels>,
    to: Point<Pixels>,
    ctrl1: Point<Pixels>,
    ctrl2: Point<Pixels>,
    color: Hsla,
    line_width: Pixels,
    dashed: bool,
}

impl IntoElement for BezierEdge {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for BezierEdge {
    type RequestLayoutState = ();
    type PrepaintState = Point<Pixels>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let style = Style {
            position: Position::Absolute,
            ..Default::default()
        };
        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
        // Capture the bounds origin for coordinate transformation
        bounds.origin
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        _cx: &mut App,
    ) {
        // Apply bounds origin to transform local coordinates to window coordinates
        let offset = *prepaint;
        let from = self.from + offset;
        let to = self.to + offset;
        let ctrl1 = self.ctrl1 + offset;
        let ctrl2 = self.ctrl2 + offset;

        let mut path_builder = PathBuilder::stroke(self.line_width);
        if self.dashed {
            path_builder = path_builder.dash_array(&[px(8.0), px(4.0)]);
        }
        path_builder.move_to(from);
        path_builder.cubic_bezier_to(to, ctrl1, ctrl2);

        if let Ok(path) = path_builder.build() {
            window.paint_path(path, self.color);
        }
    }
}

/// Render a single edge line using a proper bezier curve with an arrow at the end
fn render_edge_line(
    from_x: f32,
    from_y: f32,
    to_x: f32,
    to_y: f32,
    style: &EdgeStyle,
) -> impl IntoElement {
    // Calculate bezier control points for a smooth horizontal curve
    let dx = to_x - from_x;
    let ctrl_offset = (dx.abs() * 0.5).max(30.0);
    let ctrl1_x = from_x + ctrl_offset;
    let ctrl1_y = from_y;
    let ctrl2_x = to_x - ctrl_offset;
    let ctrl2_y = to_y;

    let color = rgb(style.color);
    // Dashed (roxy connections) use 80% opacity, solid uses full opacity
    let line_color = if style.dashed {
        Hsla::from(color).opacity(0.8)
    } else {
        Hsla::from(color)
    };

    let line_width = px(3.0);

    let bezier_edge = BezierEdge {
        from: point(px(from_x), px(from_y)),
        to: point(px(to_x), px(to_y)),
        ctrl1: point(px(ctrl1_x), px(ctrl1_y)),
        ctrl2: point(px(ctrl2_x), px(ctrl2_y)),
        color: line_color,
        line_width,
        dashed: style.dashed,
    };

    // Add arrow head at the end (pointing right) using a text character
    let arrow = div()
        .absolute()
        .left(px(to_x - 12.0))
        .top(px(to_y - 6.0))
        .text_size(px(12.0))
        .text_color(color)
        .child("▶")
        .into_any_element();

    div()
        .absolute()
        .top(px(0.0))
        .left(px(0.0))
        .size_full()
        .child(bezier_edge)
        .child(arrow)
}

// =============================================================================
// Builder
// =============================================================================

/// Builder for creating node graphs
pub struct NodeGraphBuilder {
    graph: NodeGraph,
}

impl NodeGraphBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            graph: NodeGraph::new(),
        }
    }

    /// Create a builder with custom layout
    pub fn with_layout(layout: GraphLayout) -> Self {
        Self {
            graph: NodeGraph::with_layout(layout),
        }
    }

    /// Add a node
    pub fn node(mut self, node: GraphNode) -> Self {
        self.graph.add_node(node);
        self
    }

    /// Add an edge
    pub fn edge(mut self, edge: GraphEdge) -> Self {
        self.graph.add_edge(edge);
        self
    }

    /// Build the graph
    pub fn build(self) -> NodeGraph {
        self.graph
    }
}

impl Default for NodeGraphBuilder {
    fn default() -> Self {
        Self::new()
    }
}

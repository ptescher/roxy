//! UI Components for Roxy
//!
//! This module contains reusable UI components that make up
//! the Roxy application interface.

mod about_dialog;
mod connection_detail;
mod connection_list;
mod detail_panel;
mod kubernetes_panel;
mod left_dock;
mod node_graph;
mod request_list;
mod request_row;
mod sidebar;
mod status_bar;
mod title_bar;
mod toolbar;

pub use about_dialog::open_about_window;
pub use connection_detail::{
    connection_detail_panel, ConnectionDetailPanel, ConnectionDetailPanelProps, ConnectionDetailTab,
};
pub use connection_list::{connection_list, ConnectionList, ConnectionListProps};
pub use detail_panel::{DetailPanel, DetailPanelProps, DetailTab};
pub use kubernetes_panel::{
    flow_diagram, http_routes_list, kubernetes_panel, port_forwards_list, BackendRef,
    HttpRouteInfo, K8sBackendRef, K8sGateway, K8sGatewayListener, K8sHttpRoute, K8sIngress,
    K8sParentRef, K8sService, K8sServicePort, KubernetesPanel, KubernetesPanelProps,
    PortForwardInfo,
};
pub use left_dock::{
    left_dock, KubeContext, KubeNamespace, LeftDock, LeftDockProps, LeftDockTab, ServiceSummary,
};
pub use node_graph::{
    node_graph, EdgeStyle, GraphEdge, GraphLayout, GraphNode, NodeGraph, NodeGraphBuilder, NodeId,
};
pub use request_list::{RequestList, RequestListProps};
pub use sidebar::{Sidebar, SidebarProps};
pub use status_bar::{StatusBar, StatusBarProps};
pub use title_bar::{TitleBar, TitleBarProps};
pub use toolbar::{Toolbar, ToolbarProps};

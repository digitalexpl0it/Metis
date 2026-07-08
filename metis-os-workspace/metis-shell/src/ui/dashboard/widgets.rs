//! Built-in Control Center widget registry (v1).
//!
//! Each entry maps a `DashboardWidgetId` to its settings label. Future script or
//! plugin widgets can extend this table without changing the overview layout code.

#![allow(dead_code)]

use metis_config::DashboardWidgetId;

#[derive(Debug, Clone, Copy)]
pub struct WidgetDef {
    pub id: DashboardWidgetId,
    pub title: &'static str,
}

/// All built-in widgets in default display order.
pub const BUILTIN: &[WidgetDef] = &[
    WidgetDef {
        id: DashboardWidgetId::Cpu,
        title: "Processor",
    },
    WidgetDef {
        id: DashboardWidgetId::Memory,
        title: "Memory",
    },
    WidgetDef {
        id: DashboardWidgetId::Disk,
        title: "Storage",
    },
    WidgetDef {
        id: DashboardWidgetId::Network,
        title: "Network",
    },
    WidgetDef {
        id: DashboardWidgetId::Processes,
        title: "Processes",
    },
];

pub fn def_for(id: DashboardWidgetId) -> Option<&'static WidgetDef> {
    BUILTIN.iter().find(|w| w.id == id)
}

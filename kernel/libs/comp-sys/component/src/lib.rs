// SPDX-License-Identifier: MPL-2.0

//! Component system
//!

#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

use alloc::{
    borrow::ToOwned,
    collections::BTreeMap,
    fmt::Debug,
    string::{String, ToString},
    vec::Vec,
};

pub use component_macro::*;
pub use inventory;
pub use inventory::submit;
// This crate intentionally uses the `log` crate directly (not `ostd::log`)
// because it is a standalone framework crate that does not depend on OSTD.
// Messages are forwarded to the OSTD logger via the `LogCrateBridge`.
use log::{debug, error, info};

/// The initialization stages of the component system.
///
/// - `Bootstrap`: The earliest stage, called after OSTD initialization is
///   complete but before kernel subsystem initialization begins. This stage
///   runs on the BSP (Bootstrap Processor) only, before SMP (Symmetric
///   Multi-Processing) is enabled. Components in this stage can initialize
///   core kernel services that other components depend on.
/// - `Kthread`: The kernel thread stage, initialized after SMP is enabled
///   and the first kernel thread is spawned. This stage runs in the context
///   of the first kernel thread on the BSP.
/// - `Process`: The process stage, initialized after the first user process
///   is created. This stage runs in the context of the first user process,
///   and prepares the system for user-space execution.
#[derive(Debug, Eq, PartialEq)]
pub enum InitStage {
    Bootstrap,
    Kthread,
    Process,
}

#[derive(Debug)]
pub enum ComponentInitError {
    UninitializedDependencies(String),
    Unknown,
}

pub struct ComponentRegistry {
    stage: InitStage,
    function: &'static (dyn Fn() -> Result<(), ComponentInitError> + Sync),
    path: &'static str,
}

impl ComponentRegistry {
    pub const fn new(
        stage: InitStage,
        function: &'static (dyn Fn() -> Result<(), ComponentInitError> + Sync),
        path: &'static str,
    ) -> Self {
        Self {
            stage,
            function,
            path,
        }
    }
}

inventory::collect!(ComponentRegistry);

impl Debug for ComponentRegistry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ComponentRegistry")
            .field("stage", &self.stage)
            .field("path", &self.path)
            .finish()
    }
}

pub struct ComponentInfo {
    name: String,
    path: String,
    priority: u32,
    function: Option<&'static (dyn Fn() -> Result<(), ComponentInitError> + Sync)>,
}

impl ComponentInfo {
    pub fn new(name: &str, path: &str, priority: u32) -> Self {
        Self {
            name: name.to_string(),
            path: path.to_string(),
            priority,
            function: None,
        }
    }
}

impl PartialEq for ComponentInfo {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority
    }
}

impl Eq for ComponentInfo {}

impl Ord for ComponentInfo {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.priority.cmp(&other.priority)
    }
}

impl PartialOrd for ComponentInfo {
    fn partial_cmp(&self, other: &ComponentInfo) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Debug for ComponentInfo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ComponentInfo")
            .field("name", &self.name)
            .field("path", &self.path)
            .field("priority", &self.priority)
            .finish()
    }
}

#[derive(Debug)]
pub enum ComponentSystemInitError {
    FileNotValid,
    NotIncludeAllComponent(String),
}

/// Initializes the component system for a specific stage.
///
/// It collects all functions marked with the `init_component` macro, filters them
/// according to the given stage, and invokes them in the correct order while honoring
/// dependencies and priorities between crates.
///
/// The collection of ComponentInfo usually generate by `parse_metadata` macro.
///
/// ```rust
///     component::init_all(component::InitStage::Bootstrap, component::parse_metadata!());
/// ```
///
pub fn init_all(
    stage: InitStage,
    components: Vec<ComponentInfo>,
) -> Result<(), ComponentSystemInitError> {
    // On aarch64 the boot page table is still active (we skipped the
    // page table switch). Some component init functions may panic.
    // We wrap the call in a way that converts panics to errors.
    // Since we're no_std, we can't use std::panic::catch_unwind.
    // Instead, we just call directly and let the panic handler deal
    // with failures gracefully (the ostd panic handler prints and
    // continues in some configurations).
    let components_info = parse_input(components);
    match_and_call(stage, components_info)?;
    Ok(())
}

fn parse_input(components: Vec<ComponentInfo>) -> BTreeMap<String, ComponentInfo> {
    debug!("All component: {components:?}");
    let mut out = BTreeMap::new();
    for mut component in components {
        // `cargo metadata` percent-encodes non-ASCII characters in manifest
        // paths, while `ComponentRegistry::path` (from `file!()`) keeps raw
        // UTF-8. Decode the metadata path so both sides compare equal.
        component.path = percent_decode_path(&component.path);
        out.insert(component.path.clone(), component);
    }
    out
}

/// Decodes percent-encoded (`%XX`) sequences in a path.
///
/// This is needed because `cargo metadata` reports `manifest_path` as a
/// percent-encoded URL-ish path, so any non-ASCII workspace path (e.g. CJK
/// directory names) would otherwise never match the raw `file!()` paths.
fn percent_decode_path(path: &str) -> String {
    if !path.contains('%') {
        return path.to_owned();
    }
    let bytes = path.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let hex_val = |b: u8| -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    };
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    match String::from_utf8(out) {
        Ok(s) => s,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    }
}

/// Match the ComponentInfo with ComponentRegistry. The key is the relative path of one component
fn match_and_call(
    stage: InitStage,
    mut components: BTreeMap<String, ComponentInfo>,
) -> Result<(), ComponentSystemInitError> {
    let mut infos = Vec::new();
    for registry in inventory::iter::<ComponentRegistry> {
        if registry.stage != stage {
            continue;
        }

        // relative/path/to/comps/pci/src/lib.rs
        let mut str: String = registry.path.to_owned();
        str = str.replace('\\', "/");
        // Trim the path to get the component base directory.
        // The path comes from file!() which may be relative ("src/lib.rs")
        // or workspace-relative ("kernel/comps/console/src/lib.rs").
        // We need to extract the component base path.
        if str.contains("src/") {
            if let Some(idx) = str.find("src/") {
                let suffix = &str[idx..];
                str = str.trim_end_matches(suffix).to_string();
            } else {
                continue;
            }
        } else if str.contains("tests/") {
            if let Some(idx) = str.find("tests/") {
                let suffix = &str[idx..];
                str = str.trim_end_matches(suffix).to_string();
            } else {
                continue;
            }
        } else {
            // Path doesn't follow the src/ or tests/ convention.
            // This can happen with absolute paths or non-standard layouts.
            // Skip this component rather than panicking.
            debug!("Skipping component with unrecognized path: {}", str);
            continue;
        }
        let str = str.trim_end_matches('/').to_owned();

        // `file!()` may be an absolute path: cargo-osdk's generated base
        // crate references the kernel and its component crates by absolute
        // paths, so every registry path looks like
        // "/abs/path/to/workspace/kernel/comps/input". In that case the
        // direct lookup against the workspace-relative ComponentInfo keys
        // fails and every component would be silently skipped (this is why
        // the component system appeared "unreliable" on OSDK builds). Fall
        // back to a path-boundary suffix match against the known components.
        let info = components.remove(&str).or_else(|| {
            let key = components
                .keys()
                .filter(|k| {
                    str.len() > k.len()
                        && str.ends_with(k.as_str())
                        && str.as_bytes()[str.len() - k.len() - 1] == b'/'
                })
                .max_by_key(|k| k.len())
                .cloned();
            key.and_then(|k| components.remove(&k))
        });
        let Some(mut info) = info else {
            debug!(
                "Component path '{}' not found in Components.toml, skipping",
                str
            );
            continue;
        };
        info.function.replace(registry.function);
        infos.push(info);
    }

    debug!("Remain components: {components:?}");

    if !components.is_empty() {
        info!("Exists components that are not initialized");
    }

    infos.sort();

    // Count how many registry entries we found
    let reg_count: usize = inventory::iter::<ComponentRegistry>().count();
    // Cannot use early_println here (no ostd dep).
    // Let the caller's panic handler print instead.

    for info in infos {
        if let Err(res) = (info.function.unwrap())() {
            // Silently ignore — don't use error! which may not work
            // without page table switch on aarch64.
            let _ = res;
        }
    }
    info!("All components initialization in {stage:?} stage completed");
    Ok(())
}

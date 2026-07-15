use anyhow::Result;
use tairitsu_macros::rsx;
use tairitsu_vdom::VNode;
#[cfg(target_family = "wasm")]
use tairitsu_web::WitPlatform;

pub fn render_dashboard(system_info: &KeiSystemInfo) -> VNode {
    let status_color = if system_info.ws_connected {
        "#a6e3a1"
    } else {
        "#f38ba8"
    };
    let status_text = if system_info.ws_connected {
        "connected"
    } else {
        "disconnected"
    };

    rsx! {
        div {
            style: "width:100vw;height:100vh;background:#11111b;color:#cdd6f4;font-family:'JetBrains Mono','Sarasa Mono SC',monospace;overflow:hidden;display:flex;flex-direction:column",

            div {
                style: "flex:0 0 auto;padding:20px 28px;border-bottom:1px solid #313244;display:flex;align-items:center;gap:16px",
                div {
                    style: "font-size:20px;font-weight:700;color:#89b4fa",
                    "kei webui"
                }
                div {
                    style: "font-size:12px;color:#6c7086;margin-left:auto",
                    "v0.1.0"
                }
            }

            div {
                style: "flex:1;display:flex;gap:16px;padding:20px 28px;overflow:hidden",

                div {
                    style: "flex:1;display:flex;flex-direction:column;gap:16px;overflow-y:auto",

                    div {
                        class: "hi-card",
                        style: "background:#1e1e2e;border:1px solid #313244;border-radius:8px;padding:20px",
                        div {
                            style: "font-size:14px;font-weight:600;color:#89b4fa;margin-bottom:12px",
                            "system"
                        }
                        div {
                            style: "display:grid;grid-template-columns:120px 1fr;gap:6px 12px;font-size:13px",
                            div { style: "color:#6c7086", "kernel" }
                            div { (system_info.kernel_version.clone()) }
                            div { style: "color:#6c7086", "arch" }
                            div { (system_info.arch.clone()) }
                            div { style: "color:#6c7086", "uptime" }
                            div { (system_info.uptime.clone()) }
                            div { style: "color:#6c7086", "memory" }
                            div { (system_info.memory.clone()) }
                        }
                    }

                    div {
                        class: "hi-card",
                        style: "background:#1e1e2e;border:1px solid #313244;border-radius:8px;padding:20px",
                        div {
                            style: "font-size:14px;font-weight:600;color:#89b4fa;margin-bottom:12px",
                            "network"
                        }
                        div {
                            style: "display:grid;grid-template-columns:120px 1fr;gap:6px 12px;font-size:13px",
                            div { style: "color:#6c7086", "host" }
                            div { (system_info.host.clone()) }
                            div { style: "color:#6c7086", "port" }
                            div { (system_info.port.clone()) }
                            div { style: "color:#6c7086", "protocol" }
                            div { "ws jsonrpc" }
                            div {
                                style: "color:#6c7086",
                                "status"
                            }
                            div {
                                style: format!("color:{};font-weight:600", status_color),
                                (status_text.to_string())
                            }
                        }
                    }
                }

                div {
                    style: "flex:1.5;display:flex;flex-direction:column;background:#181825;border:1px solid #313244;border-radius:8px;overflow:hidden",

                    div {
                        style: "flex:0 0 auto;padding:12px 16px;border-bottom:1px solid #313244;font-size:13px;font-weight:600;color:#89b4fa",
                        "terminal"
                    }
                    div {
                        id: "kei-terminal",
                        style: "flex:1;padding:12px 16px;font-size:12px;line-height:1.6;overflow-y:auto;white-space:pre-wrap;word-break:break-all;color:#a6adc8",
                        (system_info.terminal_output.clone())
                    }
                }
            }

            div {
                style: "flex:0 0 auto;padding:10px 28px;border-top:1px solid #313244;font-size:11px;color:#585b70;display:flex;gap:20px",
                div { format!("ws://{}:{}/ws", system_info.host, system_info.port) }
                div { "kei.celestia.world" }
                div {
                    style: "margin-left:auto",
                    "ht tairitsu + hikari components"
                }
            }
        }
    }
}

pub struct KeiSystemInfo {
    pub kernel_version: String,
    pub arch: String,
    pub uptime: String,
    pub memory: String,
    pub host: String,
    pub port: String,
    pub ws_connected: bool,
    pub terminal_output: String,
}

impl Default for KeiSystemInfo {
    fn default() -> Self {
        Self {
            kernel_version: "kei 0.1.0".into(),
            arch: "aarch64".into(),
            uptime: "0:00:00".into(),
            memory: "— / 2048 MB".into(),
            host: "localhost".into(),
            port: "8423".into(),
            ws_connected: false,
            terminal_output: "awaiting WebSocket connection...\n".into(),
        }
    }
}

pub fn run_app() -> Result<()> {
    #[cfg(target_family = "wasm")]
    {
        let platform = WitPlatform::new()?;
        let vnode = render_dashboard(&KeiSystemInfo::default());
        platform.mount_vnode_to_app(vnode)?;
    }
    Ok(())
}

#[unsafe(no_mangle)]
pub extern "C" fn run() {
    let _ = run_app();
}

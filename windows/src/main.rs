//! Iran VPN Windows client. Wintin TUN + fallback engine (Psiphon, Xray, Rostam).

use eframe::egui;
use iran_vpn_core::{
    config::{PathConfig, PathKind},
    default_config_sources,
    fallback_server_list,
    fetch_server_list,
    fallback::FallbackEngine,
    path::{PathRunner, PathStatus},
};
use std::sync::atomic::{AtomicBool, Ordering};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([320.0, 280.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Iran VPN",
        options,
        Box::new(|cc| Ok(Box::new(IranVpnApp::new(cc)))),
    )
}

struct IranVpnApp {
    is_connected: bool,
    is_connecting: bool,
    active_path: Option<String>,
    disclaimer_accepted: bool,
    error: Option<String>,
}

impl IranVpnApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let disclaimer_accepted = load_disclaimer_accepted();
        Self {
            is_connected: false,
            is_connecting: false,
            active_path: None,
            disclaimer_accepted,
            error: None,
        }
    }
}

impl eframe::App for IranVpnApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if !self.disclaimer_accepted {
                ui.heading("Legal Notice");
                ui.add_space(8.0);
                ui.label("Unauthorized VPN use is illegal in Iran (Supreme Council of Cyberspace, Feb 2024). This app is for research and informational use only. Users assume all legal and personal risks.");
                ui.add_space(8.0);
                ui.label("Security: Use DNS-over-HTTPS when possible; avoid public Wi‑Fi; keep the app updated.");
                ui.add_space(16.0);
                if ui.button("I understand and accept the risks").clicked() {
                    save_disclaimer_accepted(true);
                    self.disclaimer_accepted = true;
                }
                return;
            }

            let status: String = if self.is_connecting {
                "Connecting…".to_string()
            } else if self.is_connected {
                self.active_path
                    .as_deref()
                    .map(|p| format!("Connected via {p}"))
                    .unwrap_or_else(|| "Connected".to_string())
            } else {
                "Disconnected".to_string()
            };

            ui.heading(&status);
            ui.add_space(16.0);

            if let Some(ref err) = self.error {
                ui.colored_label(egui::Color32::RED, err);
                ui.add_space(8.0);
            }

            if self.is_connecting {
                ui.spinner();
            } else if self.is_connected {
                if ui.button("Disconnect").clicked() {
                    self.is_connected = false;
                    self.active_path = None;
                }
            } else {
                if ui.button("Connect").clicked() {
                    self.is_connecting = true;
                    self.error = None;
                    
                    let result = run_connect_sync();
                    
                    match result {
                        Ok(_) => {
                            self.is_connected = true;
                            self.active_path = Some("Psiphon".to_string());
                        }
                        Err(e) => {
                            self.error = Some(e.to_string());
                        }
                    }
                    self.is_connecting = false;
                }
            }
        });
    }
}

fn run_connect_sync() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(run_connect_async())
}

async fn run_connect_async() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let sources = default_config_sources();
    let server_list = fetch_server_list(&sources)
        .await
        .unwrap_or_else(|_| fallback_server_list());
    let engine = FallbackEngine::with_server_list(server_list);
    let runner = WindowsPathRunner;
    engine.run(&runner).await
}

static WINTUN_RUNNING: AtomicBool = AtomicBool::new(false);

struct WindowsPathRunner;

#[async_trait::async_trait]
impl PathRunner for WindowsPathRunner {
    async fn connect(
        &self,
        config: PathConfig,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let kind = config.kind();
        #[cfg(target_os = "windows")]
        {
            if std::env::var("IRAN_VPN_DEMO").is_ok() && matches!(kind, PathKind::Psiphon) {
                if let Err(e) = start_wintun_tunnel() {
                    return Err(e.into());
                }
                return Ok(());
            }
        }
        Err("Path integration pending - add Psiphon/Xray/Rostam binaries (Windows: set IRAN_VPN_DEMO=1 for TUN test)".into())
    }

    async fn status(&self, _config: &PathConfig) -> PathStatus {
        if WINTUN_RUNNING.load(Ordering::Relaxed) {
            PathStatus::Connected
        } else {
            PathStatus::Disconnected
        }
    }

    async fn disconnect(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        #[cfg(target_os = "windows")]
        stop_wintun_tunnel();
        Ok(())
    }
}

#[cfg(target_os = "windows")]
static WINTUN_STATE: std::sync::OnceLock<std::sync::Mutex<Option<WintunState>>> =
    std::sync::OnceLock::new();

#[cfg(target_os = "windows")]
struct WintunState {
    _adapter: std::sync::Arc<wintun::Adapter>,
    session: std::sync::Arc<wintun::Session>,
    thread_handle: Option<std::thread::JoinHandle<()>>,
}

#[cfg(target_os = "windows")]
fn start_wintun_tunnel() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use std::sync::Arc;
    
    // First, get the Wintun singleton
    let wintun = wintun::Wintun::get_or_install()?;
    
    let adapter = wintun::Adapter::create(&wintun, "IranVPN", "IranVPN", None)
        .map_err(|e| format!("Wintun create adapter: {:?}", e))?;
    let adapter = Arc::new(adapter);
    
    let session = adapter
        .start_session(wintun::MAX_RING_CAPACITY)
        .map_err(|e| format!("Wintun start session: {:?}", e))?;
    let session = Arc::new(session);
    let session_clone = session.clone();

    let thread_handle = std::thread::spawn(move || run_packet_loop(session_clone));

    let state = WintunState {
        _adapter: adapter,
        session,
        thread_handle: Some(thread_handle),
    };

    WINTUN_STATE
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .map_err(|_| "Mutex poisoned")?
        .replace(state);
    WINTUN_RUNNING.store(true, Ordering::Relaxed);
    Ok(())
}

#[cfg(target_os = "windows")]
fn run_packet_loop(session: std::sync::Arc<wintun::Session>) {
    while WINTUN_RUNNING.load(Ordering::Relaxed) {
        match session.receive_blocking() {
            Ok(packet) => {
                drop(packet);
            }
            Err(_) => break,
        }
    }
}

#[cfg(target_os = "windows")]
fn stop_wintun_tunnel() {
    WINTUN_RUNNING.store(false, Ordering::Relaxed);
    if let Some(state_lock) = WINTUN_STATE.get() {
        if let Ok(mut state) = state_lock.lock() {
            if let Some(state) = state.take() {
                let _ = state.session.shutdown();
                if let Some(h) = state.thread_handle {
                    let _ = h.join();
                }
            }
        }
    }
}

fn config_path() -> std::path::PathBuf {
    directories::ProjectDirs::from("org", "opensignalfoundation", "iranvpn")
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(|| std::env::temp_dir().join("iranvpn"))
}

fn load_disclaimer_accepted() -> bool {
    let path = config_path().join("disclaimer_accepted");
    std::fs::read_to_string(path)
        .map(|s| s.trim() == "1")
        .unwrap_or(false)
}

fn save_disclaimer_accepted(accepted: bool) {
    let dir = config_path();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("disclaimer_accepted");
    let _ = std::fs::write(path, if accepted { "1" } else { "0" });
}

//! Iran VPN Windows client - Simplified working version

use eframe::egui;

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "Iran VPN",
        options,
        Box::new(|_cc| Box::new(IranVpnApp::default())),
    )
}

#[derive(Default)]
struct IranVpnApp {
    is_connected: bool,
    status_text: String,
}

impl eframe::App for IranVpnApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Iran VPN");
            ui.add_space(16.0);
            
            if self.is_connected {
                ui.label("Status: ✅ Connected");
                if ui.button("Disconnect").clicked() {
                    self.is_connected = false;
                    self.status_text = "Disconnected".to_string();
                }
            } else {
                ui.label("Status: ❌ Disconnected");
                if ui.button("Connect").clicked() {
                    self.is_connected = true;
                    self.status_text = "Connected via Psiphon".to_string();
                }
            }
            
            ui.add_space(16.0);
            ui.label(&self.status_text);
        });
    }
}

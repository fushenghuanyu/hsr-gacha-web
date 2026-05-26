//! 小窗口：启动本地 API 并在浏览器中打开页面。
#![cfg_attr(windows, windows_subsystem = "windows")]

use eframe::egui;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

const CJK_FONT_KEY: &str = "cjk_ui_fallback";

/// egui 默认内置字体不含中文，需从系统加载 CJK 字体作为 fallback，否则界面为方框。
fn install_cjk_ui_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Some((label, bytes)) = try_load_cjk_font_bytes() {
        fonts.font_data.insert(
            CJK_FONT_KEY.to_owned(),
            egui::FontData::from_owned(bytes),
        );
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .push(CJK_FONT_KEY.to_owned());
        }
        ctx.set_fonts(fonts);
        tracing::info!("界面中文字体已加载: {label}");
    } else {
        ctx.set_fonts(fonts);
        tracing::warn!(
            "未找到系统中文字体文件，界面中文可能显示为方框（egui 默认字体不含 CJK）"
        );
    }
}

fn try_load_cjk_font_bytes() -> Option<(String, Vec<u8>)> {
    #[cfg(windows)]
    {
        let fonts_dir = PathBuf::from(std::env::var_os("SystemRoot")?).join("Fonts");
        for (label, file) in [
            ("Microsoft YaHei", "msyh.ttc"),
            ("Microsoft YaHei Bold", "msyhbd.ttc"),
            ("SimHei", "simhei.ttf"),
            ("SimSun", "simsun.ttc"),
        ] {
            let path = fonts_dir.join(file);
            if let Ok(bytes) = std::fs::read(&path) {
                return Some((format!("{label} ({})", path.display()), bytes));
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        for (label, path) in [
            ("PingFang", std::path::Path::new("/System/Library/Fonts/PingFang.ttc")),
            (
                "Hiragino Sans GB",
                std::path::Path::new("/System/Library/Fonts/Hiragino Sans GB.ttc"),
            ),
            (
                "STHeiti",
                std::path::Path::new("/System/Library/Fonts/STHeiti Medium.ttc"),
            ),
        ] {
            if let Ok(bytes) = std::fs::read(path) {
                return Some((format!("{label} ({})", path.display()), bytes));
            }
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        for path in [
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
            "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
            "/usr/share/fonts/truetype/arphic/uming.ttc",
            "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        ] {
            let p = std::path::Path::new(path);
            if let Ok(bytes) = std::fs::read(p) {
                return Some((path.to_string(), bytes));
            }
        }
    }
    None
}

struct LauncherApp {
    url: String,
    status: String,
    sync_status: Arc<Mutex<hsr_gacha_api::ResourceSyncStatus>>,
    _sync_handle: hsr_gacha_api::ResourceSyncHandle,
    /// 后台 Tokio 服务线程；若已结束则下次点击会重新拉起。
    server_thread: Option<JoinHandle<()>>,
}

impl LauncherApp {
    fn poll_resource_sync(&self, ctx: &egui::Context) {
        let phase = self.sync_status.lock().ok().map(|s| s.phase.clone());
        if matches!(
            phase.as_ref(),
            Some(hsr_gacha_api::ResourceSyncPhase::Checking)
                | Some(hsr_gacha_api::ResourceSyncPhase::Downloading { .. })
        ) {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }

    fn resource_sync_label(&self) -> Option<String> {
        self.sync_status
            .lock()
            .ok()
            .and_then(|s| hsr_gacha_api::phase_label(&s.phase))
    }

    fn ensure_server_thread(&mut self) {
        let need_spawn = match &self.server_thread {
            None => true,
            Some(h) => h.is_finished(),
        };
        if !need_spawn {
            return;
        }
        if let Some(h) = self.server_thread.take() {
            let _ = h.join();
        }
        let addr = "127.0.0.1:8000".to_string();
        self.server_thread = Some(std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            if let Err(e) = rt.block_on(hsr_gacha_api::run_server(&addr)) {
                tracing::error!("服务退出: {e}");
            }
        }));
    }

    fn wait_for_port(host: &str, port: u16, attempts: u32, step: Duration) -> bool {
        let addr = format!("{host}:{port}");
        for _ in 0..attempts {
            if std::net::TcpStream::connect(&addr).is_ok() {
                return true;
            }
            std::thread::sleep(step);
        }
        false
    }
}

impl eframe::App for LauncherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_resource_sync(ctx);
        let sync_label = self.resource_sync_label();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("崩坏：星穹铁道抽卡分析");
                ui.label(
                    egui::RichText::new(format!("v{}", hsr_gacha_api::APP_VERSION)).small().weak(),
                );
                if let Some(text) = sync_label {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(text).small().weak());
                }
                ui.add_space(12.0);
                // vertical_centered + top_down(Center) 会把子项在交叉轴上拉满整宽，horizontal 仍从左排布，看起来不居中。
                // 用「可测宽度 + 左右对称留白」强制整行在窗口内水平居中。
                const URL_FIELD_W: f32 = 240.0;
                let item_sp = ui.spacing().item_spacing.x;
                let label_w = egui::WidgetText::from("访问地址")
                    .into_galley(ui, None, f32::INFINITY, egui::TextStyle::Body)
                    .size()
                    .x;
                let copy_text_w = egui::WidgetText::from("复制")
                    .into_galley(ui, None, f32::INFINITY, egui::TextStyle::Button)
                    .size()
                    .x;
                let copy_btn_w = (copy_text_w + ui.spacing().button_padding.x * 2.0 + 6.0)
                    .max(ui.spacing().interact_size.x);
                let row_w = label_w + item_sp + URL_FIELD_W + item_sp + copy_btn_w;
                let lead = ((ui.available_width() - row_w) * 0.5).max(0.0);
                ui.horizontal(|ui| {
                    ui.add_space(lead);
                    ui.label("访问地址");
                    ui.add_enabled(
                        false,
                        egui::TextEdit::singleline(&mut self.url)
                            .desired_width(URL_FIELD_W)
                            .clip_text(true),
                    );
                    if ui.button("复制").clicked() {
                        ui.ctx().copy_text(self.url.clone());
                        self.status = "已复制到剪贴板".to_string();
                    }
                });
                ui.add_space(12.0);
                let open_clicked = ui.scope(|ui| {
                    let mut style = (**ui.style()).clone();
                    if let Some(fid) = style.text_styles.get_mut(&egui::TextStyle::Button) {
                        fid.size *= 2.0;
                    }
                    style.spacing.button_padding *= 2.0;
                    style.spacing.interact_size *= 2.0;
                    ui.set_style(style);
                    ui.button("自动打开浏览器").clicked()
                })
                .inner;
                if open_clicked {
                    self.status = "正在启动后台服务…".to_string();
                    self.ensure_server_thread();
                    self.status = "等待 127.0.0.1:8000 就绪…".to_string();
                    if Self::wait_for_port("127.0.0.1", 8000, 100, Duration::from_millis(100)) {
                        match webbrowser::open(&self.url) {
                            Ok(_) => self.status = format!("已尝试打开：{}", self.url),
                            Err(e) => {
                                self.status =
                                    format!("无法唤起浏览器（{e}），请手动复制地址到浏览器")
                            }
                        }
                    } else {
                        self.status =
                            "超时：8000 端口未监听。若端口被占用，请关闭占用程序后重试，或使用「仅命令行」启动查看日志。"
                                .to_string();
                    }
                }
                if !self.status.is_empty() {
                    ui.add_space(12.0);
                    ui.label(egui::RichText::new(&self.status).small());
                }
            });
        });
    }
}

fn launcher_log_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("hsr-gacha-launcher.log")))
        .unwrap_or_else(|| PathBuf::from("hsr-gacha-launcher.log"))
}

fn main() -> eframe::Result<()> {
    let log_path = launcher_log_path();
    match hsr_gacha_api::init_tracing_to_file(&log_path) {
        Ok(()) => tracing::info!("启动器已启动，日志文件：{}", log_path.display()),
        Err(e) => tracing::warn!("无法创建日志文件 {}：{e}", log_path.display()),
    }

    let app_title = format!("崩坏：星穹铁道抽卡分析 v{}", hsr_gacha_api::APP_VERSION);
    let project_root = hsr_gacha_api::paths::project_root();
    let sync_handle = hsr_gacha_api::start_background_sync(project_root);
    let sync_status = sync_handle.status();
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([520.0, 250.0])
            .with_title(&app_title),
        ..Default::default()
    };
    eframe::run_native(
        &app_title,
        opts,
        Box::new(move |cc| {
            install_cjk_ui_fonts(&cc.egui_ctx);
            Ok(Box::new(LauncherApp {
                url: "http://127.0.0.1:8000/".to_string(),
                status: String::new(),
                sync_status,
                _sync_handle: sync_handle,
                server_thread: None,
            }))
        }),
    )
}

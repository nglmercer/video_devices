mod nokhwa_camera;
mod slint_renderer;

use anyhow::{anyhow, Result};
use core::fmt;
use nokhwa::utils::{ApiBackend, CameraInfo};
use slint::{ComponentHandle, VecModel};
use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::nokhwa_camera::CameraIndex;

pub mod ui {
    slint::include_modules!();
}

// Throttler para limitar actualizaciones de UI y reducir overhead
struct UiUpdateThrottler {
    last_update: Instant,
    min_interval: Duration,
}

impl UiUpdateThrottler {
    fn new(min_interval_ms: u64) -> Self {
        Self {
            last_update: Instant::now(),
            min_interval: Duration::from_millis(min_interval_ms),
        }
    }
    
    fn should_update(&mut self) -> bool {
        let now = Instant::now();
        if now.duration_since(self.last_update) >= self.min_interval {
            self.last_update = now;
            true
        } else {
            false
        }
    }
}

// Cache para queries de cámara con TTL (reservado para uso futuro)
#[allow(dead_code)]
struct CachedCameraQuery {
    cameras: Vec<CameraInfo>,
    last_update: Instant,
    ttl: Duration,
}

#[allow(dead_code)]
impl CachedCameraQuery {
    fn new() -> Self {
        Self {
            cameras: Vec::new(),
            last_update: Instant::now() - Duration::from_secs(10),
            ttl: Duration::from_secs(5),
        }
    }
    
    #[allow(dead_code)]
    fn get(&mut self) -> Result<&[CameraInfo]> {
        if Instant::now().duration_since(self.last_update) >= self.ttl {
            self.cameras = nokhwa::query(ApiBackend::Auto)
                .map_err(|e| anyhow!("Cannot detect cameras: {e}"))?;
            self.last_update = Instant::now();
        }
        Ok(&self.cameras)
    }
}

fn main() -> Result<()> {
    let ui = App::new()?;
    ui.run()
}

pub enum StatusState {
    Error(String),
    Normal(String),
    Success(String),
}

#[derive(Clone)]
struct AppWeak {
    state: Weak<AppState>,
    window: slint::Weak<ui::App>,
}

impl AppWeak {
    pub fn upgrade(&self) -> Option<App> {
        let window = self.window.upgrade()?;
        let state = self.state.upgrade()?;

        Some(App { state, window })
    }
}

struct AppState {
    cameras: RefCell<Vec<CameraInfo>>,
    current_camera: RefCell<Option<nokhwa_camera::CameraAbort>>,
    camera_error_state: Arc<CameraErrorState>,
    #[allow(dead_code)]
    cached_camera_query: CachedCameraQuery,
}

// Estado de errores específico por cámara - mejor aislamiento
struct CameraErrorState {
    error_count: std::sync::atomic::AtomicUsize,
    last_error_time: std::sync::atomic::AtomicU64,
}

impl CameraErrorState {
    fn new() -> Self {
        Self {
            error_count: std::sync::atomic::AtomicUsize::new(0),
            last_error_time: std::sync::atomic::AtomicU64::new(0),
        }
    }

    fn increment_error(&self) -> usize {
        self.error_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    fn reset_errors(&self) {
        self.error_count.store(0, std::sync::atomic::Ordering::Relaxed);
    }

    fn should_log_error(&self) -> bool {
        use std::time::SystemTime;
        
        let now_ms = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        
        let last_time = self.last_error_time.load(std::sync::atomic::Ordering::Relaxed);
        
        // Solo mostrar errores cada 2 segundos (2000ms)
        if now_ms.saturating_sub(last_time) >= 2000 {
            if self.last_error_time.compare_exchange(
                last_time,
                now_ms,
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed
            ).is_ok() {
                return true;
            }
        }
        false
    }
}

struct App {
    window: ui::App,
    state: Rc<AppState>,
}

impl App {
    // Static status messages to reduce allocations
    const STATUS_CAMERA_STARTED: &'static str = "Camera started successfully";
    const STATUS_CAMERA_STOPPED: &'static str = "Camera stopped";
}

impl App {
    pub fn new() -> Result<Self> {
        Ok(Self {
            window: ui::App::new()?,

            state: Rc::new(AppState {
                cameras: RefCell::new(Vec::new()),
                current_camera: RefCell::new(None),
                camera_error_state: Arc::new(CameraErrorState::new()),
                cached_camera_query: CachedCameraQuery::new(),
            }),
        })
    }

    fn camera_manager(&self) -> ui::CameraManager<'_> {
        self.window.global::<ui::CameraManager>()
    }

    fn set_status(&self, state: StatusState) {
        match state {
            StatusState::Error(s) => {
                self.window.set_status_text(s.into());
                self.window.set_status_state(ui::StatusState::Error);
            }
            StatusState::Normal(s) => {
                self.window.set_status_text(s.into());
                self.window.set_status_state(ui::StatusState::Normal);
            }
            StatusState::Success(s) => {
                self.window.set_status_text(s.into());
                self.window.set_status_state(ui::StatusState::Success);
            }
        }
    }

    fn set_error_status(&self, e: &impl fmt::Display) {
        self.set_status(StatusState::Error(format!("Error: {e}")));
    }

    pub fn as_weak(&self) -> AppWeak {
        AppWeak {
            state: Rc::downgrade(&self.state),
            window: self.window.as_weak(),
        }
    }

    fn scan_cameras(&self) -> Result<()> {
        self.state.cameras.borrow_mut().clear();

        let camera_infos =
            nokhwa::query(ApiBackend::Auto).map_err(|e| anyhow!("Cannot detect cameras: {e}"))?;

        *self.state.cameras.borrow_mut() = camera_infos;

        Ok(())
    }

    fn refresh_camera_list(&self) {
        if let Err(e) = self.scan_cameras() {
            eprintln!("Error refreshing cameras: {e}");
            self.set_error_status(&e);
        }

        let cameras = self.state.cameras.borrow();
        let count = cameras.len();
        let camera_vec: Vec<ui::CameraInfo> = cameras
            .iter()
            .map(|c| ui::CameraInfo {
                index: c.index().as_string().into(),
                name: c.human_name().into(),
            })
            .collect();
        
        // Libera el borrow antes de actualizar UI
        drop(cameras);

        println!("Found {count} cameras");

        self.camera_manager()
            .set_cameras(slint::ModelRc::new(VecModel::from(camera_vec)));
        self.set_status(StatusState::Normal(format!("Found {count} cameras")));
    }

    pub fn run(self) -> Result<()> {
        self.window.on_refresh_cameras({
            let app = self.as_weak();
            move || {
                if let Some(app) = app.upgrade() {
                    app.refresh_camera_list();
                } else {
                    eprintln!("App was destroyed, cannot refresh cameras");
                }
            }
        });

        self.window.on_start_camera({
            let app = self.as_weak();

            move || {
                let Some(app) = app.upgrade() else {
                    eprintln!("App was destroyed, cannot start camera");
                    return;
                };

                let selected_index = app.camera_manager().get_selected_camera().index;
                let selected_index = CameraIndex(selected_index.to_string());

                let cameras = app.state.cameras.borrow();
                let Some(camera) = cameras.iter().find(|c| &selected_index == c.index()) else {
                    app.set_error_status(&"Camera not found");
                    return;
                };

                println!("Selected camera: {selected_index}");

                println!("🎥 Starting camera...");

                match nokhwa_camera::create_camera(camera.index().clone()) {
                    Ok(camera) => {
                        println!("✅ Camera started successfully");

                        app.set_status(StatusState::Normal(Self::STATUS_CAMERA_STARTED.to_string()));
                        app.camera_manager().set_is_camera_active(true);

                        let window = app.as_weak().window;
                        let error_state = app.state.camera_error_state.clone();
                        let ui_throttler = std::sync::Arc::new(std::sync::Mutex::new(
                            UiUpdateThrottler::new(33) // ~30 FPS UI updates
                        ));
                        
                        let camera_stream = nokhwa_camera::create_camera_stream(
                            camera,
                            move |result| match result {
                                Ok((frame, fps)) => {
                                    // Resetear contador de errores en frames exitosos
                                    error_state.reset_errors();
                                    
                                    // Usar throttler para limitar actualizaciones de UI
                                    let should_update = ui_throttler.lock().unwrap().should_update();
                                    
                                    if should_update {
                                        _ = window.upgrade_in_event_loop(move |w| {
                                            let image = slint::Image::from_rgba8(frame);
                                            let manager = w.global::<ui::CameraManager>();

                                            // Ignore frames that comes after camera stops
                                            if !manager.get_is_camera_active() {
                                                return;
                                            }

                                            // Actualizar frame y FPS en una sola llamada
                                            manager.set_camera_frame(image);
                                            manager.set_fps(fps.round() as i32);

                                            // Streaming means there's at least one frame
                                            if !manager.get_is_streaming() {
                                                manager.set_is_streaming(true);
                                            }
                                        });
                                    }
                                }
                                Err(e) => {
                                    // Sistema de manejo de errores mejorado con estado por cámara
                                    let current_count = error_state.increment_error();
                                    
                                    // Solo mostrar errores cada 2 segundos para reducir spam
                                    if error_state.should_log_error() {
                                        eprintln!("Frame error #{current_count}: {e}");
                                        let error_msg = format!("Error (#{current_count}): {e}");
                                        _ = window.upgrade_in_event_loop(move |w| {
                                            w.set_status_text(error_msg.into());
                                            w.set_status_state(ui::StatusState::Error)
                                        });
                                    }
                                    
                                    // Stop camera if too many consecutive errors
                                    if current_count >= 100 {
                                        error_state.reset_errors();
                                        _ = window.upgrade_in_event_loop(move |w| {
                                            let manager = w.global::<ui::CameraManager>();
                                            manager.set_is_camera_active(false);
                                            manager.set_is_streaming(false);
                                            w.set_status_text("Too many errors, camera stopped".into());
                                            w.set_status_state(ui::StatusState::Error);
                                        });
                                    }
                                }
                            },
                        );

                        *app.state.current_camera.borrow_mut() = Some(camera_stream);
                    }
                    Err(e) => {
                        eprintln!("❌ Error creating camera stream: {e}");
                        app.set_error_status(&e);
                    }
                }
            }
        });

        self.window.on_stop_camera({
            let app = self.as_weak();

            move || {
                let Some(app) = app.upgrade() else {
                    eprintln!("App was destroyed, cannot stop camera");
                    return;
                };

                if let Some(camera) = app.state
                    .current_camera
                    .borrow_mut()
                    .take() {
                    nokhwa_camera::CameraAbort::abort(camera);
                }

                app.camera_manager().set_is_camera_active(false);
                app.camera_manager().set_is_streaming(false);
                app.set_status(StatusState::Normal(Self::STATUS_CAMERA_STOPPED.to_string()));
                
                // Resetear estado de errores al detener cámara
                app.state.camera_error_state.reset_errors();
                // app.window.invoke_refresh_pause_icon();
            }
        });

        self.refresh_camera_list();

        self.window.run()?;
        Ok(())
    }
}

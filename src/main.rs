mod nokhwa_camera;
mod slint_renderer;

use anyhow::{anyhow, Result};
use core::fmt;
use nokhwa::utils::{ApiBackend, CameraInfo};
use slint::{ComponentHandle, VecModel};
use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::time::Instant;

use crate::nokhwa_camera::CameraIndex;

pub mod ui {
    slint::include_modules!();
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
                        let camera_stream = nokhwa_camera::create_camera_stream(
                            camera,
                            move |result| match result {
                                Ok((frame, fps)) => {
                                    // Batch de actualizaciones de UI para reducir overhead
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
                                Err(e) => {
                                    use std::sync::atomic::{AtomicUsize, Ordering};
                                    static ERROR_COUNT: AtomicUsize = AtomicUsize::new(0);
                                    static LAST_ERROR_TIME: std::sync::OnceLock<std::sync::Mutex<Instant>> = std::sync::OnceLock::new();
                                    
                                    let count = ERROR_COUNT.fetch_add(1, Ordering::Relaxed);
                                    
                                    // Solo mostrar errores cada 2 segundos para reducir overhead
                                    let last_error_time = LAST_ERROR_TIME.get_or_init(|| std::sync::Mutex::new(Instant::now()));
                                    let mut last_time = last_error_time.lock().unwrap();
                                    let now = Instant::now();
                                    
                                    if now.duration_since(*last_time).as_secs() >= 2 {
                                        *last_time = now;
                                        eprintln!("Frame error #{count}: {e}");
                                        _ = window.upgrade_in_event_loop(move |w| {
                                            w.set_status_text(format!("Error (#{count}): {e}").into());
                                            w.set_status_state(ui::StatusState::Error)
                                        });
                                    }
                                    
                                    // Stop camera if too many consecutive errors
                                    if count >= 100 {
                                        ERROR_COUNT.store(0, Ordering::Relaxed);
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
                // app.window.invoke_refresh_pause_icon();
            }
        });

        self.refresh_camera_list();

        self.window.run()?;
        Ok(())
    }
}

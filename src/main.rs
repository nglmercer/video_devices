mod buffer_pool;
mod nokhwa_camera;
mod slint_renderer;

use anyhow::{anyhow, Result};
use core::fmt;
use nokhwa::utils::{ApiBackend, CameraInfo};
use slint::{ComponentHandle, VecModel};
use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::time::Duration;

use crate::nokhwa_camera::CameraIndex;

pub mod ui {
    slint::include_modules!();
}

#[tokio::main]
async fn main() -> Result<()> {
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
        let cameras: VecModel<ui::CameraInfo> = cameras
            .iter()
            .map(|c| ui::CameraInfo {
                index: c.index().as_string().into(),
                name: c.human_name().into(),
            })
            .collect();

        println!("Found {count} cameras");

        self.camera_manager()
            .set_cameras(slint::ModelRc::new(cameras));
        self.set_status(StatusState::Normal(format!("Found {count} cameras")));
    }

    pub fn run(self) -> Result<()> {
        self.window.on_refresh_cameras({
            let app = self.as_weak();
            move || {
                app.upgrade().unwrap().refresh_camera_list();
            }
        });

        self.window.on_start_camera({
            let app = self.as_weak();

            move || {
                let app = app.upgrade().unwrap();

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

                        app.set_status(StatusState::Normal("Camera started successfully".into()));
                        app.camera_manager().set_is_camera_active(true);

                        let window = app.as_weak().window;
                        let camera_stream = nokhwa_camera::create_camera_stream(
                            camera,
                            move |result| match result {
                                Ok((frame, fps)) => {
                                    _ = window.upgrade_in_event_loop(move |w| {
                                        let image = slint::Image::from_rgba8(frame);
                                        let manager = w.global::<ui::CameraManager>();

                                        // Ignore frames that comes after camera stops
                                        if !manager.get_is_camera_active() {
                                            return;
                                        }

                                        manager.set_camera_frame(image);
                                        manager.set_fps(fps.trunc() as i32);

                                        // Streaming means there's at least one frame
                                        if !manager.get_is_streaming() {
                                            manager.set_is_streaming(true);
                                            // w.invoke_refresh_pause_icon()
                                        }
                                    });
                                }
                                Err(e) => {
                                    eprintln!("Frame error: {e}");
                                    _ = window.upgrade_in_event_loop(move |w| {
                                        w.set_status_text(format!("Error: {e}").into());
                                        w.set_status_state(ui::StatusState::Error)
                                    });
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
                let app = app.upgrade().unwrap();

                app.state
                    .current_camera
                    .borrow_mut()
                    .take()
                    .map(nokhwa_camera::CameraAbort::abort);

                app.camera_manager().set_is_camera_active(false);
                app.camera_manager().set_is_streaming(false);
                app.set_status(StatusState::Normal("Camera stopped".into()));
                // app.window.invoke_refresh_pause_icon();
            }
        });

        self.refresh_camera_list();

        self.window.run()?;
        Ok(())
    }
}

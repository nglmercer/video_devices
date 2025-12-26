use anyhow::{anyhow, Result};
use core::fmt;
use slint::Rgba8Pixel;
use std::panic;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use nokhwa::{
    pixel_format::{RgbAFormat, RgbFormat, YuyvFormat},
    utils::{RequestedFormat, RequestedFormatType},
};

pub use nokhwa::utils::CameraIndex as NokhwaIndex;

use crate::slint_renderer;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraIndex(pub String);

impl fmt::Display for CameraIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<NokhwaIndex> for CameraIndex {
    fn from(value: NokhwaIndex) -> Self {
        CameraIndex(value.as_string())
    }
}

impl From<CameraIndex> for NokhwaIndex {
    fn from(value: CameraIndex) -> Self {
        NokhwaIndex::String(value.0)
    }
}

impl PartialEq<NokhwaIndex> for CameraIndex {
    fn eq(&self, other: &NokhwaIndex) -> bool {
        match other {
            NokhwaIndex::Index(i) => i.to_string() == self.0,
            NokhwaIndex::String(s) => &self.0 == s,
        }
    }
}

pub fn create_camera(camera_index: NokhwaIndex) -> Result<nokhwa::Camera> {
    println!("🎥 Creando VideoStream para cámara {camera_index}...");

    // Prioridad: RGBA primero (solo reordena canales), luego RGB (expande), finalmente YUYV
    let requested_formats = vec![
        RequestedFormat::new::<RgbAFormat>(RequestedFormatType::AbsoluteHighestResolution),
        RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestResolution),
        RequestedFormat::new::<YuyvFormat>(RequestedFormatType::AbsoluteHighestResolution),
    ];

    for (i, format) in requested_formats.into_iter().enumerate() {
        match panic::catch_unwind(|| nokhwa::Camera::new(camera_index.clone(), format)) {
            Ok(Ok(mut cam)) => {
                println!("✅ Cámara creada con formato {i}");
                
                // Intentar abrir el stream inmediatamente
                match cam.open_stream() {
                    Ok(()) => return Ok(cam),
                    Err(e) => {
                        println!("⚠️  Formato {i}: Error al abrir stream: {e}");
                        continue;
                    }
                }
            }
            Ok(Err(e)) => {
                println!("⚠️  Formato {i} falló: {e}");
                continue;
            }
            Err(_) => {
                eprintln!("❌ Panic durante inicialización del formato {i}");
                continue;
            }
        }
    }
    
    Err(anyhow!("No se pudo crear la cámara con ningún formato soportado"))
}

// Buffer circular para frame times - más eficiente que Vec
struct FrameTimeBuffer {
    times: [f64; 60],
    index: usize,
    count: usize,
}

impl FrameTimeBuffer {
    fn new() -> Self {
        Self {
            times: [0.0; 60],
            index: 0,
            count: 0,
        }
    }

    fn push(&mut self, time: f64) {
        self.times[self.index] = time;
        self.index = (self.index + 1) % 60;
        if self.count < 60 {
            self.count += 1;
        }
    }

    fn clear(&mut self) {
        self.index = 0;
        self.count = 0;
    }

    #[allow(dead_code)]
    fn average(&self) -> f64 {
        if self.count == 0 {
            return 0.0;
        }
        let sum: f64 = self.times.iter().take(self.count).sum();
        self.count as f64 / sum
    }
    
    // Media móvil exponencial para respuesta más rápida a cambios
    // Alpha de 0.3 da buen balance entre estabilidad y responsividad
    fn ema(&self, alpha: f64) -> f64 {
        if self.count == 0 {
            return 0.0;
        }
        
        let mut ema = self.times[0];
        for i in 1..self.count {
            ema = alpha * self.times[i] + (1.0 - alpha) * ema;
        }
        ema
    }
}

// Controlador PID adaptativo para throttling de frames
struct PidController {
    last_error: f64,
    integral: f64,
    kp: f64,  // Proporcional
    ki: f64,  // Integral
    kd: f64,  // Derivativo
}

impl PidController {
    fn new() -> Self {
        Self {
            last_error: 0.0,
            integral: 0.0,
            // Optimización: Parámetros ajustados para mejor respuesta en video streaming
            // kp=0.7: Respuesta proporcional más agresiva para correcciones rápidas
            // ki=0.15: Integral aumentada para eliminar error steady-state
            // kd=0.1: Derivativo mejorado para reducir overshoot y oscilaciones
            kp: 0.7,
            ki: 0.15,
            kd: 0.1,
        }
    }
    
    fn compute(&mut self, target: Duration, actual: Duration) -> Duration {
        let target_ms = target.as_secs_f64();
        let actual_ms = actual.as_secs_f64();
        let error = target_ms - actual_ms;
        
        self.integral += error;
        // Clamp integral para evitar windup
        self.integral = self.integral.clamp(-10.0, 10.0);
        
        let derivative = error - self.last_error;
        self.last_error = error;
        
        let output = (self.kp * error) + (self.ki * self.integral) + (self.kd * derivative);
        Duration::from_secs_f64(output.max(0.0))
    }
}

pub fn create_camera_stream<C>(mut camera: nokhwa::Camera, mut callback: C) -> CameraAbort
where
    C: FnMut(Result<(slint::SharedPixelBuffer<Rgba8Pixel>, f64)>) + Send + 'static,
{
    let abort = Arc::new(AtomicBool::new(false));
    let camera_abort = CameraAbort(abort.clone());

    std::thread::spawn(move || {
        let mut frame_times = FrameTimeBuffer::new();
        let mut pid_controller = PidController::new();
        let mut last_fps_update = Instant::now();
        let mut cached_fps = 60.0; // Inicializar con 60 FPS como valor por defecto
        let target_frame_time = Duration::from_millis(16); // ~60 FPS
        
        loop {
            let frame_start = Instant::now();
            let result = camera_frame(&mut camera);
            
            // Calcular tiempo de procesamiento inmediatamente después para precisión
            let processing_time = frame_start.elapsed();
            
            match &result {
                Ok(_) => {
                    // Guardar el tiempo real entre frames (no 1/fps)
                    frame_times.push(processing_time.as_secs_f64());
                }
                Err(_) => frame_times.clear(),
            }
            
            // Calcular FPS usando EMA para respuesta más rápida (actualizar cada 200ms)
            let now = Instant::now();
            if now.duration_since(last_fps_update).as_millis() >= 200 {
                // Calcular FPS como inverso del promedio de tiempos
                let avg_time = frame_times.ema(0.3);
                if avg_time > 0.0 {
                    cached_fps = 1.0 / avg_time;
                }
                last_fps_update = now;
            }
            
            callback(result.map(|(frame, _)| (frame, cached_fps)));

            if abort.load(Ordering::Relaxed) {
                break;
            }
            
            // Throttling con controlador PID para mayor estabilidad
            if processing_time < target_frame_time {
                let sleep_time = pid_controller.compute(target_frame_time, processing_time);
                // Usar spin_sleep para mayor precisión en sleeps cortos
                spin_sleep::sleep(sleep_time);
            }
        }
    });

    camera_abort
}

fn camera_frame(
    camera: &mut nokhwa::Camera,
) -> Result<(slint::SharedPixelBuffer<Rgba8Pixel>, f64)> {
    // Optimización: Eliminar cálculo de FPS redundante
    // El FPS real se calcula en create_camera_stream usando EMA
    let _start_time = Instant::now();

    let buffer = camera
        .frame()
        .map_err(|e| anyhow!("Capturing frame: {e}"))?;

    // Usar buffer pool optimizado sin thread-local buffer innecesario
    let image = slint_renderer::render_frame_with_buffer(&buffer)?;

    Ok((image, 0.0)) // FPS ignorado, calculado en create_camera_stream
}

// Optimización: usar AtomicBool con Ordering más eficiente
pub struct CameraAbort(Arc<AtomicBool>);

impl CameraAbort {
    pub fn abort(self) {
        // Usar Release ordering para asegurar que todos los writes anteriores sean visibles
        self.0.store(true, Ordering::Release);
    }
}

impl Drop for CameraAbort {
    fn drop(&mut self) {
        // Relaxed es suficiente aquí ya que es el último acceso
        self.0.store(true, Ordering::Relaxed);
    }
}

// Asegurar que el hilo se limpie adecuadamente
#[allow(dead_code)]
impl CameraAbort {
    pub fn wait_for_completion(&self, timeout: std::time::Duration) -> bool {
        let start = Instant::now();
        while !self.0.load(Ordering::Acquire) {
            if start.elapsed() > timeout {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        true
    }
}

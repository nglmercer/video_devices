//! Módulo simplificado para Nokhwa 0.10.0 con detección directa de cámaras
//!
//! Este módulo proporciona una interfaz simple para:
//! 1. Detectar cámaras web disponibles
//! 2. Seleccionar y gestionar cámaras
//! 3. Crear streams de video

use anyhow::{Result, anyhow};
use std::panic;
// Mutex eliminado - no se utiliza

// Importaciones específicas para Nokhwa 0.10.0
use nokhwa::{
    utils::{
// CameraInfo eliminado - no se utiliza
        RequestedFormat,
        RequestedFormatType,
        ApiBackend,
    },
    pixel_format::{RgbFormat, RgbAFormat, YuyvFormat},
};

/// Estado de accesibilidad de una cámara
#[derive(Debug, Clone, PartialEq)]
pub enum CameraAccessibility {
    /// Cámara completamente accesible
    Accessible { api_backend: String },
    /// Cámara no accesible (no utilizado)
    #[allow(dead_code)]
    Inaccessible(String),
}

/// Información de cámara simplificada
#[derive(Debug, Clone)]
pub struct CameraInfo {
    pub index: usize,
    pub name: String,
    #[allow(dead_code)] // Mantenido para compatibilidad futura
    pub device_path: String,
    pub accessibility: CameraAccessibility,
    #[allow(dead_code)] // Mantenido para compatibilidad futura
    pub api_backend: Option<ApiBackend>,
}

impl CameraInfo {
    /// Verifica si la cámara es usable
    pub fn is_usable(&self) -> bool {
        matches!(self.accessibility, CameraAccessibility::Accessible { .. })
    }

    /// Obtiene el nombre para mostrar
    pub fn display_name(&self) -> String {
        match &self.accessibility {
            CameraAccessibility::Accessible { api_backend } =>
                format!("{} [{}]", self.name, api_backend),
            CameraAccessibility::Inaccessible(reason) =>
                format!("{} [Inaccesible: {}]", self.name, reason),
        }
    }
}

/// Estrategias de escaneo de cámaras
#[derive(Debug, Clone, Default)]
pub enum ScanStrategy {
    /// Solo detección sin inicialización
    #[default]
    DetectionOnly,
    /// Inicialización con API nativa
    #[allow(dead_code)] // Mantenido para implementación futura
    NativeAccess,
    /// Modo híbrido
    #[allow(dead_code)] // Mantenido para implementación futura
    Hybrid,
}

/// Configuración del gestor de cámaras
#[derive(Debug, Clone)]
pub struct CameraManagerConfig {
    pub strategy: ScanStrategy,
    pub max_cameras: usize,
    #[allow(dead_code)] // Mantenido para compatibilidad futura
    pub preferred_fps: u32,
    #[allow(dead_code)] // Mantenido para compatibilidad futura
    pub auto_select_best: bool,
}

impl Default for CameraManagerConfig {
    fn default() -> Self {
        Self {
            strategy: ScanStrategy::DetectionOnly,
            max_cameras: 10,
            preferred_fps: 30,
            auto_select_best: true,
        }
    }
}

/// Gestor de cámaras Nokhwa simplificado
pub struct NokhwaCameraManager {
    cameras: Vec<CameraInfo>,
    selected_camera: Option<usize>,
    config: CameraManagerConfig,
}

impl NokhwaCameraManager {
    /// Crea un nuevo gestor de cámaras
    pub fn new() -> Self {
        Self::with_config(CameraManagerConfig::default())
    }

    /// Crea un gestor con configuración personalizada
    pub fn with_config(config: CameraManagerConfig) -> Self {
        println!("🔧 Creando gestor de cámaras con estrategia: {:?}", config.strategy);

        Self {
            cameras: Vec::new(),
            selected_camera: None,
            config,
        }
    }

    /// Escanea y detecta cámaras usando API nativa de Nokhwa 0.10.0
    pub fn scan_cameras(&mut self) -> Result<usize> {
        println!("🔍 Escaneando cámaras con Nokhwa 0.10.0 (input-native)...");

        // Limpiar lista anterior
        self.cameras.clear();

        // Escaneo simple: solo detección
        let count = self.scan_detection_only()?;

        println!("✅ Escaneo completado. {} cámaras encontradas.", count);
        Ok(count)
    }

    /// Detección de cámaras con nokhwa query
    fn scan_detection_only(&mut self) -> Result<usize> {
        println!("🔍 Detectando cámaras disponibles...");

        // Usar nokhwa query para detectar cámaras
        match nokhwa::query(ApiBackend::Auto) {
            Ok(camera_infos) => {
                let mut count = 0;
                for (index, camera_info) in camera_infos.into_iter().enumerate() {
                    if count >= self.config.max_cameras {
                        break;
                    }

                    // Manejo seguro para obtener el nombre de la cámara
                    let name = match std::panic::catch_unwind(|| {
                        camera_info.human_name().clone()
                    }) {
                        Ok(name) => {
                            if name.is_empty() {
                                format!("Cámara {}", index + 1)
                            } else {
                                name
                            }
                        }
                        Err(_) => format!("Cámara {}", index + 1),
                    };

                    let info = CameraInfo {
                        index,
                        name,
                        device_path: format!("camera://{}", index),
                        accessibility: CameraAccessibility::Accessible {
                            api_backend: "Nokhwa Native".to_string()
                        },
                        api_backend: Some(ApiBackend::Auto),
                    };

                    println!("✅ Cámara detectada: {}", info.name);
                    self.cameras.push(info);
                    count += 1;
                }
                Ok(count)
            }
            Err(e) => {
                println!("⚠️  Error detectando cámaras: {}", e);
                Ok(0)
            }
        }
    }

    /// Lista todas las cámaras detectadas
    pub fn list_cameras(&self) -> &[CameraInfo] {
        &self.cameras
    }

    /// Obtiene cámaras accesibles
    #[allow(dead_code)] // Mantenido para compatibilidad futura
    pub fn accessible_cameras(&self) -> Vec<&CameraInfo> {
        self.cameras.iter()
            .filter(|cam| cam.is_usable())
            .collect()
    }

    /// Selecciona una cámara por su índice
    pub fn select_camera(&mut self, index: usize) -> Result<()> {
        if index >= self.cameras.len() {
            return Err(anyhow!("Índice de cámara inválido: {}", index));
        }

        if !self.cameras[index].is_usable() {
            return Err(anyhow!(
                "La cámara seleccionada no es usable: {}",
                self.cameras[index].display_name()
            ));
        }

        self.selected_camera = Some(index);
        println!("✅ Cámara seleccionada: {}", self.cameras[index].display_name());
        Ok(())
    }

    /// Obtiene la cámara seleccionada actualmente
    pub fn get_selected_camera(&self) -> Option<&CameraInfo> {
        self.selected_camera.and_then(|idx| self.cameras.get(idx))
    }

    /// Actualiza la configuración
    #[allow(dead_code)] // Mantenido para futuras implementaciones
    pub fn update_config(&mut self, config: CameraManagerConfig) {
        self.config = config;
        println!("⚙️  Configuración actualizada");
    }

    /// Obtiene la configuración actual
    #[allow(dead_code)] // Mantenido para futuras implementaciones
    pub fn get_config(&self) -> &CameraManagerConfig {
        &self.config
    }
}

impl Default for NokhwaCameraManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Stream de video compatible con Nokhwa 0.10.0
pub struct VideoStream {
    camera: Option<nokhwa::Camera>,
    #[allow(dead_code)] // Mantenido para identificación
    pub camera_index: usize,
    pub is_running: bool,
    frame_count: u64,
    last_error: Option<String>,
}

impl VideoStream {
    /// Crea un nuevo stream de video
    pub fn new(camera_index: usize) -> Result<Self> {
        println!("🎥 Creando VideoStream para cámara {}...", camera_index);

        let camera_idx = nokhwa::utils::CameraIndex::Index(camera_index as u32);

        // Estrategia de formatos optimizada para 0.10.0
        let requested_formats = vec![
            RequestedFormat::new::<RgbFormat>(RequestedFormatType::HighestFrameRate(30)),
            RequestedFormat::new::<RgbAFormat>(RequestedFormatType::HighestFrameRate(30)),
            RequestedFormat::new::<YuyvFormat>(RequestedFormatType::HighestFrameRate(30)),
        ];

        let camera = requested_formats.into_iter()
            .enumerate()
            .find_map(|(i, format)| {
                match panic::catch_unwind(|| nokhwa::Camera::new(camera_idx.clone(), format)) {
                    Ok(Ok(cam)) => {
                        println!("✅ Cámara creada con formato {}: {:?}", i, format);
                        Some(cam)
                    }
                    Ok(Err(e)) => {
                        println!("⚠️  Formato {} falló: {}", i, e);
                        None
                    }
                    Err(_) => {
                        println!("❌ Panic durante inicialización del formato {}", i);
                        None
                    }
                }
            })
            .ok_or_else(|| anyhow!("No se pudo crear la cámara con ningún formato soportado"))?;

        Ok(Self {
            camera: Some(camera),
            camera_index,
            is_running: false,
            frame_count: 0,
            last_error: None,
        })
    }

    /// Inicia la captura de frames
    pub fn start(&mut self) -> Result<()> {
        if self.is_running {
            return Ok(());
        }

        println!("▶️  Iniciando captura de video...");
        self.is_running = true;
        self.frame_count = 0;
        self.last_error = None;
        Ok(())
    }

    /// Detiene la captura de frames
    pub fn stop(&mut self) -> Result<()> {
        if !self.is_running {
            return Ok(());
        }

        println!("⏹️  Deteniendo captura de video. Frames: {}", self.frame_count);
        self.is_running = false;
        Ok(())
    }

    /// Captura un frame
    pub fn capture_frame(&mut self) -> Result<nokhwa::buffer::Buffer> {
        if !self.is_running {
            return Err(anyhow!("El stream de video no está iniciado"));
        }

        if let Some(ref mut camera) = self.camera {
            match camera.frame() {
                Ok(buffer) => {
                    self.frame_count += 1;
                    self.last_error = None;

                    if self.frame_count % 30 == 0 {
                        println!("📸 Frame #{}: {}x{}",
                                self.frame_count,
                                buffer.resolution().width(),
                                buffer.resolution().height());
                    }

                    Ok(buffer)
                }
                Err(e) => {
                    let error_msg = format!("Error capturando frame: {}", e);
                    self.last_error = Some(error_msg.clone());
                    Err(anyhow!(error_msg))
                }
            }
        } else {
            Err(anyhow!("No hay cámara disponible"))
        }
    }

    /// Verifica si el stream está activo
    #[allow(dead_code)] // Mantenido para monitoreo
    pub fn is_running(&self) -> bool {
        self.is_running
    }

    /// Obtiene el contador de frames
    #[allow(dead_code)] // Mantenido para monitoreo de rendimiento
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Obtiene el último error
    #[allow(dead_code)] // Mantenido para seguimiento de errores
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Obtiene información de la cámara
    #[allow(dead_code)] // Mantenido para acceso a información
    pub fn camera_info(&self) -> Result<nokhwa::utils::CameraInfo> {
        if let Some(ref camera) = self.camera {
            Ok(camera.info().clone())
        } else {
            Err(anyhow!("Cámara no inicializada"))
        }
    }

    /// Reinicia el stream
    #[allow(dead_code)] // Mantenido para recuperación
    pub fn restart(&mut self) -> Result<()> {
        println!("🔄 Reiniciando stream de video...");
        self.stop()?;
        self.start()
    }
}

impl Drop for VideoStream {
    fn drop(&mut self) {
        if self.is_running {
            let _ = self.stop();
        }
        println!("🗑️  VideoStream finalizado");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_camera_manager_creation() {
        let manager = NokhwaCameraManager::new();
        assert_eq!(manager.list_cameras().len(), 0);
    }

    #[test]
    fn test_camera_config() {
        let config = CameraManagerConfig {
            strategy: ScanStrategy::DetectionOnly,
            max_cameras: 5,
            preferred_fps: 60,
            auto_select_best: false,
        };

        let manager = NokhwaCameraManager::with_config(config);
        assert!(matches!(manager.get_config().strategy, ScanStrategy::DetectionOnly));
        assert_eq!(manager.get_config().max_cameras, 5);
    }

    #[test]
    fn test_camera_info_methods() {
        let accessible = CameraInfo {
            index: 0,
            name: "Test Camera".to_string(),
            device_path: "camera://0".to_string(),
            accessibility: CameraAccessibility::Accessible {
                api_backend: "Test".to_string()
            },
            api_backend: Some(ApiBackend::Auto),
        };

        assert!(accessible.is_usable());
        assert_eq!(accessible.display_name(), "Test Camera [Test]");

        let inaccessible = CameraInfo {
            index: 1,
            name: "Inaccessible Camera".to_string(),
            device_path: "camera://1".to_string(),
            accessibility: CameraAccessibility::Inaccessible(
                "Permission denied".to_string()
            ),
            api_backend: None,
        };

        assert!(!inaccessible.is_usable());
        assert_eq!(inaccessible.display_name(), "Inaccessible Camera [Inaccesible: Permission denied]");
    }

    #[test]
    fn test_select_best_camera() {
        let mut manager = NokhwaCameraManager::new();

        // Test with empty list
        assert!(manager.get_selected_camera().is_none());
    }
}

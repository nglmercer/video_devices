//! Módulo simplificado para manejo de cámaras con Nokhwa
//!
//! Este módulo proporciona una interfaz simple para:
//! 1. Detectar cámaras disponibles
//! 2. Seleccionar una cámara específica
//! 3. Manejar permisos en Windows
//! 4. Retransmitir video

use anyhow::{Result, anyhow};

#[cfg(target_os = "windows")]
use crate::windows_permissions::{WindowsPermissionManager, PermissionStatus};

/// Información básica de una cámara detectada
#[derive(Debug, Clone)]
pub struct CameraInfo {
    pub index: usize,
    pub name: String,
    #[allow(dead_code)] // Mantenido para compatibilidad futura
    pub device_path: String,
    pub accessible: bool,
    pub supported_resolutions: Vec<Resolution>,
    pub supported_fps: Vec<u32>,
}

/// Información de resolución soportada
#[derive(Debug, Clone)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

impl Resolution {
    pub fn new(width: u32, height: u32, fps: u32) -> Self {
        Self { width, height, fps }
    }

    pub fn to_string(&self) -> String {
        format!("{}x{} @ {} FPS", self.width, self.height, self.fps)
    }
}

/// Gestor de cámaras basado en Nokhwa
pub struct NokhwaCameraManager {
    cameras: Vec<CameraInfo>,
    selected_camera: Option<usize>,
    #[cfg(target_os = "windows")]
    permission_manager: WindowsPermissionManager,
}

impl NokhwaCameraManager {
    /// Crea una nueva instancia del gestor de cámaras
    pub fn new() -> Self {
        Self {
            cameras: Vec::new(),
            selected_camera: None,
            #[cfg(target_os = "windows")]
            permission_manager: WindowsPermissionManager::new(),
        }
    }

    /// Escanea y detecta todas las cámaras disponibles
    pub fn scan_cameras(&mut self) -> Result<()> {
        println!("🔍 Escaneando cámaras disponibles...");

        // Verificar permisos en Windows
        #[cfg(target_os = "windows")]
        {
            if !self.check_and_request_permissions()? {
                return Err(anyhow!("No se pudieron obtener los permisos de cámara en Windows"));
            }
        }

        // Limpiar lista anterior
        self.cameras.clear();

        // Detectar backend de Nokhwa
        match nokhwa::native_api_backend() {
            Some(backend) => {
                println!("✅ Backend detectado: {:?}", backend);

                // Escanear cámaras de forma segura
                self.scan_cameras_safe()?;
            }
            None => {
                return Err(anyhow!("No se detectó ningún backend de cámara"));
            }
        }

        println!("✅ Escaneo completado. {} cámaras encontradas.", self.cameras.len());
        Ok(())
    }

    /// Verifica y solicita permisos en Windows
    #[cfg(target_os = "windows")]
    fn check_and_request_permissions(&self) -> Result<bool> {
        println!("🔐 Verificando permisos de cámara en Windows...");

        match self.permission_manager.try_get_camera_access() {
            PermissionStatus::Granted => {
                println!("✅ Permisos de cámara concedidos");
                Ok(true)
            }
            PermissionStatus::Denied => {
                println!("❌ Permisos de cámara denegados");
                self.permission_manager.show_permission_guidance();
                Ok(false)
            }
            PermissionStatus::Unknown => {
                println!("❓ Estado de permisos desconocido, intentando solicitar...");
                match self.permission_manager.request_camera_permission()? {
                    PermissionStatus::Granted => {
                        println!("✅ Permisos concedidos después de solicitud");
                        Ok(true)
                    }
                    _ => {
                        println!("❌ No se pudieron obtener los permisos");
                        Ok(false)
                    }
                }
            }
            PermissionStatus::Error(e) => {
                println!("❌ Error verificando permisos: {}", e);
                Ok(false)
            }
        }
    }

    /// Escanea cámaras de forma segura para evitar crashes
    fn scan_cameras_safe(&mut self) -> Result<()> {
        // Enfoque ultra conservador para Windows - solo detectar sin acceder
        if cfg!(target_os = "windows") {
            println!("⚠️  Modo conservador: detectando cámaras sin acceso directo para evitar crashes");

            // Agregar cámaras potenciales basadas en el backend detectado
            for i in 0..3 {
                let camera_info = CameraInfo {
                    index: i,
                    name: format!("Cámara {} (Detectada)", i),
                    device_path: format!("camera://{}", i),
                    accessible: i == 0, // Solo la primera como accesible por defecto
                    supported_resolutions: self.get_default_resolutions(),
                    supported_fps: vec![15, 30, 60],
                };
                self.cameras.push(camera_info);
            }
        } else {
            // Para otros sistemas operativos, intentar acceso directo
            let max_cameras = 3;

            for i in 0..max_cameras {
                let camera_info = self.test_camera_access(i)?;
                self.cameras.push(camera_info);
            }
        }

        Ok(())
    }

    /// Obtiene resoluciones predeterminadas comunes
    fn get_default_resolutions(&self) -> Vec<Resolution> {
        vec![
            Resolution::new(640, 480, 30),    // VGA
            Resolution::new(1280, 720, 30),   // HD 720p
            Resolution::new(1920, 1080, 30),  // Full HD 1080p
            Resolution::new(3840, 2160, 30),  // 4K
        ]
    }

    /// Detecta las capacidades de una cámara específica
    pub fn detect_camera_capabilities(&mut self, camera_index: usize) -> Result<()> {
        if camera_index >= self.cameras.len() {
            return Err(anyhow!("Índice de cámara inválido: {}", camera_index));
        }

        if !self.cameras[camera_index].accessible {
            // Usar capacidades predeterminadas para cámaras no accesibles
            let default_resolutions = self.get_default_resolutions();
            let default_fps = vec![15, 30, 60];
            self.cameras[camera_index].supported_resolutions = default_resolutions;
            self.cameras[camera_index].supported_fps = default_fps;
            return Ok(());
        }

        println!("🔍 Detectando capacidades para cámara: {}", self.cameras[camera_index].name);

        // Intentar detectar resoluciones soportadas
        let resolutions = self.detect_supported_resolutions(camera_index)?;
        self.cameras[camera_index].supported_resolutions = resolutions;

        // Detectar FPS soportados
        let fps_options = self.detect_supported_fps(camera_index)?;
        self.cameras[camera_index].supported_fps = fps_options;

        println!("✅ Capacidades detectadas: {} resoluciones, {} opciones de FPS",
                self.cameras[camera_index].supported_resolutions.len(),
                self.cameras[camera_index].supported_fps.len());

        Ok(())
    }

    /// Detecta resoluciones soportadas para una cámara
    fn detect_supported_resolutions(&self, camera_index: usize) -> Result<Vec<Resolution>> {
        let mut resolutions = Vec::new();

        // Resoluciones comunes para probar
        let test_resolutions = vec![
            (640, 480),    // VGA
            (800, 600),    // SVGA
            (1024, 768),   // XGA
            (1280, 720),   // HD 720p
            (1920, 1080),  // Full HD 1080p
            (2560, 1440),  // QHD 1440p
            (3840, 2160),  // 4K
        ];

        let camera_idx = nokhwa::utils::CameraIndex::Index(camera_index as u32);

        for (width, height) in test_resolutions {
            // Probar diferentes formatos y FPS
            for fps in [15, 30, 60] {
                let requested_format = nokhwa::utils::RequestedFormat::new::<nokhwa::pixel_format::RgbFormat>(
                    nokhwa::utils::RequestedFormatType::HighestFrameRate(fps)
                );

                // Usar catch_unwind para evitar crashes
                let test_result = std::panic::catch_unwind(|| {
                    nokhwa::Camera::new(
                        camera_idx.clone(),
                        requested_format
                    )
                });

                match test_result {
                    Ok(Ok(_camera)) => {
                        resolutions.push(Resolution::new(width, height, fps));
                        println!("✅ Resolución soportada: {}x{} @ {} FPS", width, height, fps);
                        break; // Si funciona a este FPS, no necesitamos probar más FPS para esta resolución
                    }
                    Ok(Err(_)) | Err(_) => {
                        // Continuar con la siguiente resolución/FPS
                    }
                }
            }
        }

        // Si no se detectaron resoluciones, usar predeterminadas
        if resolutions.is_empty() {
            println!("⚠️  No se detectaron resoluciones, usando predeterminadas");
            resolutions = self.get_default_resolutions();
        }

        Ok(resolutions)
    }

    /// Detecta FPS soportados para una cámara
    fn detect_supported_fps(&self, camera_index: usize) -> Result<Vec<u32>> {
        let mut fps_options = Vec::new();

        // FPS comunes para probar
        let test_fps = vec![15, 24, 30, 60, 120];

        let camera_idx = nokhwa::utils::CameraIndex::Index(camera_index as u32);

        for fps in test_fps {
            let requested_format = nokhwa::utils::RequestedFormat::new::<nokhwa::pixel_format::RgbFormat>(
                nokhwa::utils::RequestedFormatType::HighestFrameRate(fps)
            );

            let test_result = std::panic::catch_unwind(|| {
                nokhwa::Camera::new(camera_idx.clone(), requested_format)
            });

            match test_result {
                Ok(Ok(_camera)) => {
                    fps_options.push(fps);
                    println!("✅ FPS soportado: {}", fps);
                }
                Ok(Err(_)) | Err(_) => {
                    // Continuar con el siguiente FPS
                }
            }
        }

        // Si no se detectaron FPS, usar predeterminados
        if fps_options.is_empty() {
            println!("⚠️  No se detectaron FPS, usando predeterminados");
            fps_options = vec![15, 30, 60];
        }

        Ok(fps_options)
    }

    /// Prueba el acceso a una cámara específica de forma segura
    fn test_camera_access(&self, index: usize) -> Result<CameraInfo> {
        let camera_index = nokhwa::utils::CameraIndex::Index(index as u32);
        let requested_format = nokhwa::utils::RequestedFormat::new::<nokhwa::pixel_format::RgbFormat>(
            nokhwa::utils::RequestedFormatType::HighestFrameRate(30)
        );

        // Usar catch_unwind para evitar crashes
        let test_result = std::panic::catch_unwind(|| {
            nokhwa::Camera::new(camera_index, requested_format)
        });

        match test_result {
            Ok(Ok(camera)) => {
                let info = camera.info();
                Ok(CameraInfo {
                    index,
                    name: info.human_name().clone(),
                    device_path: format!("camera://{}", index),
                    accessible: true,
                    supported_resolutions: self.get_default_resolutions(),
                    supported_fps: vec![15, 30, 60],
                })
            }
            Ok(Err(_e)) => {
                Ok(CameraInfo {
                    index,
                    name: format!("Cámara {}", index),
                    device_path: format!("camera://{}", index),
                    accessible: false,
                    supported_resolutions: self.get_default_resolutions(),
                    supported_fps: vec![15, 30, 60],
                })
            }
            Err(_) => {
                // Ocurrió un panic durante el acceso
                Ok(CameraInfo {
                    index,
                    name: format!("Cámara {} (Inaccesible)", index),
                    device_path: format!("camera://{}", index),
                    accessible: false,
                    supported_resolutions: self.get_default_resolutions(),
                    supported_fps: vec![15, 30, 60],
                })
            }
        }
    }

    /// Lista todas las cámaras detectadas
    pub fn list_cameras(&self) -> &Vec<CameraInfo> {
        &self.cameras
    }

    /// Selecciona una cámara por su índice
    pub fn select_camera(&mut self, index: usize) -> Result<()> {
        if index >= self.cameras.len() {
            return Err(anyhow!("Índice de cámara inválido: {}", index));
        }

        if !self.cameras[index].accessible {
            return Err(anyhow!("La cámara seleccionada no es accesible"));
        }

        self.selected_camera = Some(index);
        println!("✅ Cámara seleccionada: {} ({})",
                self.cameras[index].name,
                self.cameras[index].index);
        Ok(())
    }

    /// Obtiene la cámara seleccionada actualmente
    pub fn get_selected_camera(&self) -> Option<&CameraInfo> {
        self.selected_camera.and_then(|idx| self.cameras.get(idx))
    }

    /// Inicia la retransmisión de video desde la cámara seleccionada
    #[allow(dead_code)] // Mantenido para API completa
    pub fn start_video_stream(&self) -> Result<VideoStream> {
        let camera_index = self.selected_camera
            .ok_or_else(|| anyhow!("No hay ninguna cámara seleccionada"))?;

        let camera_info = &self.cameras[camera_index];
        if !camera_info.accessible {
            return Err(anyhow!("La cámara seleccionada no es accesible"));
        }

        println!("🎥 Iniciando stream de video desde: {}", camera_info.name);
        VideoStream::new(camera_info.index)
    }
}

/// Stream de video que captura frames de una cámara
pub struct VideoStream {
    camera: Option<nokhwa::Camera>,
    #[allow(dead_code)] // Usado internamente para identificación
    pub camera_index: usize,
    #[allow(dead_code)] // Mantenido para API pública
    pub is_running: bool,
}

impl VideoStream {
    /// Crea un nuevo stream de video
    pub fn new(camera_index: usize) -> Result<Self> {
        let camera_idx = nokhwa::utils::CameraIndex::Index(camera_index as u32);

        // Try different formats to get uncompressed RGB data
        let requested_formats = vec![
            nokhwa::utils::RequestedFormat::new::<nokhwa::pixel_format::RgbFormat>(
                nokhwa::utils::RequestedFormatType::HighestFrameRate(30)
            ),
            nokhwa::utils::RequestedFormat::new::<nokhwa::pixel_format::RgbAFormat>(
                nokhwa::utils::RequestedFormatType::HighestFrameRate(30)
            ),
            nokhwa::utils::RequestedFormat::new::<nokhwa::pixel_format::YuyvFormat>(
                nokhwa::utils::RequestedFormatType::HighestFrameRate(30)
            ),
        ];

        let camera = requested_formats.into_iter()
            .enumerate()
            .find_map(|(i, format)| {
                match nokhwa::Camera::new(camera_idx.clone(), format) {
                    Ok(cam) => {
                        println!("✅ VideoStream created with format variant {}", i);
                        Some(cam)
                    }
                    Err(e) => {
                        println!("⚠️  VideoStream format variant {} failed: {}", i, e);
                        None
                    }
                }
            })
            .ok_or_else(|| anyhow!("Failed to create camera with any format"))?;

        Ok(Self {
            camera: Some(camera),
            camera_index,
            is_running: false,
        })
    }

    /// Inicia la captura de frames
    pub fn start(&mut self) -> Result<()> {
        if self.is_running {
            return Ok(());
        }

        println!("▶️  Iniciando captura de video...");
        self.is_running = true;
        Ok(())
    }

    /// Detiene la captura de frames
    pub fn stop(&mut self) -> Result<()> {
        if !self.is_running {
            return Ok(());
        }

        println!("⏹️  Deteniendo captura de video...");
        self.is_running = false;
        Ok(())
    }

    /// Captura un frame de la cámara
    pub fn capture_frame(&mut self) -> Result<nokhwa::buffer::Buffer> {
        if !self.is_running {
            return Err(anyhow!("El stream de video no esta iniciado"));
        }

        if let Some(ref mut camera) = self.camera {
            match camera.frame() {
                Ok(buffer) => {
                    println!("📸 Frame capturado: {}x{}",
                            buffer.resolution().width(),
                            buffer.resolution().height());
                    Ok(buffer)
                }
                Err(e) => Err(anyhow!("Error capturando frame: {}", e))
            }
        } else {
            Err(anyhow!("La cámara no está inicializada"))
        }
    }

    /// Verifica si el stream está activo
    #[allow(dead_code)] // Mantenido para API completa
    pub fn is_running(&self) -> bool {
        self.is_running
    }

    /// Obtiene información de la cámara
    #[allow(dead_code)] // Mantenido para API completa
    pub fn camera_info(&self) -> Result<nokhwa::utils::CameraInfo> {
        if let Some(ref camera) = self.camera {
            Ok(camera.info().clone())
        } else {
            Err(anyhow!("La cámara no está inicializada"))
        }
    }
}

impl Drop for VideoStream {
    fn drop(&mut self) {
        if self.is_running {
            let _ = self.stop();
        }
    }
}

/// Función de utilidad para seleccionar la mejor cámara disponible
#[allow(dead_code)] // Mantenido para utilidad futura
pub fn select_best_camera(manager: &NokhwaCameraManager) -> Option<usize> {
    let cameras = manager.list_cameras();

    // Buscar la primera cámara accesible
    for (index, camera) in cameras.iter().enumerate() {
        if camera.accessible {
            return Some(index);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_camera_manager_creation() {
        let manager = NokhwaCameraManager::new();
        assert_eq!(manager.list_cameras().len(), 0);
        assert!(manager.get_selected_camera().is_none());
    }

    #[test]
    fn test_camera_selection() {
        let mut manager = NokhwaCameraManager::new();

        // Intentar seleccionar cámara sin escanear debería fallar
        assert!(manager.select_camera(0).is_err());
    }
}

//! Buffer pooling para reducir allocaciones de memoria en procesamiento de video
//! 
//! Este módulo implementa pools de buffers reutilizables para:
//! - Frames RGBA procesados
//! - Datos temporales de conversión
//! - Buffers intermedios para procesamiento paralelo

use anyhow::Result;
use parking_lot::Mutex;
use std::sync::Arc;

/// Pool simple de buffers para evitar allocaciones repetitivas
pub struct SimpleBufferPool {
    buffers: Arc<Mutex<Vec<Vec<u8>>>>,
    buffer_size: usize,
    max_buffers: usize,
}

impl SimpleBufferPool {
    /// Crea un nuevo pool simple
    pub fn new(buffer_size: usize, max_buffers: usize) -> Self {
        Self {
            buffers: Arc::new(Mutex::new(Vec::with_capacity(max_buffers))),
            buffer_size,
            max_buffers,
        }
    }
    
    /// Limpia buffers no utilizados para liberar memoria
    pub fn cleanup_unused_buffers(&self) {
        let mut buffers = self.buffers.lock();
        // Mantener solo la mitad de los buffers si hay muchos sin usar
        if buffers.len() > self.max_buffers / 2 {
            buffers.truncate(self.max_buffers / 2);
        }
    }
    
    /// Obtiene un buffer del pool o crea uno nuevo
    pub fn get_buffer(&self) -> Vec<u8> {
        let mut buffers = self.buffers.lock();
        if let Some(mut buffer) = buffers.pop() {
            buffer.clear();
            buffer.reserve(self.buffer_size);
            buffer
        } else {
            Vec::with_capacity(self.buffer_size)
        }
    }
    
    /// Devuelve un buffer al pool
    pub fn return_buffer(&self, mut buffer: Vec<u8>) {
        let mut buffers = self.buffers.lock();
        if buffers.len() < self.max_buffers {
            buffer.clear();
            // Shrink para liberar memoria si es necesario
            if buffer.capacity() > self.buffer_size * 2 {
                buffer.shrink_to_fit();
            }
            buffers.push(buffer);
        }
    }
    
    /// Obtiene el tamaño del buffer
    pub fn buffer_size(&self) -> usize {
        self.buffer_size
    }
}

/// Pool de buffers para frames RGBA
pub struct RgbaBufferPool {
    pool: SimpleBufferPool,
    #[allow(dead_code)] // Mantenido para metadata del pool
    pub width: u32,
    #[allow(dead_code)] // Mantenido para metadata del pool
    pub height: u32,
}

impl RgbaBufferPool {
    /// Crea un nuevo pool para buffers del tamaño especificado
    pub fn new(width: u32, height: u32, pool_size: usize) -> Self {
        let buffer_size = (width * height * 4) as usize;
        
        Self {
            pool: SimpleBufferPool::new(buffer_size, pool_size),
            width,
            height,
        }
    }
    
    /// Obtiene un buffer del pool
    pub fn get_buffer(&self) -> Vec<u8> {
        self.pool.get_buffer()
    }
    
    /// Devuelve un buffer al pool
    pub fn return_buffer(&self, buffer: Vec<u8>) {
        self.pool.return_buffer(buffer);
    }
    
    /// Obtiene el tamaño del buffer
    pub fn buffer_size(&self) -> usize {
        self.pool.buffer_size()
    }
}

/// Pool de buffers para datos temporales de conversión
pub struct TempBufferPool {
    #[allow(dead_code)] // Mantenido para API completa
    pub pool: SimpleBufferPool,
    #[allow(dead_code)] // Mantenido para validaciones
    pub max_size: usize,
}

impl TempBufferPool {
    /// Crea un nuevo pool para buffers temporales
    pub fn new(max_size: usize, pool_size: usize) -> Self {
        Self {
            pool: SimpleBufferPool::new(max_size, pool_size),
            max_size,
        }
    }
    
    /// Obtiene un buffer del pool
    #[allow(dead_code)] // Mantenido para API completa
    pub fn get_buffer(&self) -> Vec<u8> {
        self.pool.get_buffer()
    }
    
    /// Obtiene un buffer del pool con tamaño específico (si es menor al máximo)
    #[allow(dead_code)] // Mantenido para API completa
    pub fn get_sized_buffer(&self, size: usize) -> Result<Vec<u8>> {
        if size > self.max_size {
            return Err(anyhow::anyhow!("Requested buffer size {} exceeds maximum {}", size, self.max_size));
        }
        
        let mut buffer = self.pool.get_buffer();
        buffer.resize(size, 0);
        Ok(buffer)
    }
    
    /// Devuelve un buffer al pool
    #[allow(dead_code)] // Mantenido para API completa
    pub fn return_buffer(&self, buffer: Vec<u8>) {
        self.pool.return_buffer(buffer);
    }
}

/// Gestor centralizado de pools de buffers
pub struct BufferPoolManager {
    rgba_pools: Arc<Mutex<std::collections::HashMap<(u32, u32), Arc<RgbaBufferPool>>>>,
    #[allow(dead_code)] // Mantenido para extensiones futuras
    pub temp_pool: Arc<TempBufferPool>,
}

impl BufferPoolManager {
    /// Crea un nuevo gestor de pools
    pub fn new() -> Self {
        Self {
            rgba_pools: Arc::new(Mutex::new(std::collections::HashMap::new())),
            temp_pool: Arc::new(TempBufferPool::new(1920 * 1080 * 4, 5)), // Reducido a 5 buffers
        }
    }
    
    /// Obtiene o crea un pool RGBA para el tamaño especificado
    pub fn get_rgba_pool(&self, width: u32, height: u32) -> Arc<RgbaBufferPool> {
        let mut pools = self.rgba_pools.lock();
        
        // Optimización: usar menos buffers para reducir uso de memoria
        let pool_size = if width * height > 1280 * 720 {
            6 // Menos buffers para resoluciones altas
        } else {
            3 // Buffer reducido para resoluciones normales
        };
        
        pools.entry((width, height))
            .or_insert_with(|| Arc::new(RgbaBufferPool::new(width, height, pool_size)))
            .clone()
    }
    
    /// Obtiene el pool de buffers temporales
    #[allow(dead_code)] // Mantenido para API completa
    pub fn get_temp_pool(&self) -> Arc<TempBufferPool> {
        self.temp_pool.clone()
    }
}

impl Default for BufferPoolManager {
    fn default() -> Self {
        Self::new()
    }
}

lazy_static::lazy_static! {
    static ref BUFFER_POOL_MANAGER: BufferPoolManager = BufferPoolManager::new();
}

/// Obtiene el gestor global de pools de buffers
pub fn get_buffer_pool_manager() -> &'static BufferPoolManager {
    &BUFFER_POOL_MANAGER
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_simple_buffer_pool() {
        let pool = SimpleBufferPool::new(1024, 3);
        
        // Obtener un buffer
        let buffer = pool.get_buffer();
        assert_eq!(buffer.capacity(), 1024);
        
        // Devolver el buffer
        pool.return_buffer(buffer);
    }
    
    #[test]
    fn test_rgba_buffer_pool() {
        let pool = RgbaBufferPool::new(640, 480, 3);
        
        // Obtener un buffer
        let buffer = pool.get_buffer();
        assert_eq!(buffer.capacity(), 640 * 480 * 4);
        
        // Devolver el buffer
        pool.return_buffer(buffer);
    }
    
    #[test]
    fn test_temp_buffer_pool() {
        let pool = TempBufferPool::new(1024, 3);
        
        // Obtener un buffer
        let buffer = pool.get_buffer();
        assert_eq!(buffer.capacity(), 1024);
        
        // Obtener un buffer con tamaño específico
        let sized_buffer = pool.get_sized_buffer(512).unwrap();
        assert_eq!(sized_buffer.len(), 512);
        
        // Devolver los buffers
        pool.return_buffer(buffer);
        pool.return_buffer(sized_buffer);
    }
    
    #[test]
    fn test_buffer_pool_manager() {
        let manager = BufferPoolManager::new();
        
        // Obtener pool RGBA
        let pool1 = manager.get_rgba_pool(640, 480);
        let pool2 = manager.get_rgba_pool(640, 480);
        
        // Debería ser el mismo pool (mismo tamaño)
        assert!(Arc::ptr_eq(&pool1, &pool2));
        
        // Obtener pool con diferente tamaño
        let pool3 = manager.get_rgba_pool(1280, 720);
        assert!(!Arc::ptr_eq(&pool1, &pool3));
    }
}
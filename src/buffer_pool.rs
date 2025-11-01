//! Buffer pooling para reducir allocaciones de memoria en procesamiento de video
//!
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

/// Gestor centralizado de pools de buffers
pub struct BufferPool {
    buffers_pool: HashMap<usize, Vec<u8>>,
}

impl BufferPool {
    pub fn new() -> Self {
        Self {
            buffers_pool: HashMap::new(),
        }
    }

    pub fn get_buffer(&mut self, size: usize) -> &mut [u8] {
        self.buffers_pool
            .entry(size)
            .or_insert_with(|| vec![0; size])
            .as_mut_slice()
    }
}

pub static BUFFER_POOL: LazyLock<Arc<Mutex<BufferPool>>> =
    LazyLock::new(|| Arc::new(Mutex::new(BufferPool::new())));

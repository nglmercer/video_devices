# Rust Video Devices

A high-performance camera capture and display application built with Rust, featuring optimized frame processing and a modern Slint UI.

## Overview

This project provides a simplified camera system using the Nokhwa library for cross-platform camera access, with advanced performance optimizations including:

- **Async frame processing** with adaptive frame skipping
- **SIMD optimizations** for image processing
- **Buffer pooling** for memory efficiency
- **Optimized Slint renderer** with frame caching
- **Windows permission handling** for camera access
- **Real-time FPS monitoring** and performance metrics

## Features

- 🎥 **Multi-camera support** - Detect and switch between available cameras
- 🚀 **High performance** - Optimized rendering pipeline with 30+ FPS capability
- 🔄 **Async processing** - Non-blocking frame capture and processing
- 📊 **Performance monitoring** - Real-time FPS and performance metrics
- 🖼️ **Modern UI** - Clean, responsive interface built with Slint
- 🔧 **Cross-platform** - Works on Windows, Linux, and macOS
- 🛡️ **Error handling** - Robust error handling and recovery

## Architecture

The application is structured into several key modules:

### Core Modules

- **[`main.rs`](src/main.rs)** - Application entry point and UI logic
- **[`nokhwa_camera.rs`](src/nokhwa_camera.rs)** - Camera detection and management
- **[`async_frame_handler.rs`](src/async_frame_handler.rs)** - Asynchronous frame processing
- **[`slint_renderer.rs`](src/slint_renderer.rs)** - Optimized rendering for Slint UI
- **[`buffer_pool.rs`](src/buffer_pool.rs)** - Memory-efficient buffer management
- **[`frame_processor.rs`](src/frame_processor.rs)** - Image processing and transformations

### Platform-Specific

- **[`windows_permissions.rs`](src/windows_permissions.rs)** - Windows camera permission handling
- **[`ui/camera_view.slint`](ui/camera_view.slint)** - Slint UI definition

## Requirements

- Rust 1.70+ (2021 edition)
- Camera device (USB webcam, integrated camera, etc.)
- For Windows: Camera permissions must be granted

## Installation

1. Clone the repository:
```bash
git clone <repository-url>
cd rust_video_devices
```

2. Build the project:
```bash
cargo build --release
```

3. Run the application:
```bash
cargo run --release
```

## Usage

1. **Launch the application** - The UI will open and automatically scan for available cameras
2. **Select a camera** - Choose from the dropdown list of detected cameras
3. **Start streaming** - Click the "Start" button to begin video capture
4. **Monitor performance** - View real-time FPS and status information
5. **Stop streaming** - Click "Stop" to end video capture

## Performance Features

### Optimized Rendering Pipeline

The application includes two rendering modes:

- **Performance Mode (Default)**: Uses async frame processing with adaptive frame skipping
- **Legacy Mode**: Synchronous frame processing with enhanced rendering

### Memory Optimization

- **Buffer Pooling**: Reuses memory buffers to reduce allocations
- **SIMD Processing**: Uses SIMD instructions for faster image processing
- **Frame Caching**: Intelligent caching to reduce redundant processing

### Adaptive Quality

- **Dynamic Frame Skipping**: Automatically skips frames when processing can't keep up
- **Quality Scaling**: Adjusts processing quality based on performance
- **Memory Management**: Efficient memory usage with automatic cleanup

## Development

### Building

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release

# Run tests
cargo test
```

### Code Structure

The project follows a modular architecture:

- **Camera Management**: Abstraction over Nokhwa for camera operations
- **Frame Processing**: Pipeline for capturing, processing, and rendering frames
- **UI Integration**: Slint-based user interface with real-time updates
- **Performance Optimization**: Various optimization techniques for smooth video

### Adding New Features

1. **Camera Features**: Extend [`nokhwa_camera.rs`](src/nokhwa_camera.rs) for new camera capabilities
2. **Processing**: Modify [`frame_processor.rs`](src/frame_processor.rs) for new image processing
3. **UI**: Update [`ui/camera_view.slint`](ui/camera_view.slint) for interface changes
4. **Performance**: Tune [`async_frame_handler.rs`](src/async_frame_handler.rs) for processing optimizations

## Troubleshooting

### Windows Camera Permissions

If camera access is denied on Windows:

1. Check Windows Settings → Privacy → Camera
2. Ensure "Allow apps to access your camera" is enabled
3. Grant permission for the application or for desktop apps

### Performance Issues

If experiencing low FPS:

1. Ensure "Performance Mode" is enabled (default)
2. Check camera resolution settings
3. Close other applications using the camera
4. Verify USB bandwidth for external cameras

### Camera Not Detected

1. Refresh the camera list using the "Refresh" button
2. Check camera connections and drivers
3. Verify camera is not in use by another application
4. Restart the application

## Dependencies

### Core Dependencies

- **[`nokhwa`](https://crates.io/crates/nokhwa)** - Cross-platform camera access
- **[`slint`](https://crates.io/crates/slint)** - Modern UI framework
- **[`tokio`](https://crates.io/crates/tokio)** - Async runtime
- **[`image`](https://crates.io/crates/image)** - Image processing

### Performance Dependencies

- **[`rayon`](https://crates.io/crates/rayon)** - Data parallelism
- **[`wide`](https://crates.io/crates/wide)** - SIMD operations
- **[`parking_lot`](https://crates.io/crates/parking_lot)** - High-performance synchronization

### Platform-Specific

- **[`windows`](https://crates.io/crates/windows)** - Windows APIs (Windows only)

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add some amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

## Acknowledgments

- **[Nokhwa](https://github.com/l1npengtul/nokhwa)** - For the excellent camera abstraction library
- **[Slint](https://slint.dev/)** - For the modern UI framework
- **[Rust Community](https://www.rust-lang.org/community)** - For the amazing ecosystem
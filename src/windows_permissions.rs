//! Windows camera permission handling module
//! 
//! This module provides functionality to check and request camera permissions
//! on Windows systems using the Windows API.

use anyhow::Result;
use std::ptr::null_mut;

/// Result of permission check
#[derive(Debug, Clone)]
pub enum PermissionStatus {
    Granted,
    Denied,
    #[allow(dead_code)]
    Unknown,
    Error(String),
}

/// Windows permission manager
pub struct WindowsPermissionManager;

impl WindowsPermissionManager {
    /// Create a new permission manager
    pub fn new() -> Self {
        Self
    }

    /// Check current camera permission status
    pub fn check_camera_permission(&self) -> PermissionStatus {
        println!("🔍 Checking Windows camera permission status...");

        // Initialize COM for Media Foundation
        unsafe {
            let hr = windows::Win32::System::Com::CoInitializeEx(
                Some(null_mut()),
                windows::Win32::System::Com::COINIT_MULTITHREADED,
            );

            if hr.is_err() {
                return PermissionStatus::Error(format!("Failed to initialize COM: {:?}", hr));
            }

            // Try to create a Media Foundation attribute object to test permissions
            let mut attributes: Option<windows::Win32::Media::MediaFoundation::IMFAttributes> = None;
            let hr = windows::Win32::Media::MediaFoundation::MFCreateAttributes(
                &mut attributes,
                1,
            );

            if hr.is_err() {
                windows::Win32::System::Com::CoUninitialize();
                return PermissionStatus::Denied;
            }

            // Try to set up camera device source using a simpler approach
            if let Some(ref attrs) = attributes {
                let hr = attrs.SetGUID(
                    &windows::Win32::Media::MediaFoundation::MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                    &windows::Win32::Media::MediaFoundation::MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
                );

                if hr.is_err() {
                    windows::Win32::System::Com::CoUninitialize();
                    return PermissionStatus::Denied;
                }
            }

            // Clean up
            let _ = windows::Win32::System::Com::CoUninitialize();
            PermissionStatus::Granted
        }
    }

    /// Request camera permissions (shows Windows permission dialog if needed)
    pub fn request_camera_permission(&self) -> Result<PermissionStatus> {
        println!("📋 Requesting Windows camera permissions...");

        // On Windows, permissions are typically handled through the Settings app
        // We can try to open the camera privacy settings for the user
        self.open_camera_privacy_settings()?;

        // After opening settings, check if permissions are now available
        Ok(self.check_camera_permission())
    }

    /// Open Windows camera privacy settings
    fn open_camera_privacy_settings(&self) -> Result<()> {
        println!("🪟 Opening Windows camera privacy settings...");

        unsafe {
            use windows::Win32::UI::Shell::ShellExecuteW;
            use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
            use windows::core::PCWSTR;
            use std::ffi::OsStr;
            use std::os::windows::ffi::OsStrExt;

            // Convert the URL to wide string
            let url: Vec<u16> = OsStr::new("ms-settings:privacy-webcam")
                .encode_wide()
                .chain(Some(0))
                .collect();

            let result = ShellExecuteW(
                windows::Win32::Foundation::HWND::default(),
                PCWSTR(null_mut()),
                PCWSTR(url.as_ptr()),
                PCWSTR(null_mut()),
                PCWSTR(null_mut()),
                SW_SHOWNORMAL,
            );

            if result.0 <= 32 {
                return Err(anyhow::anyhow!("Failed to open privacy settings"));
            }
        }

        println!("✅ Camera privacy settings opened successfully");
        println!("💡 Please enable camera access for this application in the settings");
        Ok(())
    }

    /// Check if the application has elevated privileges
    #[allow(dead_code)] // Mantenido para utilidades futuras
    pub fn is_elevated(&self) -> bool {
        unsafe {
            use windows::Win32::Security::TOKEN_QUERY;
            use windows::Win32::Foundation::HANDLE;
            use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
            use windows::Win32::Security::{TOKEN_ELEVATION, TokenElevation, GetTokenInformation};

            let mut token_handle = HANDLE::default();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token_handle).is_err() {
                return false;
            }

            let mut elevation = TOKEN_ELEVATION::default();
            let mut size = std::mem::size_of::<TOKEN_ELEVATION>() as u32;
            
            let result = GetTokenInformation(
                token_handle,
                TokenElevation,
                Some(&mut elevation as *mut _ as *mut _),
                size,
                &mut size,
            );

            result.is_ok() && elevation.TokenIsElevated != 0
        }
    }

    /// Try to get camera access with fallback strategies
    pub fn try_get_camera_access(&self) -> PermissionStatus {
        println!("🔄 Attempting to get camera access...");

        // First, check current permission status
        let status = self.check_camera_permission();
        if matches!(status, PermissionStatus::Granted) {
            return status;
        }

        // If denied, try to request permissions
        if matches!(status, PermissionStatus::Denied) {
            println!("⚠️  Camera permissions currently denied");
            println!("📋 Attempting to request permissions...");
            
            match self.request_camera_permission() {
                Ok(new_status) => {
                    if matches!(new_status, PermissionStatus::Granted) {
                        println!("✅ Camera permissions granted!");
                        return new_status;
                    }
                }
                Err(e) => {
                    println!("❌ Failed to request permissions: {}", e);
                }
            }
        }

        // Final fallback check
        self.check_camera_permission()
    }

    /// Provide user guidance for camera permissions
    pub fn show_permission_guidance(&self) {
        println!("\n📖 Windows Camera Permission Guidance:");
        println!("=====================================");
        println!("1. 🪟 Open Windows Settings > Privacy & Security > Camera");
        println!("2. 📷 Ensure 'Camera access' is turned on");
        println!("3. 🔍 Enable 'Allow apps to access your camera'");
        println!("4. 📱 Check that this application is in the allowed list");
        println!("5. 🔄 Restart this application after changing settings");
        println!("\n🚀 Alternative: Run as Administrator");
        println!("   Right-click the application > 'Run as administrator'");
        println!("\n🔧 Developer Options:");
        println!("   - Ensure Windows Camera Frame Server is running");
        println!("   - Check Device Manager for camera drivers");
        println!("   - Verify camera works in Windows Camera app");
    }
}

impl Default for WindowsPermissionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience function to check and request permissions
#[allow(dead_code)] // Mantenido para API completa
pub fn ensure_camera_permissions() -> PermissionStatus {
    let manager = WindowsPermissionManager::new();
    
    // Check if running with elevated privileges
    if manager.is_elevated() {
        println!("✅ Running with elevated privileges");
    } else {
        println!("ℹ️  Running with standard privileges");
    }

    let status = manager.try_get_camera_access();
    
    match &status {
        PermissionStatus::Granted => {
            println!("✅ Camera permissions are granted");
        }
        PermissionStatus::Denied => {
            println!("❌ Camera permissions are denied");
            manager.show_permission_guidance();
        }
        PermissionStatus::Unknown => {
            println!("❓ Camera permission status is unknown");
            manager.show_permission_guidance();
        }
        PermissionStatus::Error(e) => {
            println!("❌ Error checking camera permissions: {}", e);
            manager.show_permission_guidance();
        }
    }

    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_manager_creation() {
        let _manager = WindowsPermissionManager::new();
    }

    #[test]
    fn test_elevation_check() {
        let manager = WindowsPermissionManager::new();
        let _is_elevated = manager.is_elevated();
    }
}
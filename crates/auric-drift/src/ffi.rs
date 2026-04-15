//! C-compatible FFI for consuming auric-drift from Swift/Objective-C.

use crate::types::DriftConfig;
use crate::analyzer::DriftAnalyzer;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr;

#[repr(C)]
pub struct CDriftFeatures {
    pub bpm: f32,
    pub key: i32,
    pub energy: f32,
    pub brightness: f32,
    pub dynamic_range: f32,
    pub error: *mut c_char,
}

#[repr(C)]
pub struct CDriftConfig {
    pub artist_separation: u32,
    pub album_separation: u32,
    pub genre_separation: u32,
    pub freshness_decay_hours: f64,
    pub skip_penalty_weight: f64,
    pub discovery_boost: f64,
    pub genre_transition_smoothing: bool,
    pub harmonic_mixing: bool,
    pub harmonic_weight: f64,
    pub bpm_continuity: bool,
    pub max_bpm_delta: f32,
    pub energy_smoothing: bool,
    pub max_energy_delta: f32,
    pub brightness_smoothing: bool,
    pub max_brightness_delta: f32,
}

impl From<CDriftConfig> for DriftConfig {
    fn from(c: CDriftConfig) -> Self {
        Self {
            artist_separation: c.artist_separation as usize,
            album_separation: c.album_separation as usize,
            genre_separation: c.genre_separation as usize,
            freshness_decay_hours: c.freshness_decay_hours,
            skip_penalty_weight: c.skip_penalty_weight,
            discovery_boost: c.discovery_boost,
            genre_transition_smoothing: c.genre_transition_smoothing,
            harmonic_mixing: c.harmonic_mixing,
            harmonic_weight: c.harmonic_weight,
            bpm_continuity: c.bpm_continuity,
            max_bpm_delta: c.max_bpm_delta,
            energy_smoothing: c.energy_smoothing,
            max_energy_delta: c.max_energy_delta,
            brightness_smoothing: c.brightness_smoothing,
            max_brightness_delta: c.max_brightness_delta,
        }
    }
}

#[no_mangle]
pub extern "C" fn auric_drift_analyze_file(path: *const c_char) -> CDriftFeatures {
    let path = match unsafe { CStr::from_ptr(path) }.to_str() {
        Ok(s) => s,
        Err(_) => {
            return CDriftFeatures {
                bpm: 0.0, key: 0, energy: 0.0, brightness: 0.0, dynamic_range: 0.0,
                error: CString::new("invalid UTF-8 path").unwrap().into_raw(),
            };
        }
    };

    let analyzer = DriftAnalyzer::new();
    match analyzer.analyze_file(std::path::Path::new(path)) {
        Ok(f) => CDriftFeatures {
            bpm: f.bpm, key: f.key, energy: f.energy,
            brightness: f.brightness, dynamic_range: f.dynamic_range,
            error: ptr::null_mut(),
        },
        Err(e) => CDriftFeatures {
            bpm: 0.0, key: 0, energy: 0.0, brightness: 0.0, dynamic_range: 0.0,
            error: CString::new(e.to_string()).unwrap().into_raw(),
        },
    }
}

#[no_mangle]
pub extern "C" fn auric_drift_default_config() -> CDriftConfig {
    let d = DriftConfig::default();
    CDriftConfig {
        artist_separation: d.artist_separation as u32,
        album_separation: d.album_separation as u32,
        genre_separation: d.genre_separation as u32,
        freshness_decay_hours: d.freshness_decay_hours,
        skip_penalty_weight: d.skip_penalty_weight,
        discovery_boost: d.discovery_boost,
        genre_transition_smoothing: d.genre_transition_smoothing,
        harmonic_mixing: d.harmonic_mixing,
        harmonic_weight: d.harmonic_weight,
        bpm_continuity: d.bpm_continuity,
        max_bpm_delta: d.max_bpm_delta,
        energy_smoothing: d.energy_smoothing,
        max_energy_delta: d.max_energy_delta,
        brightness_smoothing: d.brightness_smoothing,
        max_brightness_delta: d.max_brightness_delta,
    }
}

#[no_mangle]
pub extern "C" fn auric_drift_free_string(s: *mut c_char) {
    if !s.is_null() {
        unsafe { drop(CString::from_raw(s)) };
    }
}

//! The app's logic without Tauri. The Tauri app (apps/app/src-tauri) and the browser build both
//! use these modules; only the thin command layer around them differs.

pub mod assets;
pub mod convert;
pub mod editor;
pub mod journey;
pub mod settings;
pub mod sply;
pub mod transfer;
pub mod views;
pub mod workbook;

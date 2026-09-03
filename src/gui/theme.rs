// The palette everything in the GUI paints with.

use eframe::egui;

pub const LIGHT_SQUARE: egui::Color32 = egui::Color32::from_rgb(240, 217, 181);
pub const DARK_SQUARE: egui::Color32 = egui::Color32::from_rgb(181, 136, 99);
pub const SELECTED_OUTLINE: egui::Color32 = egui::Color32::from_rgb(246, 246, 105);
pub const LEGAL_TARGET_DOT: egui::Color32 = egui::Color32::from_rgba_premultiplied(20, 20, 20, 110);

pub const APP_BG: egui::Color32 = egui::Color32::from_rgb(24, 23, 22);
pub const PANEL_BG: egui::Color32 = egui::Color32::from_rgb(37, 35, 33);
pub const PANEL_BORDER: egui::Color32 = egui::Color32::from_rgb(60, 56, 51);
pub const TEXT_PRIMARY: egui::Color32 = egui::Color32::from_rgb(242, 238, 231);
pub const TEXT_MUTED: egui::Color32 = egui::Color32::from_rgb(168, 158, 146);
pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(226, 178, 118);

// the two sides, for the disc that shows whose move it is
pub const WHITE_SIDE: egui::Color32 = egui::Color32::from_rgb(238, 236, 230);
pub const BLACK_SIDE: egui::Color32 = egui::Color32::from_rgb(44, 41, 38);

// how urgent a line reads at a glance: the game is running, someone is in check,
// or the game is over
pub const CALM: egui::Color32 = egui::Color32::from_rgb(126, 186, 128);
pub const WARNING: egui::Color32 = egui::Color32::from_rgb(232, 168, 84);
pub const DANGER: egui::Color32 = egui::Color32::from_rgb(214, 106, 96);

// one colour per search number, so the panel can be read without looking at labels
pub const STAT_TIME: egui::Color32 = egui::Color32::from_rgb(132, 172, 214);
pub const STAT_SPEED: egui::Color32 = egui::Color32::from_rgb(126, 186, 128);
pub const STAT_EVAL: egui::Color32 = egui::Color32::from_rgb(186, 162, 214);

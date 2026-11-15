//! Chat UI layout state, views, tabs and styling options

use crate::session::ChatMessage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextEffect {
    #[default]
    None,
    Outline,
    Shadow,
}

impl TextEffect {
    pub fn next(self) -> Self {
        match self {
            Self::None => Self::Outline,
            Self::Outline => Self::Shadow,
            Self::Shadow => Self::None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Выкл.",
            Self::Outline => "Обводка",
            Self::Shadow => "Тень",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "none" | "off" => Some(Self::None),
            "outline" => Some(Self::Outline),
            "shadow" | "drop_shadow" => Some(Self::Shadow),
            _ => None,
        }
    }
}

pub fn clamp_outline_width(width: f32) -> f32 {
    if width.is_finite() { width.clamp(0.5, 4.0) } else { 1.5 }
}

pub const SETTINGS_CONTENT_HEIGHT: f32 = 520.0;

pub fn max_settings_scroll(panel_height: u32) -> f32 {
    (SETTINGS_CONTENT_HEIGHT - panel_height as f32).max(0.0)
}

#[derive(Default)]
pub struct HeaderFade {
    progress: f32,
    pub hovered: bool,
}

impl HeaderFade {
    pub fn advance(&mut self, seconds: f32) -> f32 {
        let step = seconds.max(0.0) / if self.hovered { 0.18 } else { 0.22 };
        self.progress = if self.hovered {
            (self.progress + step).min(1.0)
        } else {
            (self.progress - step).max(0.0)
        };
        self.progress * self.progress * (3.0 - 2.0 * self.progress)
    }

    pub fn is_animating(&self) -> bool {
        if self.hovered { self.progress < 1.0 } else { self.progress > 0.0 }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum UiView {
    Chat,
    Settings,
    Search,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub msg_idx: usize,
    pub timestamp_ms: i64,
    pub sec: f64,
    pub video_path: Option<String>,
    pub time_str: String,
    pub author: String,
    pub color_hex: String,
    pub text: String,
}

pub const SEARCH_RESULTS_TOP: f32 = 90.0;
pub const SEARCH_RESULT_HEIGHT: f32 = 40.0;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SearchHit {
    Message(usize),
    Timestamp(usize),
}

pub fn search_result_hit(ui: &UiState, x: f32, y: f32) -> Option<SearchHit> {
    let (pw, ph) = ui.effective_panel_size();
    if x < 12.0 || x >= pw as f32 - 12.0 || y < SEARCH_RESULTS_TOP || y >= ph as f32 { return None; }
    let relative = y - SEARCH_RESULTS_TOP;
    let row = (relative / SEARCH_RESULT_HEIGHT) as usize;
    if relative % SEARCH_RESULT_HEIGHT >= SEARCH_RESULT_HEIGHT - 4.0 { return None; }
    let index = ui.search_scroll_offset + row;
    if index >= ui.search_results.len() { return None; }
    Some(if x < 82.0 { SearchHit::Timestamp(index) } else { SearchHit::Message(index) })
}

pub const MINIMIZED_SIZE: u32 = 44;
pub const CHANNEL_TEXT_COLOR: [u8; 4] = [191, 148, 255, 255];

fn compute_rel_pos(x: i32, y: i32, w: u32, h: u32, osd_w: i32, osd_h: i32) -> (f32, f32) {
    let avail_x = (osd_w - w as i32).max(0);
    let avail_y = (osd_h - h as i32).max(0);
    let rx = if avail_x > 0 {
        (x as f32 / avail_x as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let ry = if avail_y > 0 {
        (y as f32 / avail_y as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (rx, ry)
}

fn resolve_abs_pos(rel: (f32, f32), w: u32, h: u32, osd_w: i32, osd_h: i32) -> (i32, i32) {
    let avail_x = (osd_w - w as i32).max(0);
    let avail_y = (osd_h - h as i32).max(0);
    let x = ((rel.0.clamp(0.0, 1.0) * avail_x as f32).round() as i32).clamp(0, avail_x);
    let y = ((rel.1.clamp(0.0, 1.0) * avail_y as f32).round() as i32).clamp(0, avail_y);
    (x, y)
}

#[derive(Debug, Clone)]
pub struct UiState {
    // Current Active View
    pub current_view: UiView,

    // Toggles and layout
    pub chat_enabled: bool,
    pub minimized: bool,
    pub minimized_position: Option<(i32, i32)>,
    pub minimized_rel_pos: Option<(f32, f32)>,
    pub emotes_enabled: bool,
    pub badges_enabled: bool,
    pub zebra_striping: bool,
    pub offset_ms: i64,
    pub font_size: f32,
    pub panel_width: u32,
    pub panel_height: u32,
    pub panel_x: i32,
    pub panel_y: i32,
    pub rel_pos: Option<(f32, f32)>,
    pub osd_size: Option<(i32, i32)>,
    pub bg_opacity: f32,
    pub header_opacity: f32,
    pub text_effect: TextEffect,
    pub outline_width: f32,
    pub settings_scroll_offset: f32,

    // Chat scroll state (0 = live auto-scroll, >0 = history/past, <0 = future)
    pub scroll_offset: i32,
    pub chat_scroll_anchor: Option<usize>,
    pub highlighted_message_ts: Option<i64>,

    // Selected message for 1-click sync calibration
    pub selected_sync_msg: Option<ChatMessage>,

    // Search state
    pub search_query: String,
    pub search_results: Vec<SearchResult>,
    pub search_scroll_offset: usize,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            current_view: UiView::Chat,
            chat_enabled: true,
            minimized: false,
            minimized_position: None,
            minimized_rel_pos: None,
            emotes_enabled: true,
            badges_enabled: true,
            zebra_striping: true,
            offset_ms: 0,
            font_size: 14.0,
            panel_width: 340,
            panel_height: 600,
            panel_x: 1470,
            panel_y: 60,
            rel_pos: None,
            osd_size: None,
            bg_opacity: 0.75,
            header_opacity: 0.0,
            text_effect: TextEffect::None,
            outline_width: 1.5,
            settings_scroll_offset: 0.0,
            scroll_offset: 0,
            chat_scroll_anchor: None,
            highlighted_message_ts: None,
            selected_sync_msg: None,
            search_query: String::new(),
            search_results: Vec::new(),
            search_scroll_offset: 0,
        }
    }
}

impl UiState {
    pub fn effective_panel_size(&self) -> (u32, u32) {
        match self.osd_size {
            Some((ow, oh)) if ow > 0 && oh > 0 => (
                self.panel_width.min(ow as u32).max(1),
                self.panel_height.min(oh as u32).max(1),
            ),
            _ => (self.panel_width, self.panel_height),
        }
    }

    pub fn overlay_geometry(&self) -> (i32, i32, u32, u32) {
        if self.minimized {
            let (x, y) = self.minimized_position.unwrap_or((self.panel_x, self.panel_y));
            (x, y, MINIMIZED_SIZE, MINIMIZED_SIZE)
        } else {
            let (w, h) = self.effective_panel_size();
            (self.panel_x, self.panel_y, w, h)
        }
    }

    pub fn minimize(&mut self, osd: (i32, i32)) {
        let (x, y) = self.minimized_position.unwrap_or((self.panel_x, self.panel_y));
        self.minimized_position = Some((x.clamp(0, (osd.0 - MINIMIZED_SIZE as i32).max(0)), y.clamp(0, (osd.1 - MINIMIZED_SIZE as i32).max(0))));
        self.minimized = true;
    }

    pub fn restore(&mut self) {
        self.minimized = false;
    }

    pub fn set_overlay_position(&mut self, x: i32, y: i32) {
        if self.minimized {
            self.minimized_position = Some((x, y));
        } else {
            self.panel_x = x;
            self.panel_y = y;
        }
    }

    pub fn set_overlay_position_in_osd(&mut self, x: i32, y: i32, osd: (i32, i32)) {
        self.set_overlay_position(x, y);
        if osd.0 > 0 && osd.1 > 0 {
            self.osd_size = Some(osd);
            if self.minimized {
                self.minimized_rel_pos = Some(compute_rel_pos(
                    x,
                    y,
                    MINIMIZED_SIZE,
                    MINIMIZED_SIZE,
                    osd.0,
                    osd.1,
                ));
            } else {
                let (ew, eh) = self.effective_panel_size();
                self.rel_pos = Some(compute_rel_pos(x, y, ew, eh, osd.0, osd.1));
            }
        }
    }

    /// Updates `osd_size` and recalculates `panel_x`, `panel_y`, and `minimized_position`
    /// from relative coordinates so the overlay stays at the same relative position across
    /// fullscreen/windowed transitions and window resizes. Returns `true` if geometry changed.
    pub fn apply_osd_bounds(&mut self, osd_w: i32, osd_h: i32) -> bool {
        if osd_w <= 0 || osd_h <= 0 {
            return false;
        }

        let prev_panel = (self.panel_x, self.panel_y);
        let prev_eff = self.effective_panel_size();
        let prev_min = self.minimized_position;

        self.osd_size = Some((osd_w, osd_h));
        let (eff_w, eff_h) = self.effective_panel_size();

        let rel = *self.rel_pos.get_or_insert_with(|| {
            let fits = self.panel_x >= 0
                && self.panel_y >= 0
                && self.panel_x + eff_w as i32 <= osd_w
                && self.panel_y + eff_h as i32 <= osd_h;
            let (ref_w, ref_h) = if fits {
                (osd_w, osd_h)
            } else {
                (
                    osd_w.max(1920).max(self.panel_x + self.panel_width as i32),
                    osd_h.max(1080).max(self.panel_y + self.panel_height as i32),
                )
            };
            compute_rel_pos(self.panel_x, self.panel_y, self.panel_width, self.panel_height, ref_w, ref_h)
        });

        let (nx, ny) = resolve_abs_pos(rel, eff_w, eff_h, osd_w, osd_h);
        self.panel_x = nx;
        self.panel_y = ny;

        if let Some((mx, my)) = self.minimized_position {
            let m_rel = *self.minimized_rel_pos.get_or_insert_with(|| {
                let fits = mx >= 0
                    && my >= 0
                    && mx + MINIMIZED_SIZE as i32 <= osd_w
                    && my + MINIMIZED_SIZE as i32 <= osd_h;
                let (ref_w, ref_h) = if fits {
                    (osd_w, osd_h)
                } else {
                    (
                        osd_w.max(1920).max(mx + MINIMIZED_SIZE as i32),
                        osd_h.max(1080).max(my + MINIMIZED_SIZE as i32),
                    )
                };
                compute_rel_pos(mx, my, MINIMIZED_SIZE, MINIMIZED_SIZE, ref_w, ref_h)
            });
            self.minimized_position = Some(resolve_abs_pos(
                m_rel,
                MINIMIZED_SIZE,
                MINIMIZED_SIZE,
                osd_w,
                osd_h,
            ));
        }

        prev_panel != (self.panel_x, self.panel_y)
            || prev_eff != (eff_w, eff_h)
            || prev_min != self.minimized_position
    }

    /// Applies chat overlay settings from a JSON object (supporting both global `config.json`
    /// and legacy per-session `chat_settings.json` keys). Returns `true` if at least one
    /// recognized overlay setting key was present and applied.
    pub fn apply_settings_json(&mut self, val: &serde_json::Value) -> bool {
        let mut matched = false;
        let mut pos_overridden = false;
        let mut min_pos_overridden = false;

        if let Some(x) = val.get("chat_x").and_then(|v| v.as_i64()) {
            self.panel_x = x as i32;
            pos_overridden = true;
            matched = true;
        }
        if let Some(y) = val.get("chat_y").and_then(|v| v.as_i64()) {
            self.panel_y = y as i32;
            pos_overridden = true;
            matched = true;
        }
        if pos_overridden {
            self.rel_pos = None;
        }
        if let (Some(rx), Some(ry)) = (
            val.get("chat_rel_x").and_then(|v| v.as_f64()),
            val.get("chat_rel_y").and_then(|v| v.as_f64()),
        ) {
            if rx.is_finite() && ry.is_finite() {
                self.rel_pos = Some(((rx as f32).clamp(0.0, 1.0), (ry as f32).clamp(0.0, 1.0)));
                matched = true;
            }
        }
        if let Some(w) = val.get("chat_width").and_then(|v| v.as_u64()) {
            self.panel_width = w as u32;
            matched = true;
        }
        if let Some(h) = val.get("chat_height").and_then(|v| v.as_u64()) {
            self.panel_height = h as u32;
            matched = true;
        }
        if let Some(fs) = val.get("chat_font_size").and_then(|v| v.as_f64()) {
            self.font_size = fs as f32;
            matched = true;
        }
        if let Some(op) = val.get("chat_bg_opacity").and_then(|v| v.as_f64()) {
            self.bg_opacity = (op as f32).clamp(0.0, 1.0);
            matched = true;
        }
        if let Some(enabled) = val
            .get("chat_enabled")
            .or_else(|| val.get("default_chat_enabled"))
            .and_then(|v| v.as_bool())
        {
            self.chat_enabled = enabled;
            matched = true;
        }
        if let Some(enabled) = val
            .get("emotes_enabled")
            .or_else(|| val.get("default_emotes_enabled"))
            .and_then(|v| v.as_bool())
        {
            self.emotes_enabled = enabled;
            matched = true;
        }
        if let Some(enabled) = val.get("badges_enabled").and_then(|v| v.as_bool()) {
            self.badges_enabled = enabled;
            matched = true;
        }
        if let Some(zebra) = val
            .get("zebra_striping")
            .or_else(|| val.get("chat_zebra_striping"))
            .and_then(|v| v.as_bool())
        {
            self.zebra_striping = zebra;
            matched = true;
        }
        if let Some(effect) = val
            .get("chat_text_effect")
            .or_else(|| val.get("text_effect"))
            .and_then(|v| v.as_str())
            .and_then(TextEffect::parse)
        {
            self.text_effect = effect;
            matched = true;
        }
        if let Some(position) = val.get("chat_minimized_position").and_then(|v| v.as_array()) {
            if let (Some(x), Some(y)) = (
                position.first().and_then(|v| v.as_i64()),
                position.get(1).and_then(|v| v.as_i64()),
            ) {
                self.minimized_position = Some((x as i32, y as i32));
                min_pos_overridden = true;
                matched = true;
            }
        }
        if min_pos_overridden {
            self.minimized_rel_pos = None;
        }
        if let Some(rel_position) = val.get("chat_minimized_rel_position").and_then(|v| v.as_array()) {
            if let (Some(rx), Some(ry)) = (
                rel_position.first().and_then(|v| v.as_f64()),
                rel_position.get(1).and_then(|v| v.as_f64()),
            ) {
                if rx.is_finite() && ry.is_finite() {
                    self.minimized_rel_pos = Some(((rx as f32).clamp(0.0, 1.0), (ry as f32).clamp(0.0, 1.0)));
                    matched = true;
                }
            }
        }
        if let Some(width) = val
            .get("chat_outline_width")
            .or_else(|| val.get("text_effect_thickness"))
            .and_then(|v| v.as_f64())
        {
            self.outline_width = clamp_outline_width(width as f32);
            matched = true;
        }

        matched
    }
}

#[derive(Debug, Copy, Clone)]
pub struct ChatViewport {
    pub top: f32,
    pub bottom: f32,
    pub base_y: f32,
}

impl ChatViewport {
    pub fn new(ui: &UiState, total_message_height: f32) -> Self {
        let (_, ph) = ui.effective_panel_size();
        let top = 32.0;
        let banner_height = if ui.scroll_offset != 0 || ui.chat_scroll_anchor.is_some() { 28.0 } else { 0.0 };
        let bottom = (ph as f32 - banner_height - 10.0).max(top);
        let available = bottom - top;
        let base_y = if total_message_height > available {
            bottom - total_message_height
        } else {
            top
        };
        Self { top, bottom, base_y }
    }

    pub fn contains_y(&self, y: f32) -> bool {
        y >= self.top && y < self.bottom
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_hits_distinguish_timestamp_and_message() {
        let result = SearchResult { msg_idx: 0, timestamp_ms: 0, sec: 0.0, video_path: None, time_str: "00:00".into(), author: "user".into(), color_hex: String::new(), text: "text".into() };
        let ui = UiState { search_results: vec![result.clone(), result], ..Default::default() };
        assert_eq!(search_result_hit(&ui, 20.0, 95.0), Some(SearchHit::Timestamp(0)));
        assert_eq!(search_result_hit(&ui, 100.0, 115.0), Some(SearchHit::Message(0)));
        assert_eq!(search_result_hit(&ui, 100.0, 135.0), Some(SearchHit::Message(1)));
        assert_eq!(search_result_hit(&ui, 100.0, 70.0), None);
        assert_eq!(search_result_hit(&ui, 100.0, 127.0), None);
        assert_eq!(search_result_hit(&ui, 100.0, 200.0), None);
    }

    #[test]
    fn minimized_position_is_independent_of_restored_panel() {
        let mut ui = UiState { panel_x: 100, panel_y: 60, ..Default::default() };
        ui.minimize((800, 700));
        assert_eq!(ui.overlay_geometry(), (100, 60, 44, 44));
        ui.set_overlay_position(300, 200);
        ui.restore();
        assert_eq!(ui.overlay_geometry(), (100, 60, 340, 600));
        ui.minimize((800, 700));
        assert_eq!(ui.overlay_geometry(), (300, 200, 44, 44));
        ui.minimize((200, 150));
        assert_eq!(ui.overlay_geometry(), (156, 106, 44, 44));
    }

    #[test]
    fn header_fade_appears_disappears_and_reverses_smoothly() {
        let mut fade = HeaderFade::default();
        assert_eq!(fade.advance(0.1), 0.0);
        assert!(!fade.is_animating());
        fade.hovered = true;
        assert!(fade.is_animating());
        let halfway = fade.advance(0.09);
        assert!((halfway - 0.5).abs() < 0.001);
        assert_eq!(fade.advance(0.1), 1.0);
        assert!(!fade.is_animating());
        fade.hovered = false;
        let fading = fade.advance(0.11);
        assert!((fading - 0.5).abs() < 0.001);
        fade.hovered = true;
        let reversed = fade.advance(0.02);
        assert!(reversed > fading && reversed < 1.0);
        fade.hovered = false;
        assert_eq!(fade.advance(1.0), 0.0);
        assert!(!fade.is_animating());
    }

    #[test]
    fn text_effect_modes_and_width_limits_are_stable() {
        assert_eq!(TextEffect::None.next(), TextEffect::Outline);
        assert_eq!(TextEffect::Outline.next(), TextEffect::Shadow);
        assert_eq!(TextEffect::Shadow.next(), TextEffect::None);
        assert_eq!(TextEffect::parse("outline"), Some(TextEffect::Outline));
        assert_eq!(TextEffect::parse("unexpected"), None);
        assert_eq!(clamp_outline_width(10.0), 4.0);
        assert_eq!(clamp_outline_width(-1.0), 0.5);
        assert_eq!(clamp_outline_width(f32::NAN), 1.5);
        assert_eq!(serde_json::to_string(&TextEffect::Shadow).unwrap(), "\"shadow\"");
        assert_eq!(max_settings_scroll(220), 300.0);
    }

    #[test]
    fn chat_viewport_excludes_header_and_scroll_banner() {
        let mut ui = UiState { panel_height: 220, ..UiState::default() };
        let live = ChatViewport::new(&ui, 500.0);
        assert_eq!(live.top, 32.0);
        assert_eq!(live.bottom, 210.0);
        assert!(!live.contains_y(25.0));
        assert!(live.contains_y(32.0));
        assert!(!live.contains_y(216.0));
        assert_eq!(live.base_y + 500.0, live.bottom);
        ui.scroll_offset = 4;
        let history = ChatViewport::new(&ui, 500.0);
        assert_eq!(history.bottom, 182.0);
        assert!(!history.contains_y(200.0));
        ui.panel_height = 10;
        let tiny = ChatViewport::new(&ui, 500.0);
        assert!(!tiny.contains_y(32.0));
    }

    #[test]
    fn osd_bounds_preserve_relative_position_across_fullscreen_and_windowed() {
        let mut ui = UiState::default();
        assert!(!ui.apply_osd_bounds(1920, 1080));
        assert_eq!((ui.panel_x, ui.panel_y), (1470, 60));

        // Switch to 1280x720 windowed: keeps proportional position instead of overflowing
        assert!(ui.apply_osd_bounds(1280, 720));
        assert_eq!((ui.panel_x, ui.panel_y), (875, 15));
        assert!(ui.panel_x + ui.panel_width as i32 <= 1280);
        assert!(ui.panel_y + ui.panel_height as i32 <= 720);

        // Return to 1920x1080 fullscreen: restores exact original coordinates
        assert!(ui.apply_osd_bounds(1920, 1080));
        assert_eq!((ui.panel_x, ui.panel_y), (1470, 60));

        // Minimized badge position also survives fullscreen <-> windowed round-trip
        ui.minimize((1920, 1080));
        assert!(!ui.apply_osd_bounds(1920, 1080));
        assert_eq!(ui.minimized_position, Some((1470, 60)));
        assert!(ui.apply_osd_bounds(1280, 720));
        assert_eq!(ui.minimized_position, Some((969, 39)));
        assert!(ui.apply_osd_bounds(1920, 1080));
        assert_eq!(ui.minimized_position, Some((1470, 60)));
    }

    #[test]
    fn initial_default_position_opening_directly_in_small_window_stays_inside_bounds() {
        let mut ui = UiState::default();
        assert!(ui.apply_osd_bounds(1280, 720));
        assert_eq!((ui.panel_x, ui.panel_y), (875, 15));
        assert_eq!(ui.overlay_geometry(), (875, 15, 340, 600));
    }

    #[test]
    fn small_window_clamps_effective_size_without_mutating_configured_size() {
        let mut ui = UiState { panel_width: 500, panel_height: 600, ..Default::default() };
        assert!(ui.apply_osd_bounds(400, 350));
        assert_eq!(ui.effective_panel_size(), (400, 350));
        assert_eq!((ui.panel_width, ui.panel_height), (500, 600));
        assert_eq!(ui.overlay_geometry(), (0, 0, 400, 350));

        assert!(ui.apply_osd_bounds(1920, 1080));
        assert_eq!(ui.effective_panel_size(), (500, 600));
    }

    #[test]
    fn settings_json_loads_and_overrides_relative_positions() {
        let mut ui = UiState::default();
        ui.set_overlay_position_in_osd(1470, 60, (1920, 1080));
        ui.minimize((1920, 1080));
        ui.apply_osd_bounds(1920, 1080);
        assert!(ui.rel_pos.is_some());
        assert!(ui.minimized_rel_pos.is_some());

        // Legacy absolute coordinates without relative fields reset stale relative positions
        let legacy = serde_json::json!({
            "chat_x": 100,
            "chat_y": 80,
            "chat_minimized_position": [40, 50]
        });
        assert!(ui.apply_settings_json(&legacy));
        assert_eq!((ui.panel_x, ui.panel_y), (100, 80));
        assert_eq!(ui.rel_pos, None);
        assert_eq!(ui.minimized_position, Some((40, 50)));
        assert_eq!(ui.minimized_rel_pos, None);

        // Explicit relative fields are loaded, clamped to 0..=1, and used on OSD sync
        let relative = serde_json::json!({
            "chat_x": 100,
            "chat_y": 80,
            "chat_rel_x": 0.5,
            "chat_rel_y": 1.5,
            "chat_minimized_position": [40, 50],
            "chat_minimized_rel_position": [-0.2, 0.25]
        });
        assert!(ui.apply_settings_json(&relative));
        assert_eq!(ui.rel_pos, Some((0.5, 1.0)));
        assert_eq!(ui.minimized_rel_pos, Some((0.0, 0.25)));
    }
}

/// Helper struct for button bounds and click detection
#[derive(Debug, Copy, Clone)]
pub struct HitBox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl HitBox {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
}

use eframe::egui;
use mc630_plt_studio::sheet::{CcdTrial, CutShape, Layout};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Unit {
    #[default]
    Inches,
    Cm,
    Mm,
}
impl Unit {
    pub fn factor(self) -> f64 {
        match self {
            Self::Inches => 25.4,
            Self::Cm => 10.,
            Self::Mm => 1.,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Inches => "in",
            Self::Cm => "cm",
            Self::Mm => "mm",
        }
    }
    pub fn number(self, mm: f64) -> String {
        format!("{:.3}", mm / self.factor())
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
    pub fn size(self, w: f64, h: f64) -> String {
        format!("{} x {} {}", self.number(w), self.number(h), self.label())
    }
    pub fn parse(self, text: &str) -> Option<f64> {
        text.trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .map(|v| v * self.factor())
    }
    pub fn distance<'a>(self, value: &'a mut f64) -> egui::DragValue<'a> {
        egui::DragValue::new(value)
            .update_while_editing(false)
            .speed(0.1)
            .suffix(format!(" {}", self.label()))
            .custom_formatter(move |v, _| self.number(v))
            .custom_parser(move |s| self.parse(s))
    }
    pub fn slider<'a>(
        self,
        value: &'a mut f64,
        range: std::ops::RangeInclusive<f64>,
    ) -> egui::Slider<'a> {
        egui::Slider::new(value, range)
            .suffix(format!(" {}", self.label()))
            .custom_formatter(move |v, _| self.number(v))
            .custom_parser(move |s| self.parse(s))
    }
}
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Preferences {
    #[serde(default)]
    pub reopen_last_project: bool,
    pub unit: Unit,
    pub layout: Layout,
    pub ccd: bool,
    pub profile: CcdTrial,
    pub cut_shape: CutShape,
    pub remove_white: bool,
    pub threshold: u8,
    pub background_tolerance: u8,
    pub print_without_background: bool,
    pub rulers: bool,
    pub grid: bool,
    pub show_cut: bool,
    pub lock: bool,
    pub preview_origin: bool,
    pub nudge_fine: f64,
    pub nudge_coarse: f64,
    pub zoom: f32,
    pub contour_rgba: [u8; 4],
    #[serde(default = "autosave_default")]
    pub autosave_enabled: bool,
    #[serde(default = "autosave_seconds_default")]
    pub autosave_seconds: u64,
}
pub fn autosave_default() -> bool {
    true
}
pub fn autosave_seconds_default() -> u64 {
    15
}
pub fn path() -> std::path::PathBuf {
    std::env::var_os("APPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("MC630-PLT-Studio")
        .join("settings.json")
}
impl Preferences {
    pub fn validate(&self) -> Result<(), String> {
        self.validate_values()?;
        self.layout.items()?;
        Ok(())
    }
    pub fn validate_values(&self) -> Result<(), String> {
        self.layout.capacity()?;
        if self.layout.count == 0
            || self.layout.count > 1000
            || self.layout.custom.len() > 1000
            || self
                .layout
                .hidden_images
                .iter()
                .any(|&i| i >= self.layout.count)
            || self.layout.guides.iter().enumerate().any(|(axis, guides)| {
                let length = [self.layout.page_width, self.layout.page_height][axis];
                guides.len() > 100 || guides.iter().any(|v| !(0. ..=length).contains(v))
            })
            || !(5..=300).contains(&self.autosave_seconds)
        {
            return Err("Invalid recovery/settings limits.".into());
        }
        if !self.zoom.is_finite() || !(0.5..=16.).contains(&self.zoom) {
            return Err("Invalid zoom setting.".into());
        }
        if !(0.0..=4.).contains(&self.layout.smoothing)
            || !(-5.0..=5.).contains(&self.layout.contour_offset)
            || !(1.0..=10.).contains(&self.profile.diameter)
            || !(5.0..=30.).contains(&self.profile.inset)
            || !(0.001..=100.).contains(&self.nudge_fine)
            || !(0.001..=100.).contains(&self.nudge_coarse)
        {
            return Err("Invalid settings values.".into());
        }
        Ok(())
    }
    pub fn save(&self) -> Result<(), String> {
        self.validate()?;
        let path = path();
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(
            path,
            serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }
    #[cfg(not(test))]
    pub fn load() -> Result<Self, String> {
        let p: Self = serde_json::from_slice(&std::fs::read(path()).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        p.validate()?;
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distances_convert_and_parse_in_all_units() {
        assert_eq!(Unit::default(), Unit::Inches);
        assert_eq!(Unit::Inches.number(25.4), "1");
        assert_eq!(Unit::Cm.number(25.4), "2.54");
        assert_eq!(Unit::Mm.number(25.4), "25.4");
        assert_eq!(Unit::Inches.parse("1"), Some(25.4));
        assert_eq!(Unit::Cm.parse("2.54"), Some(25.4));
        assert_eq!(Unit::Mm.parse("25.4"), Some(25.4));
        for unit in [Unit::Inches, Unit::Cm, Unit::Mm] {
            assert!(unit.parse("NaN").is_none());
            assert!(unit.parse("inf").is_none());
            for mm in [-7., 0., 0.1, 35., 210., 297.] {
                let roundtrip = unit.parse(&unit.number(mm)).unwrap();
                assert!((roundtrip - mm).abs() <= unit.factor() * 0.00051);
            }
        }
    }
}

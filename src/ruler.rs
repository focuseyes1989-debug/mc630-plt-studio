use crate::preferences::Unit;

/// Screen-dependent subdivisions; document geometry remains in millimetres.
pub struct RulerScale {
    pub step_mm: f64,
    divisions: i64,
}

impl RulerScale {
    pub fn new(unit: Unit, pixels_per_mm: f64) -> Self {
        let pixels_per_unit = pixels_per_mm.max(0.0001) * unit.factor();
        let target = 60. / pixels_per_unit;
        let (major, divisions) = match unit {
            Unit::Inches => {
                let major = 2_f64.powf(target.log2().ceil());
                let divisions = [16, 8, 4, 2, 1]
                    .into_iter()
                    .find(|d| major * pixels_per_unit / *d as f64 >= 6.)
                    .unwrap_or(1);
                (major, divisions)
            }
            Unit::Cm | Unit::Mm => {
                let power = 10_f64.powf(target.log10().floor());
                let major = [1., 2., 5., 10.]
                    .into_iter()
                    .map(|n| n * power)
                    .find(|n| *n >= target)
                    .unwrap();
                let divisions = [10, 5, 2, 1]
                    .into_iter()
                    .find(|d| major * pixels_per_unit / *d as f64 >= 6.)
                    .unwrap_or(1);
                (major, divisions)
            }
        };
        Self {
            step_mm: major * unit.factor() / divisions as f64,
            divisions,
        }
    }

    pub fn labeled(&self, index: i64) -> bool {
        index % self.divisions == 0
    }

    pub fn length(&self, index: i64) -> f32 {
        if self.labeled(index) {
            10.
        } else if self.divisions % 2 == 0 && index % (self.divisions / 2) == 0 {
            7.
        } else {
            4.
        }
    }

    pub fn visible(
        &self,
        start_mm: f64,
        end_mm: f64,
        page_mm: f64,
    ) -> std::ops::RangeInclusive<i64> {
        let first = (start_mm.max(0.) / self.step_mm).ceil() as i64;
        let last = (end_mm.min(page_mm) / self.step_mm).floor() as i64;
        first..=last
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zoom_reveals_finer_ticks_in_every_unit() {
        for unit in [Unit::Inches, Unit::Cm, Unit::Mm] {
            let normal = RulerScale::new(unit, 4.);
            let zoomed = RulerScale::new(unit, 64.);
            assert!(zoomed.step_mm < normal.step_mm);
            for pixels in [0.2, 1., 4., 16., 64., 200.] {
                let ruler = RulerScale::new(unit, pixels);
                assert!(ruler.step_mm * pixels >= 6. - 1e-8);
                assert!(ruler.step_mm * ruler.divisions as f64 * pixels >= 60. - 1e-8);
            }
        }
    }
    #[test]
    fn visible_ticks_are_clipped_to_view_and_paper() {
        let ruler = RulerScale::new(Unit::Mm, 64.);
        let ticks: Vec<_> = ruler.visible(100., 110., 105.).collect();
        assert!(!ticks.is_empty());
        assert!(ticks.iter().all(|i| {
            let mm = *i as f64 * ruler.step_mm;
            (100. ..=105.).contains(&mm)
        }));
        assert!(ruler.visible(220., 230., 210.).next().is_none());
    }
}

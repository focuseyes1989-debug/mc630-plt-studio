use std::{path::PathBuf, time::Instant};

pub static PROCESS_START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

pub struct Session {
    started: Instant,
    ready: Option<Instant>,
    last: Instant,
    samples: Vec<Vec<f64>>,
    frame: usize,
    memory: Vec<serde_json::Value>,
    output: PathBuf,
    renderer: &'static str,
}

impl Session {
    pub fn new(renderer: &'static str, output: PathBuf) -> Self {
        let started = *PROCESS_START.get_or_init(Instant::now);
        Self {
            started,
            ready: None,
            last: started,
            samples: vec![vec![]; 4],
            frame: 0,
            memory: vec![],
            output,
            renderer,
        }
    }

    pub fn step(&mut self) -> Option<(bool, f32, [f32; 2])> {
        let now = Instant::now();
        if self.ready.is_none() {
            self.ready = Some(now);
            self.last = now;
        }
        let phase = self.frame / 180;
        if phase >= 4 {
            return None;
        }
        if self.frame % 180 > 15 {
            self.samples[phase].push(now.duration_since(self.last).as_secs_f64() * 1000.);
        }
        self.last = now;
        if self.frame % 180 == 120 {
            self.memory.push(memory_snapshot());
        }
        let t = (self.frame % 180) as f32 / 180. * std::f32::consts::TAU;
        self.frame += 1;
        let zoom = if phase % 2 == 0 {
            1.5 + 0.5 * t.sin()
        } else {
            2.
        };
        let pan = if phase % 2 == 1 {
            [180. * t.sin(), 120. * t.cos()]
        } else {
            [0.; 2]
        };
        Some((phase >= 2, zoom, pan))
    }

    pub fn finish(&mut self) {
        let names = ["main_zoom", "main_pan", "back_zoom", "back_pan"];
        let mut phases = serde_json::Map::new();
        for (name, values) in names.into_iter().zip(&mut self.samples) {
            values.sort_by(f64::total_cmp);
            if values.is_empty() {
                continue;
            }
            phases.insert(name.into(), serde_json::json!({
                "frames": values.len(),
                "median_interval_ms": values[values.len()/2],
                "p95_interval_ms": values[(values.len() * 95 / 100).min(values.len()-1)],
                "over_33ms_percent": values.iter().filter(|&&v| v > 33.34).count() as f64 / values.len() as f64 * 100.,
            }));
        }
        let result = serde_json::json!({
            "renderer": self.renderer,
            "open_ready_ms": self.ready.unwrap_or(self.started).duration_since(self.started).as_secs_f64() * 1000.,
            "phases": phases,
            "memory_by_phase": self.memory,
            "notes": "Frame intervals include UI preparation and presentation; continuous repaint replay. Programmatic camera replay, no input-to-photon measurement."
        });
        if let Err(error) =
            std::fs::write(&self.output, serde_json::to_vec_pretty(&result).unwrap())
        {
            eprintln!("Cannot save benchmark: {error}");
        }
    }
}

pub fn arguments(flag: &str) -> Option<(PathBuf, PathBuf)> {
    let args: Vec<_> = std::env::args_os().collect();
    let index = args.iter().position(|arg| arg == flag)?;
    Some((args.get(index + 1)?.into(), args.get(index + 2)?.into()))
}

fn memory_snapshot() -> serde_json::Value {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::{ProcessStatus::*, Threading::GetCurrentProcess};
        let mut counters = PROCESS_MEMORY_COUNTERS_EX::default();
        counters.cb = std::mem::size_of_val(&counters) as u32;
        if GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
            counters.cb,
        )
        .is_ok()
        {
            let mb = |bytes: usize| bytes as f64 / 1048576.;
            return serde_json::json!({
                "working_set_mib": mb(counters.WorkingSetSize),
                "private_mib": mb(counters.PrivateUsage),
                "peak_working_set_mib": mb(counters.PeakWorkingSetSize),
            });
        }
    }
    serde_json::Value::Null
}

//! Cava audio visualizer reader.
//!
//! Spawns `cava` as a child process with a generated config that writes raw
//! binary bar data to stdout. A background thread continuously reads frames
//! into a shared buffer. `bars()` returns the latest snapshot at any time —
//! never blocks, never closes cava's pipe.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

pub struct CavaReader {
    bars: Arc<Mutex<Vec<f32>>>,
    n:    usize,
}

impl CavaReader {
    /// Spawn cava and start reading. Returns None if cava isn't installed.
    pub fn start(n_bars: usize) -> Option<Self> {
        let cfg = format!(
            "[general]\nbars = {n}\nsleep_timer = 0\n\
             [input]\nmethod = pipewire\n\
             [output]\nmethod = raw\nraw_target = /dev/stdout\n\
             data_format = binary\nbits = 8\n\
             [smoothing]\nmonstercat = 1\nintegration = 0.7\n",
            n = n_bars
        );

        // Write config to a tmpfile (cava needs -p path, not stdin in all versions).
        let cfg_path = "/tmp/woven-cava.ini";
        if std::fs::write(cfg_path, &cfg).is_err() {
            warn!("cava: could not write /tmp/woven-cava.ini");
            return None;
        }

        let child = Command::new("cava")
            .arg("-p").arg(cfg_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();

        let mut child = match child {
            Ok(c)  => c,
            Err(e) => { warn!("cava: spawn failed: {}", e); return None; }
        };

        let stdout = match child.stdout.take() {
            Some(s) => s,
            None    => { warn!("cava: no stdout"); return None; }
        };

        let bars = Arc::new(Mutex::new(vec![0.0f32; n_bars]));
        let bars_bg = bars.clone();

        std::thread::Builder::new()
            .name("cava-reader".into())
            .spawn(move || {
                info!("cava reader thread started ({} bars)", n_bars);
                let mut reader = std::io::BufReader::new(stdout);
                let mut buf    = vec![0u8; n_bars];
                loop {
                    match reader.read_exact(&mut buf) {
                        Ok(()) => {
                            if let Ok(mut v) = bars_bg.lock() {
                                for (i, &b) in buf.iter().enumerate() {
                                    v[i] = b as f32 / 255.0;
                                }
                            }
                        }
                        Err(_) => {
                            warn!("cava reader: pipe closed, cava may have exited");
                            // Zero out bars so the visualizer goes dark cleanly.
                            if let Ok(mut v) = bars_bg.lock() {
                                v.iter_mut().for_each(|x| *x = 0.0);
                            }
                            break;
                        }
                    }
                }
                // Reap the child so it doesn't become a zombie.
                let _ = child.wait();
            })
            .ok()?;

        Some(Self { bars, n: n_bars })
    }

    /// Returns the latest bar values (0.0–1.0), one per bar.
    pub fn bars(&self) -> Vec<f32> {
        self.bars.lock().map(|v| v.clone()).unwrap_or_else(|_| vec![0.0; self.n])
    }
}

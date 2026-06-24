use super::{RenderCtx, Widget};
use crate::draw::{fill_rounded_rect, hex_color};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

const BARS: usize = 10;
const MAX_VAL: u8  = 7;
const BAR_W:  f32  = 4.0;
const BAR_GAP: f32 = 2.0;

pub struct CavaWidget {
    values: Arc<Mutex<Vec<u8>>>,
}

impl CavaWidget {
    pub fn new() -> Self {
        let values = Arc::new(Mutex::new(vec![0u8; BARS]));
        let v2 = values.clone();

        std::thread::spawn(move || {
            let cfg = build_cava_config();
            let cfg_path = "/tmp/woven-bar-cava.cfg";
            if std::fs::write(cfg_path, &cfg).is_err() { return; }

            loop {
                let Ok(mut child) = Command::new("cava")
                    .arg("-p").arg(cfg_path)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                else { std::thread::sleep(std::time::Duration::from_secs(2)); continue; };

                let stdout = match child.stdout.take() {
                    Some(s) => s,
                    None    => { let _ = child.kill(); continue; }
                };

                let reader = BufReader::new(stdout);
                for line in reader.lines() {
                    let Ok(line) = line else { break };
                    let parsed: Vec<u8> = line.split(';')
                        .filter_map(|s| s.trim().parse::<u8>().ok())
                        .take(BARS)
                        .collect();
                    if !parsed.is_empty() {
                        if let Ok(mut vals) = v2.lock() {
                            let n = parsed.len().min(vals.len());
                            vals[..n].copy_from_slice(&parsed[..n]);
                        }
                    }
                }

                let _ = child.kill();
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        });

        Self { values }
    }
}

impl Widget for CavaWidget {
    fn width(&self, _theme: &crate::config::Theme, _text: &mut crate::text::TextRenderer) -> u32 {
        ((BAR_W + BAR_GAP) * BARS as f32 - BAR_GAP + 8.0) as u32
    }

    fn render(&mut self, ctx: &mut RenderCtx<'_>, x: f32) {
        let vals = match self.values.lock() {
            Ok(v)  => v.clone(),
            Err(_) => return,
        };

        let h      = ctx.height as f32;
        let accent = hex_color(&ctx.theme.accent);
        let dim    = hex_color(&ctx.theme.dim);

        for (i, &val) in vals.iter().enumerate() {
            let bx    = x + 4.0 + i as f32 * (BAR_W + BAR_GAP);
            let frac  = val as f32 / MAX_VAL as f32;
            let bar_h = (frac * (h - 6.0)).max(2.0);
            let by    = h - bar_h - 3.0;

            fill_rounded_rect(ctx.pixmap, bx, 3.0, BAR_W, h - 6.0, 1.0, dim);
            fill_rounded_rect(ctx.pixmap, bx, by, BAR_W, bar_h, 1.0, accent);
        }
    }
}

fn build_cava_config() -> String {
    format!(
        "[general]\nbars = {BARS}\nframerate = 60\n\n\
         [input]\nmethod = pulse\nsource = auto\n\n\
         [output]\nmethod = raw\nraw_target = /dev/stdout\n\
         data_format = ascii\nascii_max_range = {MAX_VAL}\n"
    )
}

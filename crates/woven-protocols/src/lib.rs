//! woven-protocols: Wayland protocol extensions for the v2 overview.
//!
//! # What lives here
//! - `ScreencopyManager` — spawns a dedicated screencopy thread with its own
//!   Wayland connection.  Send `CaptureRequest`s in, receive `ThumbnailFrame`s
//!   out over crossbeam channels.
//!
//! # Why a separate connection?
//! woven-render already owns the layer-shell Wayland connection.  A second
//! connection for screencopy is explicitly allowed by the Wayland protocol and
//! gives us clean separation — the screencopy thread is entirely independent of
//! the render loop.
//!
//! # Protocol path
//! ```text
//! CaptureRequest { window_id, x, y, w, h }
//!   → zwlr_screencopy_manager_v1.capture_output_region(output, x, y, w, h)
//!   → buffer event  → ShmAlloc + wl_shm_pool + wl_buffer + copy()
//!   → ready event   → ThumbnailFrame { window_id, width, height, stride, data }
//! ```

pub mod screencopy;
pub mod shm;

use std::{sync::Arc, thread::{self, JoinHandle}};

use anyhow::Context;
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use rustix::event::{PollFd, PollFlags};
use rustix::time::Timespec;
use tracing::{error, info};
use wayland_client::Connection;

use crate::screencopy::ScreencopyState;

// ──────────────────────────────────────────────────────────────────────────────
// Public types
// ──────────────────────────────────────────────────────────────────────────────

/// Request a screenshot capture.
///
/// `output_idx` selects which bound `wl_output` to capture from.
/// For single-monitor setups always use `0`.
#[derive(Debug, Clone)]
pub struct CaptureRequest {
    /// Opaque ID passed back unchanged in the resulting `ThumbnailFrame`.
    pub window_id:  u64,
    /// Which output to capture from (0 = primary).
    pub output_idx: usize,
    /// If true, capture the entire output (ignores x/y/w/h).
    /// If false, capture the region described by x/y/w/h.
    pub full_output: bool,
    /// Window position in compositor logical coordinates (ignored when full_output=true).
    pub x: i32,
    pub y: i32,
    /// Window dimensions in compositor logical coordinates (ignored when full_output=true).
    pub w: u32,
    pub h: u32,
}

/// Completed screencopy frame — XRGB8888 pixels at native window resolution.
#[derive(Debug)]
pub struct ThumbnailFrame {
    pub window_id: u64,
    pub width:     u32,
    pub height:    u32,
    /// Row stride in bytes.
    pub stride:    u32,
    /// Pixel data: `stride * height` bytes, XRGB8888 little-endian.
    pub data:      Arc<[u8]>,
}

impl ThumbnailFrame {
    /// Iterate rows as byte slices.
    pub fn rows(&self) -> impl Iterator<Item = &[u8]> {
        let stride = self.stride as usize;
        (0..self.height as usize).map(move |y| {
            &self.data[y * stride..(y + 1) * stride]
        })
    }

    /// Scale down to `(target_w, target_h)` using nearest-neighbour, returning
    /// raw XRGB8888 bytes.  Used by woven-render to produce card thumbnails.
    pub fn scale_nearest(&self, target_w: u32, target_h: u32) -> Vec<u8> {
        let mut out = vec![0u8; (target_w * target_h * 4) as usize];
        let src_w = self.width as f32;
        let src_h = self.height as f32;
        let stride = self.stride as usize;

        for dy in 0..target_h as usize {
            for dx in 0..target_w as usize {
                let sx = ((dx as f32 / target_w as f32) * src_w) as usize;
                let sy = ((dy as f32 / target_h as f32) * src_h) as usize;
                let src_off = sy * stride + sx * 4;
                let dst_off = dy * (target_w as usize * 4) + dx * 4;
                out[dst_off..dst_off + 4]
                .copy_from_slice(&self.data[src_off..src_off + 4]);
            }
        }
        out
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// ScreencopyManager — public handle owned by woven-render
// ──────────────────────────────────────────────────────────────────────────────

pub struct ScreencopyManager {
    pub request_tx: Sender<CaptureRequest>,
    pub frame_rx:   Receiver<ThumbnailFrame>,
    _thread:        JoinHandle<()>,
}

impl ScreencopyManager {
    /// Spawn the screencopy thread.  Returns `Err` if the compositor doesn't
    /// expose `wl_shm` or `zwlr_screencopy_manager_v1`.
    pub fn spawn() -> anyhow::Result<Self> {
        // Unbounded channels — the render thread drains these every frame.
        let (request_tx, request_rx) = crossbeam_channel::unbounded::<CaptureRequest>();
        let (frame_tx, frame_rx)     = crossbeam_channel::unbounded::<ThumbnailFrame>();

        // Verify we can connect before spawning.
        let _ = Connection::connect_to_env()
        .context("screencopy: can't connect to Wayland display")?;

        let thread = thread::Builder::new()
        .name("woven-screencopy".into())
        .spawn(move || {
            if let Err(e) = run(request_rx, frame_tx) {
                error!("screencopy thread exited: {e:#}");
            }
        })
        .context("thread spawn failed")?;

        Ok(Self { request_tx, frame_rx, _thread: thread })
    }

    /// Send a capture request (non-blocking).
    pub fn request(&self, req: CaptureRequest) {
        let _ = self.request_tx.send(req);
    }

    /// Send a batch of capture requests.
    pub fn request_batch(&self, reqs: impl IntoIterator<Item = CaptureRequest>) {
        for r in reqs { let _ = self.request_tx.send(r); }
    }

    /// Drain all completed frames without blocking.
    pub fn drain(&self) -> impl Iterator<Item = ThumbnailFrame> + '_ {
        std::iter::from_fn(|| self.frame_rx.try_recv().ok())
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Thread entry — owns the Wayland connection for screencopy
// ──────────────────────────────────────────────────────────────────────────────

fn run(
    request_rx: Receiver<CaptureRequest>,
    frame_tx:   Sender<ThumbnailFrame>,
) -> anyhow::Result<()> {
    let conn    = Connection::connect_to_env().context("wayland connect")?;
    let display = conn.display();
    let mut queue = conn.new_event_queue::<ScreencopyState>();
    let qh = queue.handle();

    let mut state = ScreencopyState::new(frame_tx);

    // Trigger global enumeration.
    let _registry = display.get_registry(&qh, ());
    queue.roundtrip(&mut state).context("initial roundtrip")?;

    if state.shm.is_none() {
        anyhow::bail!("compositor did not advertise wl_shm");
    }
    if state.screencopy_manager.is_none() {
        anyhow::bail!(
            "compositor did not advertise zwlr_screencopy_manager_v1 — \
Hyprland and Niri both support it; is the compositor running?"
        );
    }
    if state.outputs.is_empty() {
        anyhow::bail!("no wl_output found on screencopy connection");
    }

    info!(
        "screencopy thread ready ({} output(s))",
          state.outputs.len()
    );

    // Socket fd for poll — avoids busy-spinning when idle.
    use std::os::fd::AsFd;
    let conn_fd = conn.as_fd();

    loop {
        // ── 1. Accept incoming capture requests ───────────────────────────
        loop {
            match request_rx.try_recv() {
                Ok(req)                       => state.issue_capture(&req, &qh),
                Err(TryRecvError::Empty)      => break,
                Err(TryRecvError::Disconnected) => {
                    info!("screencopy request channel closed — exiting thread");
                    return Ok(());
                }
            }
        }

        // ── 2. Flush pending outgoing Wayland requests ─────────────────────
        queue.flush().context("queue flush")?;

        // ── 3. Dispatch already-buffered events (non-blocking) ─────────────
        queue.dispatch_pending(&mut state).context("dispatch_pending")?;

        // ── 4. Poll + read from socket ─────────────────────────────────────
        let timeout_ms: i64 = if state.pending_frames.is_empty() { 50 } else { 5 };
        let ts = Timespec { tv_sec: 0, tv_nsec: timeout_ms * 1_000_000 };
        let mut poll_fds = [PollFd::new(&conn_fd, PollFlags::IN)];
        let _ = rustix::event::poll(&mut poll_fds, Some(&ts));

        if let Some(guard) = queue.prepare_read() {
            // read() blocks if there's data; returns quickly if polled readable.
            guard.read().ok();
        }

        queue.dispatch_pending(&mut state).context("dispatch_pending post-read")?;
    }
}

//! Client-side wlr-screencopy-unstable-v1 state machine.
//!
//! Protocol flow per frame:
//!   1. `zwlr_screencopy_manager_v1.capture_output_region(output, x, y, w, h)`
//!      → `ZwlrScreencopyFrameV1` proxy (returned, stored in `live_frames` to
//!      keep the Wayland object alive)
//!   2. Compositor fires `buffer(Xrgb8888, w, h, stride)` event
//!      → We allocate a `ShmAlloc`, create `wl_shm_pool` + `wl_buffer`,
//!      call `frame.copy(buffer)` — all inside the Dispatch handler.
//!   3. Compositor fires `ready` → pixels are in the mmap; we send
//!      `ThumbnailFrame` on the channel and clean up.
//!   4. Compositor fires `failed` → log and clean up silently.
//!
//! Issuing `copy()` inside the `buffer` event handler is explicitly allowed by
//! the wlr-screencopy spec: "The client should issue the copy request after
//! this event."  We skip `buffer_done` gating and go immediately on the first
//! Xrgb8888 SHM format event — also spec-compliant.

use std::{collections::HashMap, sync::Arc};

use std::os::fd::AsFd;

use crossbeam_channel::Sender;
use tracing::{error, warn};
use wayland_client::{
  Connection, Dispatch, Proxy, QueueHandle, WEnum,
  protocol::{
    wl_buffer::{self, WlBuffer},
    wl_output::{self, WlOutput},
    wl_registry::{self, WlRegistry},
    wl_shm::{self, WlShm},
    wl_shm_pool::{self, WlShmPool},
  },
  backend::ObjectId,
};
use wayland_protocols_wlr::screencopy::v1::client::{
  zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1},
  zwlr_screencopy_manager_v1::{self, ZwlrScreencopyManagerV1},
};

use crate::{shm::ShmAlloc, CaptureRequest, ThumbnailFrame};

// ──────────────────────────────────────────────────────────────────────────────
// Per-frame allocation state (keyed by ZwlrScreencopyFrameV1::id())
// ──────────────────────────────────────────────────────────────────────────────

pub(crate) struct PendingFrame {
  pub window_id: u64,
  pub alloc:     ShmAlloc,
  pub pool:      WlShmPool,
  pub wl_buf:    WlBuffer,
  pub width:     u32,
  pub height:    u32,
  pub stride:    u32,
}

// ──────────────────────────────────────────────────────────────────────────────
// Main screencopy client state — one per screencopy Wayland connection
// ──────────────────────────────────────────────────────────────────────────────

pub(crate) struct ScreencopyState {
  pub shm:                Option<WlShm>,
  pub screencopy_manager: Option<ZwlrScreencopyManagerV1>,
  pub outputs:            Vec<WlOutput>,
  /// Scale factor per output (index-matched to outputs). wl_output::scale
  /// gives integer factor (1 = no scale, 2 = HiDPI×2, etc.).
  /// capture_output_region takes PHYSICAL coords; Niri IPC gives LOGICAL coords.
  /// Multiply logical coords × scale before issuing capture.
  pub output_scales:      Vec<i32>,
  pub live_frames:        Vec<ZwlrScreencopyFrameV1>,
  pub pending_frames:     HashMap<ObjectId, PendingFrame>,
  pub frame_tx:           Sender<ThumbnailFrame>,
}

impl ScreencopyState {
  pub fn new(frame_tx: Sender<ThumbnailFrame>) -> Self {
    Self {
      shm:                None,
      screencopy_manager: None,
      outputs:            Vec::new(),
      output_scales:      Vec::new(),
      live_frames:        Vec::new(),
      pending_frames:     HashMap::new(),
      frame_tx,
    }
  }
  
  pub fn issue_capture(&mut self, req: &CaptureRequest, qh: &QueueHandle<Self>) {
    let Some(mgr) = &self.screencopy_manager else {
      warn!("screencopy_manager not yet bound — dropping request");
      return;
    };
    if self.outputs.is_empty() {
      warn!("no wl_output bound yet — dropping capture request");
      return;
    }
    let idx = req.output_idx.min(self.outputs.len() - 1);
    let output = &self.outputs[idx];
    // Scale logical → physical. Niri IPC reports window positions in logical
    // pixels; capture_output_region needs physical output pixels.
    let scale = self.output_scales.get(idx).copied().unwrap_or(1).max(1);
    
    let frame = if req.full_output {
      mgr.capture_output(0, output, qh, req.window_id)
    } else {
      mgr.capture_output_region(
        0, // overlay_cursor = false
        output,
        req.x * scale,
        req.y * scale,
        req.w as i32 * scale,
        req.h as i32 * scale,
        qh,
        req.window_id,
      )
    };
    self.live_frames.push(frame);
  }
}

// ──────────────────────────────────────────────────────────────────────────────
// wl_registry — bind wl_shm, zwlr_screencopy_manager_v1, wl_output
// ──────────────────────────────────────────────────────────────────────────────

impl Dispatch<WlRegistry, ()> for ScreencopyState {
  fn event(
    state: &mut Self,
    registry: &WlRegistry,
    event: wl_registry::Event,
    _: &(),
           _: &Connection,
           qh: &QueueHandle<Self>,
  ) {
    let wl_registry::Event::Global { name, interface, version } = event else {
      return;
    };
    match interface.as_str() {
      "wl_shm" => {
        state.shm = Some(registry.bind(name, version.min(1), qh, ()));
      }
      "zwlr_screencopy_manager_v1" => {
        // v3 adds buffer_done + linux-dmabuf; v1 is the baseline we need.
        state.screencopy_manager =
        Some(registry.bind(name, version.min(3), qh, ()));
      }
      "wl_output" => {
        // v2 adds the `scale` event we need for HiDPI coordinate mapping.
        state.outputs.push(registry.bind(name, version.clamp(2, 4), qh, ()));
      }
      _ => {}
    }
  }
}

// ──────────────────────────────────────────────────────────────────────────────
// wl_shm — ignore format advertisements
// ──────────────────────────────────────────────────────────────────────────────

impl Dispatch<WlShm, ()> for ScreencopyState {
  fn event(_: &mut Self, _: &WlShm, _: wl_shm::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

// ──────────────────────────────────────────────────────────────────────────────
// wl_shm_pool / wl_buffer / wl_output — no events we care about
// ──────────────────────────────────────────────────────────────────────────────

impl Dispatch<WlShmPool, ()> for ScreencopyState {
  fn event(_: &mut Self, _: &WlShmPool, _: wl_shm_pool::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<WlBuffer, ()> for ScreencopyState {
  fn event(_: &mut Self, _: &WlBuffer, _: wl_buffer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<WlOutput, ()> for ScreencopyState {
  fn event(
    state: &mut Self,
    output: &WlOutput,
    event: wl_output::Event,
    _: &(),
           _: &Connection,
           _: &QueueHandle<Self>,
  ) {
    if let wl_output::Event::Scale { factor } = event {
      // Find this output's index and store its scale.
      if let Some(idx) = state.outputs.iter().position(|o| o == output) {
        if state.output_scales.len() <= idx {
          state.output_scales.resize(idx + 1, 1);
        }
        state.output_scales[idx] = factor;
        tracing::debug!("output[{idx}] scale = {factor}");
      }
    }
  }
}

// ──────────────────────────────────────────────────────────────────────────────
// zwlr_screencopy_manager_v1 — no events
// ──────────────────────────────────────────────────────────────────────────────

impl Dispatch<ZwlrScreencopyManagerV1, ()> for ScreencopyState {
  fn event(_: &mut Self, _: &ZwlrScreencopyManagerV1, _: zwlr_screencopy_manager_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

// ──────────────────────────────────────────────────────────────────────────────
// zwlr_screencopy_frame_v1 — the real work
// user data = window_id (u64)
// ──────────────────────────────────────────────────────────────────────────────

impl Dispatch<ZwlrScreencopyFrameV1, u64> for ScreencopyState {
  fn event(
    state: &mut Self,
    frame: &ZwlrScreencopyFrameV1,
    event: zwlr_screencopy_frame_v1::Event,
    window_id: &u64,
    _: &Connection,
    qh: &QueueHandle<Self>,
  ) {
    use zwlr_screencopy_frame_v1::Event;
    
    match event {
      // ── buffer: compositor tells us what size/format SHM to prepare ───
      Event::Buffer { format, width, height, stride } => {
        // Only handle XRGB8888 SHM.  If we've already allocated for
        // this frame (v2+ sends multiple buffer events), skip.
        if format != WEnum::Value(wl_shm::Format::Xrgb8888) {
          return;
        }
        if state.pending_frames.contains_key(&frame.id()) {
          return;
        }
        
        let Some(shm) = &state.shm else {
          error!("wl_shm not bound when buffer event arrived");
          return;
        };
        
        let len = (stride * height) as usize;
        let alloc = match ShmAlloc::new(len) {
          Ok(a)  => a,
          Err(e) => { error!("ShmAlloc failed: {e:#}"); return; }
        };
        
        // wl_shm_pool → wl_buffer
        let pool   = shm.create_pool(alloc.fd.as_fd(), len as i32, qh, ());
        let wl_buf = pool.create_buffer(
          0,
          width as i32, height as i32, stride as i32,
          wl_shm::Format::Xrgb8888,
          qh, (),
        );
        
        // Issue copy — safe to call inside event handler.
        frame.copy(&wl_buf);
        
        state.pending_frames.insert(frame.id(), PendingFrame {
          window_id: *window_id,
          alloc,
          pool,
          wl_buf,
          width, height, stride,
        });
      }
      
      // ── ready: compositor has written pixels into our SHM ─────────────
      Event::Ready { .. } => {
        // Remove live frame proxy → sends `destroy` to compositor.
        state.live_frames.retain(|f| f.id() != frame.id());
        
        if let Some(pf) = state.pending_frames.remove(&frame.id()) {
          let data: Arc<[u8]> = pf.alloc.data().into();
          let _ = state.frame_tx.try_send(ThumbnailFrame {
            window_id: pf.window_id,
            width:     pf.width,
            height:    pf.height,
            stride:    pf.stride,
            data,
          });
          // Clean up Wayland objects.
          pf.wl_buf.destroy();
          pf.pool.destroy();
          // pf.alloc (ShmAlloc) drops here → munmap
        }
      }
      
      // ── failed: compositor couldn't capture this frame ────────────────
      Event::Failed => {
        warn!("screencopy frame failed for window_id={window_id}");
        state.live_frames.retain(|f| f.id() != frame.id());
        if let Some(pf) = state.pending_frames.remove(&frame.id()) {
          pf.wl_buf.destroy();
          pf.pool.destroy();
        }
      }
      
      // ── buffer_done (v2+), linux_dmabuf, flags, damage — ignore ───────
      _ => {}
    }
  }
}

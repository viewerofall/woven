//! Client-side ext-image-copy-capture-v1 state machine (Sway/Hyprland).
//!
//! Protocol flow:
//!   Session lifecycle:
//!   1. Get ext_image_capture_source_v1 from wl_output
//!   2. Create session from source
//!   3. Receive buffer constraints: buffer_size, shm_format, done
//!   4. Allocate SHM buffer once, reuse across captures
//!
//!   Per-frame (one frame per session at a time):
//!   1. Create frame within session
//!   2. Attach buffer to frame
//!   3. Damage entire buffer
//!   4. Send capture request
//!   5. Receive ready/failed, destroy frame

use std::{sync::Arc, os::fd::AsFd};

use crossbeam_channel::{Receiver, Sender};
use tracing::{error, info, warn, debug};
use wayland_client::{
  Connection, Dispatch, QueueHandle, WEnum,
  protocol::{
    wl_buffer::{self, WlBuffer},
    wl_output::{self, WlOutput},
    wl_registry::{self, WlRegistry},
    wl_shm::{self, WlShm},
    wl_shm_pool::{self, WlShmPool},
  },
};
use wayland_protocols::ext::image_capture_source::v1::client::ext_image_capture_source_v1::ExtImageCaptureSourceV1;
use wayland_protocols::ext::image_copy_capture::v1::client::{
  ext_image_copy_capture_manager_v1::{self, ExtImageCopyCaptureManagerV1},
  ext_image_copy_capture_session_v1::{self, ExtImageCopyCaptureSessionV1},
  ext_image_copy_capture_frame_v1::{self, ExtImageCopyCaptureFrameV1},
};
use rustix::time::Timespec;
use rustix::event::{PollFd, PollFlags};

use crate::{shm::ShmAlloc, CaptureRequest, ThumbnailFrame, TryRecvError};

// ──────────────────────────────────────────────────────────────────────────────
// Per-session state (one per output; reused across frames)
// ──────────────────────────────────────────────────────────────────────────────

struct SessionState {
  /// The allocated SHM buffer (reused across frames)
  alloc: Option<ShmAlloc>,
  pool: Option<WlShmPool>,
  wl_buf: Option<WlBuffer>,

  /// Negotiated buffer dimensions from buffer_size event
  width: u32,
  height: u32,
  stride: u32,

  /// Have we received the initial buffer constraints?
  constraints_ready: bool,

  /// If a capture is in progress, the window_id requesting it
  pending_window_id: Option<u64>,
}

impl SessionState {
  fn new() -> Self {
    Self {
      alloc: None,
      pool: None,
      wl_buf: None,
      width: 0,
      height: 0,
      stride: 0,
      constraints_ready: false,
      pending_window_id: None,
    }
  }
}

// ──────────────────────────────────────────────────────────────────────────────
// Main ext_image state machine — one per screencopy Wayland connection
// ──────────────────────────────────────────────────────────────────────────────

pub(crate) struct ExtImageState {
  pub shm: Option<WlShm>,
  pub copy_capture_manager: Option<ExtImageCopyCaptureManagerV1>,

  pub outputs: Vec<WlOutput>,
  pub output_scales: Vec<i32>,
  pub capture_sources: Vec<ExtImageCaptureSourceV1>,
  pub sessions: Vec<ExtImageCopyCaptureSessionV1>,

  /// Per-output session state
  session_states: Vec<SessionState>,

  pub frame_tx: Sender<ThumbnailFrame>,
}

impl ExtImageState {
  pub fn new(frame_tx: Sender<ThumbnailFrame>) -> Self {
    Self {
      shm: None,
      copy_capture_manager: None,
      outputs: Vec::new(),
      output_scales: Vec::new(),
      capture_sources: Vec::new(),
      sessions: Vec::new(),
      session_states: Vec::new(),
      frame_tx,
    }
  }

  pub fn issue_capture(&mut self, req: &CaptureRequest, qh: &QueueHandle<Self>) {
    if self.sessions.is_empty() {
      warn!("no sessions created yet — dropping capture request");
      return;
    }

    let idx = req.output_idx.min(self.sessions.len() - 1);
    let session = &self.sessions[idx];
    let session_state = &mut self.session_states[idx];

    if !session_state.constraints_ready {
      warn!("buffer constraints not yet received for output {idx} — dropping request");
      return;
    }

    if session_state.pending_window_id.is_some() {
      warn!("frame already pending for output {idx} — dropping request");
      return;
    }

    // Create frame for this capture
    let frame = session.create_frame(qh, idx);

    // Attach buffer (must exist from constraints_ready)
    if let Some(buf) = &session_state.wl_buf {
      frame.attach_buffer(buf);
    } else {
      warn!("no buffer allocated for output {idx}");
      return;
    }

    // Damage entire buffer
    frame.damage_buffer(
      0, 0,
      session_state.width as i32,
      session_state.height as i32,
    );

    // Send capture request
    frame.capture();

    // Mark this session as having a pending capture
    session_state.pending_window_id = Some(req.window_id);
  }
}

// ──────────────────────────────────────────────────────────────────────────────
// wl_registry — bind wl_shm, ext_image_copy_capture_manager_v1, wl_output,
//               ext_image_capture_source_v1
// ──────────────────────────────────────────────────────────────────────────────

impl Dispatch<WlRegistry, ()> for ExtImageState {
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

    debug!("global: {} v{}", interface, version);

    match interface.as_str() {
      "wl_shm" => {
        state.shm = Some(registry.bind(name, version.min(1), qh, ()));
      }
      "ext_image_copy_capture_manager_v1" => {
        debug!("binding ext_image_copy_capture_manager_v1 v{}", version.min(1));
        state.copy_capture_manager =
          Some(registry.bind(name, version.min(1), qh, ()));
      }
      "wl_output" => {
        state.outputs.push(registry.bind(name, version.clamp(2, 4), qh, ()));
      }
      "ext_image_capture_source_v1" => {
        debug!("binding ext_image_capture_source_v1");
        state.capture_sources.push(registry.bind(name, version.min(1), qh, ()));
      }
      _ => {}
    }
  }
}

// ──────────────────────────────────────────────────────────────────────────────
// wl_shm — ignore format advertisements
// ──────────────────────────────────────────────────────────────────────────────

impl Dispatch<WlShm, ()> for ExtImageState {
  fn event(_: &mut Self, _: &WlShm, _: wl_shm::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

// ──────────────────────────────────────────────────────────────────────────────
// wl_shm_pool / wl_buffer / wl_output — no events we care about
// ──────────────────────────────────────────────────────────────────────────────

impl Dispatch<WlShmPool, ()> for ExtImageState {
  fn event(_: &mut Self, _: &WlShmPool, _: wl_shm_pool::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<WlBuffer, ()> for ExtImageState {
  fn event(_: &mut Self, _: &WlBuffer, _: wl_buffer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<WlOutput, ()> for ExtImageState {
  fn event(
    state: &mut Self,
    output: &WlOutput,
    event: wl_output::Event,
    _: &(),
    _: &Connection,
    _: &QueueHandle<Self>,
  ) {
    if let wl_output::Event::Scale { factor } = event {
      if let Some(idx) = state.outputs.iter().position(|o| o == output) {
        if state.output_scales.len() <= idx {
          state.output_scales.resize(idx + 1, 1);
        }
        state.output_scales[idx] = factor;
        debug!("output[{idx}] scale = {factor}");
      }
    }
  }
}

// ──────────────────────────────────────────────────────────────────────────────
// ext_image_capture_source_v1 — no events
// ──────────────────────────────────────────────────────────────────────────────

impl Dispatch<ExtImageCaptureSourceV1, ()> for ExtImageState {
  fn event(_: &mut Self, _: &ExtImageCaptureSourceV1, _event: wayland_protocols::ext::image_capture_source::v1::client::ext_image_capture_source_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

// ──────────────────────────────────────────────────────────────────────────────
// ext_image_copy_capture_manager_v1 — no events
// ──────────────────────────────────────────────────────────────────────────────

impl Dispatch<ExtImageCopyCaptureManagerV1, ()> for ExtImageState {
  fn event(_: &mut Self, _: &ExtImageCopyCaptureManagerV1, _: ext_image_copy_capture_manager_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

// ──────────────────────────────────────────────────────────────────────────────
// ext_image_copy_capture_session_v1 — handle buffer constraints
// user data = session index (usize)
// ──────────────────────────────────────────────────────────────────────────────

impl Dispatch<ExtImageCopyCaptureSessionV1, usize> for ExtImageState {
  fn event(
    state: &mut Self,
    _session: &ExtImageCopyCaptureSessionV1,
    event: ext_image_copy_capture_session_v1::Event,
    idx: &usize,
    _: &Connection,
    qh: &QueueHandle<Self>,
  ) {
    use ext_image_copy_capture_session_v1::Event;

    match event {
      Event::BufferSize { width, height } => {
        if let Some(sess_state) = state.session_states.get_mut(*idx) {
          sess_state.width = width;
          sess_state.height = height;
          sess_state.stride = width * 4; // XRGB8888 = 4 bytes/pixel
          debug!("session[{idx}] buffer_size: {width}x{height}, stride={}", sess_state.stride);
        }
      }

      Event::ShmFormat { format } => {
        if format == WEnum::Value(wl_shm::Format::Xrgb8888) {
          debug!("session[{idx}] supports XRGB8888");
        }
      }

      Event::DmabufFormat { .. } => {
        // Ignore DMA-buf for now; we only support SHM
      }

      Event::DmabufDevice { .. } => {
        // Ignore DMA-buf device
      }

      Event::Done => {
        // Buffer constraints sent; now allocate buffer
        if let Some(sess_state) = state.session_states.get_mut(*idx) {
          if sess_state.width > 0 && sess_state.height > 0 && !sess_state.constraints_ready {
            let len = (sess_state.stride * sess_state.height) as usize;
            let alloc = match ShmAlloc::new(len) {
              Ok(a) => a,
              Err(e) => {
                error!("ShmAlloc failed for session[{idx}]: {e:#}");
                return;
              }
            };

            let Some(shm) = &state.shm else {
              error!("wl_shm not bound when done event arrived");
              return;
            };

            let pool = shm.create_pool(
              alloc.fd.as_fd(),
              len as i32,
              qh,
              (),
            );

            let wl_buf = pool.create_buffer(
              0,
              sess_state.width as i32,
              sess_state.height as i32,
              sess_state.stride as i32,
              wl_shm::Format::Xrgb8888,
              qh,
              (),
            );

            sess_state.alloc = Some(alloc);
            sess_state.pool = Some(pool);
            sess_state.wl_buf = Some(wl_buf);
            sess_state.constraints_ready = true;

            debug!("session[{idx}] ready: buffer allocated");
          }
        }
      }

      Event::Stopped => {
        warn!("session[{idx}] stopped");
      }

      _ => {}
    }
  }
}

// ──────────────────────────────────────────────────────────────────────────────
// ext_image_copy_capture_frame_v1 — handle frame completion
// user data = session index (usize)
// ──────────────────────────────────────────────────────────────────────────────

impl Dispatch<ExtImageCopyCaptureFrameV1, usize> for ExtImageState {
  fn event(
    state: &mut Self,
    frame: &ExtImageCopyCaptureFrameV1,
    event: ext_image_copy_capture_frame_v1::Event,
    idx: &usize,
    _: &Connection,
    _qh: &QueueHandle<Self>,
  ) {
    use ext_image_copy_capture_frame_v1::Event;

    match event {
      Event::Transform { .. } => {
        // Ignore transform for now
      }

      Event::Damage { .. } => {
        // Ignore damage info; we already have full image
      }

      Event::PresentationTime { .. } => {
        // Ignore presentation time
      }

      Event::Ready => {
        if let Some(sess_state) = state.session_states.get_mut(*idx) {
          if let Some(window_id) = sess_state.pending_window_id.take() {
            if let Some(alloc) = &sess_state.alloc {
              let data: Arc<[u8]> = alloc.data().into();
              let _ = state.frame_tx.try_send(ThumbnailFrame {
                window_id,
                width: sess_state.width,
                height: sess_state.height,
                stride: sess_state.stride,
                data,
              });
            }
          }
        }
        frame.destroy();
      }

      Event::Failed { reason: _ } => {
        warn!("frame capture failed for session[{idx}]");
        if let Some(sess_state) = state.session_states.get_mut(*idx) {
          sess_state.pending_window_id = None;
        }
        frame.destroy();
      }

      _ => {}
    }
  }
}

// ──────────────────────────────────────────────────────────────────────────────
// Public entry point
// ──────────────────────────────────────────────────────────────────────────────

pub fn run_ext_image(
    request_rx: Receiver<CaptureRequest>,
    frame_tx: Sender<ThumbnailFrame>,
) -> anyhow::Result<()> {
    use anyhow::Context;

    info!("ext_image: connecting to Wayland display");
    let conn = Connection::connect_to_env().context("wayland connect")?;
    let display = conn.display();
    let mut queue = conn.new_event_queue::<ExtImageState>();
    let qh = queue.handle();

    let mut state = ExtImageState::new(frame_tx);

    // Trigger global enumeration
    let _registry = display.get_registry(&qh, ());
    info!("ext_image: running initial roundtrip");
    queue.roundtrip(&mut state).context("initial roundtrip")?;
    info!("ext_image: roundtrip complete");
    info!("ext_image: found {} outputs, {} capture_sources, shm={}, mgr={}",
        state.outputs.len(),
        state.capture_sources.len(),
        state.shm.is_some(),
        state.copy_capture_manager.is_some()
    );

    if state.shm.is_none() {
        anyhow::bail!("compositor did not advertise wl_shm");
    }
    if state.copy_capture_manager.is_none() {
        anyhow::bail!(
            "compositor did not advertise ext_image_copy_capture_manager_v1; \
is the compositor running?"
        );
    }
    if state.outputs.is_empty() {
        anyhow::bail!("no wl_output found on screencopy connection");
    }

    // Create one session per output
    if state.capture_sources.len() < state.outputs.len() {
        anyhow::bail!(
            "fewer capture sources ({}) than outputs ({}); \
ext_image_capture_source_v1 not properly advertised?",
            state.capture_sources.len(),
            state.outputs.len()
        );
    }

    let mgr = state.copy_capture_manager.as_ref().unwrap();
    for (idx, source) in state.capture_sources.iter().enumerate() {
        let session = mgr.create_session(source, ext_image_copy_capture_manager_v1::Options::empty(), &qh, idx);
        state.sessions.push(session);
        state.session_states.push(SessionState::new());
    }

    debug!("about to do session creation roundtrip...");
    queue.roundtrip(&mut state).context("session creation roundtrip")?;

    info!(
        "ext_image screencopy thread ready ({} output(s), {} session(s))",
        state.outputs.len(),
        state.sessions.len()
    );

    let conn_fd = conn.as_fd();

    loop {
        // ── 1. Accept incoming capture requests ───────────────────────────
        loop {
            match request_rx.try_recv() {
                Ok(req) => state.issue_capture(&req, &qh),
                Err(TryRecvError::Empty) => break,
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
        let has_pending = state.session_states.iter().any(|s| s.pending_window_id.is_some());
        let timeout_ms: i64 = if has_pending { 5 } else { 50 };
        let ts = Timespec { tv_sec: 0, tv_nsec: timeout_ms * 1_000_000 };
        let mut poll_fds = [PollFd::new(&conn_fd, PollFlags::IN)];
        let _ = rustix::event::poll(&mut poll_fds, Some(&ts));

        if let Some(guard) = queue.prepare_read() {
            guard.read().ok();
        }

        queue.dispatch_pending(&mut state).context("dispatch_pending post-read")?;
    }
}

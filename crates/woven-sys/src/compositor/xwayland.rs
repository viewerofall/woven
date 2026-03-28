//! XWayland compatibility layer.
//! XWayland windows appear in Hyprland's client list tagged xwayland=true
//! but their PIDs need resolving via _NET_WM_PID on the X11 side.
//! After PID resolution they flow through the normal window pipeline.

use anyhow::Result;
use tracing::debug;

/// Given an XWayland window's X11 window ID, resolve its PID via _NET_WM_PID.
/// Falls back to None gracefully if xcb or the property isn't available.
/// No warning when DISPLAY is unset (pure Wayland sessions) or resolution fails.
pub fn resolve_pid(xwin_id: u32) -> Option<u32> {
    if std::env::var("DISPLAY").is_err() {
        return None;
    }
    match try_resolve_pid(xwin_id) {
        Ok(pid) => Some(pid),
        Err(e) => {
            debug!("XWayland PID resolution failed for {}: {}", xwin_id, e);
            None
        }
    }
}

fn try_resolve_pid(xwin_id: u32) -> Result<u32> {
    use xcb::Connection;

    let (conn, _) = Connection::connect(None)
    .map_err(|e| anyhow::anyhow!("xcb connect failed: {}", e))?;

    // intern the _NET_WM_PID atom
    let atom_cookie = conn.send_request(&xcb::x::InternAtom {
        only_if_exists: true,
        name: b"_NET_WM_PID",
    });
    let atom_reply = conn.wait_for_reply(atom_cookie)
    .map_err(|e| anyhow::anyhow!("InternAtom failed: {}", e))?;

    let pid_atom = atom_reply.atom();
    if pid_atom == xcb::x::ATOM_NONE {
        anyhow::bail!("_NET_WM_PID atom not available on this display");
    }

    // build the window xid from the raw u32
    let window: xcb::x::Window = unsafe { std::mem::transmute(xwin_id) };

    // request the property
    let prop_cookie = conn.send_request(&xcb::x::GetProperty {
        delete:      false,
        window,
        property:    pid_atom,
        r#type:      xcb::x::ATOM_CARDINAL,
        long_offset: 0,
        long_length: 1,
    });
    let prop_reply = conn.wait_for_reply(prop_cookie)
    .map_err(|e| anyhow::anyhow!("GetProperty failed: {}", e))?;

    let data: &[u32] = prop_reply.value();
    data.first()
    .copied()
    .ok_or_else(|| anyhow::anyhow!("_NET_WM_PID property was empty"))
}

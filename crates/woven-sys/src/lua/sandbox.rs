//! Strips the Lua stdlib down to only what is safe.
//! Called once before any user or runtime Lua is loaded.
//! After this runs, Lua cannot touch the filesystem, OS, or network
//! except through the woven.* API that Rust explicitly provides.

use anyhow::Result;
use mlua::prelude::*;
use tracing::debug;

/// Globals stripped entirely before any Lua code runs.
const STRIP: &[&str] = &[
    "io",
"os",
"package",
"debug",
"loadfile",       // opens files directly — blocked
"dofile",         // opens files directly — blocked
"collectgarbage",
// `load` is intentionally NOT in this list.
// boot.lua uses it to compile the user config string into a chunk
// and execute it inside pcall. It cannot open files on its own.
// loadfile/dofile are the dangerous ones — those stay nil'd.
//
// `require` is also NOT stripped here — registry.bind() replaces
// it with a sandboxed version restricted to runtime/ and config dir.
];

pub fn apply(lua: &Lua) -> Result<()> {
    let globals = lua.globals();

    for name in STRIP {
        globals.set(*name, LuaNil)?;
        debug!("sandbox: stripped '{}'", name);
    }

    // nil require now — registry.bind() installs the safe version after
    globals.set("require", LuaNil)?;

    debug!("sandbox: environment locked down");
    Ok(())
}

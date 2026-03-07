local M = {}
function M.focus(id)      woven.window.focus(id) end
function M.close(id)      woven.window.close(id) end
function M.fullscreen(id) woven.window.fullscreen(id) end
function M.move(id, ws)   woven.window.move(id, ws) end
return M

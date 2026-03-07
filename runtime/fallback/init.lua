local guide    = require("fallback.guide")
local defaults = require("fallback.defaults")
local M = {}
function M.start()
    woven.log.info("fallback: entering setup guide")
    guide.run(defaults)
end
return M

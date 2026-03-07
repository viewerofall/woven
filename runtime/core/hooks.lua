local M = { _listeners = {} }
function M.init()
    woven.on = function(event, fn)
        if not M._listeners[event] then M._listeners[event] = {} end
        table.insert(M._listeners[event], fn)
    end
    woven.bind = function(name, fn)
        M._listeners["bind:" .. name] = { fn }
    end
end
function M.fire(event, data)
    local ls = M._listeners[event]
    if not ls then return end
    for _, fn in ipairs(ls) do
        local ok, err = pcall(fn, data)
        if not ok then
            woven.log.error("hook error [" .. event .. "]: " .. tostring(err))
        end
    end
end
function M.invoke_bind(name, ...) M.fire("bind:" .. name, ...) end
return M

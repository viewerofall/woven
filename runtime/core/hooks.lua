-- core/hooks.lua
-- Event system. Installs woven.on / woven.once / woven.off / woven.emit / woven.bind.

local M = { _listeners = {} }

function M.init()
    -- Register a persistent listener.
    woven.on = function(event, fn)
        if not M._listeners[event] then M._listeners[event] = {} end
        table.insert(M._listeners[event], { fn = fn, once = false })
    end

    -- Register a one-shot listener — fires once then removes itself.
    woven.once = function(event, fn)
        if not M._listeners[event] then M._listeners[event] = {} end
        table.insert(M._listeners[event], { fn = fn, once = true })
    end

    -- Unregister a specific listener function.
    woven.off = function(event, fn)
        local ls = M._listeners[event]
        if not ls then return end
        for i = #ls, 1, -1 do
            if ls[i].fn == fn then table.remove(ls, i) end
        end
    end

    -- Emit an event from Lua (plugin-to-plugin or user scripts).
    -- Fires all listeners registered for that event name.
    woven.emit = function(event, data)
        M.fire(event, data)
    end

    -- Register a named window action binding (single active handler per name).
    woven.bind = function(name, fn)
        M._listeners["bind:" .. name] = { { fn = fn, once = false } }
    end
end

function M.fire(event, data)
    local ls = M._listeners[event]
    if not ls then return end
    local dead = {}
    for i, entry in ipairs(ls) do
        local ok, err = pcall(entry.fn, data)
        if not ok then
            woven.log.error("hook error [" .. event .. "]: " .. tostring(err))
        end
        if entry.once then dead[#dead + 1] = i end
    end
    -- remove once-entries back-to-front so indices stay valid
    for i = #dead, 1, -1 do
        table.remove(ls, dead[i])
    end
end

function M.invoke_bind(name, ...) M.fire("bind:" .. name, ...) end

return M

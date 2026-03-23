-- skip.lua — nexus-tui aniskip integration
-- Reads skip-op_start/end and skip-ed_start/end from script-opts
-- and automatically seeks past intros/outros.
-- Place in ~/.config/mpv/scripts/ (installer does this automatically)

local opts = {
    skip_op_start = -1,
    skip_op_end   = -1,
    skip_ed_start = -1,
    skip_ed_end   = -1,
}

require("mp.options").read_options(opts, "skip")

local intro_skipped = false
local outro_skipped = false

mp.observe_property("time-pos", "number", function(_, pos)
    if pos == nil then return end

    if not intro_skipped and opts.skip_op_start >= 0 and opts.skip_op_end > 0 then
        if pos >= opts.skip_op_start - 0.5 and pos < opts.skip_op_end - 1.0 then
            intro_skipped = true
            mp.commandv("seek", tostring(opts.skip_op_end), "absolute")
            mp.osd_message("⏭ Skipped intro", 2)
        end
    end

    if not outro_skipped and opts.skip_ed_start >= 0 and opts.skip_ed_end > 0 then
        if pos >= opts.skip_ed_start - 0.5 and pos < opts.skip_ed_end - 1.0 then
            outro_skipped = true
            mp.commandv("seek", tostring(opts.skip_ed_end), "absolute")
            mp.osd_message("⏭ Skipped outro", 2)
        end
    end
end)

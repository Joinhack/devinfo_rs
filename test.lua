local devinfo = require "devinfo"

local dev = devinfo.get()
print(dev.host_name)
print(dev.sys_name)
for k, v in ipairs(dev.devices) do
    print(v.name, v.ipv4, v.mac, v.ipv6)
end

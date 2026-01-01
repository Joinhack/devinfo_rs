#[cfg(not(windows))]
use libc;
use mlua::prelude::*;
#[cfg(not(windows))]
use std::ffi::CString;
use std::io;
#[cfg(not(windows))]
use std::mem::MaybeUninit;
#[cfg(not(windows))]
use std::ptr;
use std::{collections::HashMap, ffi::CStr};

#[cfg(windows)]
use windows::Win32::NetworkManagement::IpHelper::{GetAdaptersInfo, MIB_IF_TYPE_ETHERNET};

#[cfg(windows)]
use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_SUCCESS, NO_ERROR};
#[cfg(windows)]
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RegCloseKey, RegOpenKeyExA, RegQueryValueExA,
};
#[cfg(windows)]
use windows::Win32::System::SystemInformation::{
    ComputerNamePhysicalDnsHostname, GetComputerNameExA,
};
#[cfg(windows)]
use windows::core::{PCSTR, PSTR};

#[cfg(target_os = "macos")]
const AF_LINK: i32 = libc::AF_LINK;
#[cfg(target_os = "linux")]
const AF_LINK: i32 = libc::AF_PACKET;

#[mlua::lua_module(name = "devinfo")]
fn devinfo_entry(lua: &Lua) -> LuaResult<LuaTable> {
    let table = lua.create_table()?;

    table.set(
        "get",
        lua.create_function(move |lua: &Lua, ()| {
            let table = lua.create_table()?;
            let mut dev_info = DevInfo {};
            let host_name = dev_info.host_name()?;
            let addrs = dev_info.get_addr()?;
            let sys_name = dev_info.get_system_name()?;
            let info_table = lua.create_table()?;
            for (k, v) in addrs {
                let dev_table = lua.create_table()?;
                v.ipv4.map(|ipv4| dev_table.set("ipv4", ipv4));
                v.ipv6.map(|ipv6| dev_table.set("ipv6", ipv6));
                v.mac_addr.map(|mac| dev_table.set("mac", mac));
                dev_table.set("name", k)?;
                info_table.push(dev_table)?;
            }
            table.set("devices", info_table)?;
            table.set("sys_name", sys_name)?;
            table.set("host_name", host_name)?;
            Ok(table)
        })?,
    )?;
    Ok(table)
}

struct DevInfo {}

#[derive(Default, Clone, Debug)]
struct DevAddressInfo {
    ipv4: Option<String>,
    ipv6: Option<String>,
    mac_addr: Option<String>,
}

#[cfg(windows)]
struct HKey(HKEY);

#[cfg(windows)]
impl Drop for HKey {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

impl DevInfo {
    #[cfg(windows)]
    fn get_system_name(&mut self) -> io::Result<String> {
        unsafe {
            let mut hkey = HKEY::default();
            if RegOpenKeyExA(
                HKEY_LOCAL_MACHINE,
                PCSTR(b"SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\0".as_ptr()),
                None,
                KEY_READ,
                &mut hkey,
            ) != ERROR_SUCCESS
            {
                return Err(io::Error::last_os_error());
            }
            let hkey = HKey(hkey);
            let mut data_len: u32 = 0;
            let name_ptr = b"ProductName\0".as_ptr();
            if RegQueryValueExA(
                hkey.0,
                PCSTR(name_ptr),
                None,
                None,
                None,
                Some(&mut data_len),
            ) != ERROR_SUCCESS
            {
                return Err(io::Error::last_os_error());
            }
            let mut buf = Vec::<u8>::with_capacity(data_len as _);
            buf.set_len(buf.capacity());
            if RegQueryValueExA(
                hkey.0,
                PCSTR(name_ptr),
                None,
                None,
                Some(buf.as_mut_ptr()),
                Some(&mut data_len),
            ) != ERROR_SUCCESS
            {
                return Err(io::Error::last_os_error());
            }
            String::from_utf8(buf).map_err(|e| io::Error::other(e))
        }
    }

    #[cfg(windows)]
    fn get_addr(&mut self) -> io::Result<HashMap<String, DevAddressInfo>> {
        let mut map = HashMap::<String, DevAddressInfo>::new();
        unsafe {
            let mut adapter_len: u32 = 0;
            let rs = GetAdaptersInfo(Some(std::ptr::null_mut()), &mut adapter_len);
            if rs != ERROR_BUFFER_OVERFLOW.0 {
                return Err(io::Error::other("GetAdaptersInfo buffer overflow failed."));
            }
            let mut buf: Vec<u8> = Vec::with_capacity(adapter_len as _);
            buf.set_len(buf.capacity());
            let mut adapter_ptr = buf.as_mut_ptr() as _;
            let ret = GetAdaptersInfo(Some(adapter_ptr), &mut adapter_len);
            if ret != NO_ERROR.0 {
                return Err(io::Error::other("GetAdaptersInfo error."));
            }

            while !adapter_ptr.is_null() {
                if (*adapter_ptr).Type == MIB_IF_TYPE_ETHERNET {
                    let mut dev_info = DevAddressInfo::default();
                    let raw_ip_str =
                        CStr::from_ptr((*adapter_ptr).IpAddressList.IpAddress.String.as_ptr() as _);
                    let name = CStr::from_ptr((*adapter_ptr).AdapterName.as_ptr());
                    let name = name.to_string_lossy().to_string();
                    let ip_addr = raw_ip_str.to_string_lossy().to_string();
                    dev_info.ipv4 = Some(ip_addr);
                    let mac: &[u8] = &(*adapter_ptr).Address;
                    let mac = Self::to_mac(mac);
                    dev_info.mac_addr = Some(mac);
                    map.insert(name, dev_info);
                }
                adapter_ptr = (*adapter_ptr).Next;
            }
        }
        Ok(map)
    }

    #[cfg(target_os = "macos")]
    fn get_system_name(&mut self) -> io::Result<String> {
        let ret = Self::sysctl_string("kern.osproductversion");
        let ret = ret.ok_or(io::Error::other("get version error"))?;
        Ok(format!("Macos {ret}"))
    }

    #[cfg(target_os = "macos")]
    fn sysctl_string(name: &str) -> Option<String> {
        unsafe {
            let cname = CString::new(name).ok()?;

            let mut size: libc::size_t = 0;
            if libc::sysctlbyname(
                cname.as_ptr(),
                std::ptr::null_mut(),
                &mut size,
                std::ptr::null_mut(),
                0,
            ) != 0
            {
                return None;
            }

            let mut buf = vec![0u8; size as _];
            if libc::sysctlbyname(
                cname.as_ptr(),
                buf.as_mut_ptr() as *mut libc::c_void,
                &mut size,
                std::ptr::null_mut(),
                0,
            ) != 0
            {
                return None;
            }

            if let Some(pos) = buf.iter().position(|&c| c == 0) {
                buf.truncate(pos);
            }

            String::from_utf8(buf).ok()
        }
    }

    #[inline(always)]
    fn to_mac<T: std::fmt::UpperHex>(mac: &[T]) -> String {
        return format!(
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
        );
    }

    #[cfg(target_os = "linux")]
    fn get_system_name() -> io::Result<String> {
        let s = std::fs::read_to_string("/etc/os-release")?;
        let mut pos = 0;
        let mut name = "";
        let mut version = "";
        for line in s.split("\n") {
            if let Some((k, v)) = line.split_once("\n") {
                if "PRETTY_NAME" == k {
                    return Ok(v.to_string());
                }
                if "VERSION" == k {
                    version = v;
                }
                if "NAME" == k {
                    name = v;
                }
            }
        }
        return Ok(format!("{name} {version}"));
    }

    #[cfg(target_os = "linux")]
    fn get_system_name(&mut self) -> io::Result<String> {}

    #[cfg(windows)]
    fn host_name(&mut self) -> io::Result<String> {
        let mut buf = [0u8; 1024];
        let mut size = buf.len() as u32;
        let str = unsafe {
            GetComputerNameExA(
                ComputerNamePhysicalDnsHostname,
                Some(PSTR(buf.as_mut_ptr() as _)),
                &mut size,
            )?;
            let sli = std::slice::from_raw_parts(buf.as_ptr(), size as _);
            String::from_utf8_lossy(sli)
        };
        Ok(str.to_string())
    }

    #[cfg(not(windows))]
    fn host_name(&mut self) -> io::Result<String> {
        let mut buf = [0u8; 1024];
        let hostname = unsafe {
            if libc::gethostname(buf.as_mut_ptr() as _, buf.len()) != 0 {
                return Err(io::Error::last_os_error());
            }
            CStr::from_ptr(buf.as_ptr() as _)
        };
        let hostname = hostname.to_string_lossy().to_string();
        Ok(hostname)
    }

    #[cfg(not(windows))]
    fn get_addr(&mut self) -> io::Result<HashMap<String, DevAddressInfo>> {
        let mut ifaddr: MaybeUninit<*mut libc::ifaddrs> = MaybeUninit::uninit();
        let mut map = HashMap::<String, DevAddressInfo>::new();
        unsafe {
            let ifaddr_ptr = ifaddr.as_mut_ptr();
            let mut name = String::new();
            let mut info = DevAddressInfo::default();
            if libc::getifaddrs(ifaddr_ptr) != 0 {
                return Err(io::Error::other("getifaddrs failed."));
            }
            let mut addr_ptr = *ifaddr_ptr;
            let process = |addr_ptr: *mut libc::ifaddrs, info: &mut DevAddressInfo| {
                let sa_family: i32 = (*(*addr_ptr).ifa_addr).sa_family as _;
                if sa_family == AF_LINK {
                    #[cfg(target_os = "macos")]
                    let addr = {
                        let addr = &*((*addr_ptr).ifa_addr as *mut libc::sockaddr_dl);
                        let sdl_ptr = addr.sdl_data.as_ptr();
                        std::slice::from_raw_parts(sdl_ptr.add(addr.sdl_nlen as _), 6)
                    };

                    #[cfg(target_os = "linux")]
                    let addr = {
                        let addr = &*((*addr_ptr).ifa_addr as *mut libc::sockaddr_ll);
                        let sal_ptr = addr.sll_addr.as_ptr();
                        std::slice::from_raw_parts(sal_ptr, 6)
                    };

                    info.mac_addr = Some(Self::to_mac(addr));
                }
                if sa_family == libc::AF_INET {
                    let addr_in = (*addr_ptr).ifa_addr as *const libc::sockaddr_in;
                    let ipv4 = std::net::Ipv4Addr::from_bits((*addr_in).sin_addr.s_addr.to_be());
                    info.ipv4 = Some(ipv4.to_string());
                }
                if sa_family == libc::AF_INET6 {
                    let addr_in = (*addr_ptr).ifa_addr as *const libc::sockaddr_in6;
                    let ipv6 = std::net::Ipv6Addr::from_octets((*addr_in).sin6_addr.s6_addr);
                    info.ipv6 = Some(ipv6.to_string());
                }
            };
            while addr_ptr != ptr::null_mut() {
                let flags: i32 = (*addr_ptr).ifa_flags as _;
                if flags & libc::IFF_UP == 0 {
                    addr_ptr = (*addr_ptr).ifa_next;
                    continue;
                }
                if name.len() > 0
                    && libc::strncmp((*addr_ptr).ifa_name, name.as_ptr() as _, name.len()) == 0
                {
                    process(addr_ptr, &mut info);
                } else {
                    if name.len() != 0 {
                        map.insert(name.clone(), info);
                        info = Default::default();
                    }
                    name = CStr::from_ptr((*addr_ptr).ifa_name)
                        .to_string_lossy()
                        .to_string();
                    process(addr_ptr, &mut info);
                }
                addr_ptr = (*addr_ptr).ifa_next;
            }
            libc::freeifaddrs(*ifaddr_ptr);
            Ok(map)
        }
    }
}

use core::ptr::null_mut;

const BOOTROM_MAGIC_OFFSET: u16 = 0x10;
const BOOTROM_VERSION_OFFSET: u16 = 0x12;

const BOOTROM_FUNC_TABLE_OFFSET: u16 = 0x14;

pub struct ROM;

enum BootromVersion {
    RP2040,
    RP235x,
}

impl ROM {
    pub unsafe fn reset_usb_boot(
        activity_gpio: Option<u8>,
        disable_usb: bool,
        disable_picoboot: bool,
    ) -> ! {
        match unsafe { Self::check_bootrom_magic() } {
            Some(BootromVersion::RP2040) => unsafe {
                Self::reset_usb_boot_rp2040(activity_gpio, disable_usb, disable_picoboot)
            },
            Some(BootromVersion::RP235x) => unsafe {
                Self::reset_usb_boot_rp235x(activity_gpio, disable_usb, disable_picoboot)
            },
            None => panic!("unknown bootrom version"),
        }
    }

    unsafe fn reset_usb_boot_rp2040(
        activity_gpio: Option<u8>,
        disable_usb: bool,
        disable_picoboot: bool,
    ) -> ! {
        let func_ptr = unsafe { Self::rp2040_rom_func_lookup(Self::rom_table_code(b"UB")) };

        type RomResetUsbBootFn =
            unsafe extern "C" fn(activity_gpio: u32, disable_interfaces: u32) -> !;

        let func = unsafe { core::mem::transmute::<*const (), RomResetUsbBootFn>(func_ptr) };

        let activity_gpio = if let Some(gpio) = activity_gpio {
            if gpio & 0x80 != 0 {
                panic!("active-low gpio not supported on rp2040")
            }

            if gpio >= 32 {
                panic!("out-of-range gpio {gpio}");
            }

            1 << gpio
        } else {
            0
        };

        unsafe {
            func(
                activity_gpio,
                if disable_usb { 1 << 0 } else { 0 } | if disable_picoboot { 1 << 1 } else { 0 },
            )
        }
    }

    unsafe fn reset_usb_boot_rp235x(
        activity_gpio: Option<u8>,
        disable_usb: bool,
        disable_picoboot: bool,
    ) -> ! {
        let func_ptr = unsafe { Self::rp235x_rom_func_lookup(Self::rom_table_code(b"RB")) };

        type RomResetUsbBootFn =
            unsafe extern "C" fn(flags: u32, delay_ms: u32, p0: u32, p1: u32) -> core::ffi::c_int;

        const REBOOT_TYPE_BOOTSEL: u32 = 0x0002;
        const NO_RETURN_ON_SUCCESS: u32 = 0x0100;

        let func = unsafe { core::mem::transmute::<*const (), RomResetUsbBootFn>(func_ptr) };

        let (activity_gpio, gpio_enabled, gpio_active_low) = if let Some(gpio) = activity_gpio {
            let active_low = gpio & 0x80 != 0;
            let gpio = gpio & 0x7F;

            if gpio >= 32 {
                panic!("out-of-range gpio {gpio}");
            }

            (1 << gpio, true, active_low)
        } else {
            (0, false, false)
        };

        let ret = unsafe {
            func(
                REBOOT_TYPE_BOOTSEL | NO_RETURN_ON_SUCCESS,
                1,
                if disable_usb { 1 << 0 } else { 0 }
                    | if disable_picoboot { 1 << 1 } else { 0 }
                    | if gpio_active_low { 1 << 4 } else { 0 }
                    | if gpio_enabled { 1 << 5 } else { 0 },
                activity_gpio,
            )
        };

        unreachable!("reboot failed, error code {ret}");
    }

    unsafe fn rom_read<T>(rom_address: u16) -> T {
        unsafe { core::ptr::with_exposed_provenance::<T>(rom_address.into()).read_volatile() }
    }

    unsafe fn check_bootrom_magic() -> Option<BootromVersion> {
        let magic = unsafe { Self::rom_read::<u16>(BOOTROM_MAGIC_OFFSET) };

        if magic != u16::from_ne_bytes(*b"Mu") {
            return None;
        }

        let version = unsafe { Self::rom_read::<u8>(BOOTROM_VERSION_OFFSET) };

        match version {
            0x01 => Some(BootromVersion::RP2040),
            0x02 => Some(BootromVersion::RP235x),
            _ => None,
        }
    }

    const fn rom_table_code(ident: &[u8; 2]) -> u32 {
        u16::from_ne_bytes(*ident) as _
    }

    unsafe fn rp2040_rom_func_lookup(code: u32) -> *const () {
        let func_table_addr = unsafe {
            core::ptr::with_exposed_provenance::<u16>(
                Self::rom_read::<u16>(BOOTROM_FUNC_TABLE_OFFSET).into(),
            )
        };

        type RomTableLookupFn =
            unsafe extern "C" fn(table: *const u16, code: u32) -> *const core::ffi::c_void;

        const BOOTROM_TABLE_LOOKUP_OFFSET: u16 = 0x18;

        let lookup_addr = unsafe { Self::rom_read::<u16>(BOOTROM_TABLE_LOOKUP_OFFSET) };
        let lookup_addr = core::ptr::with_exposed_provenance::<()>(lookup_addr.into());
        let lookup_addr =
            unsafe { core::mem::transmute::<*const (), RomTableLookupFn>(lookup_addr) };

        unsafe { lookup_addr(func_table_addr, code) as *const () }
    }

    unsafe fn rp235x_rom_func_lookup(code: u32) -> *const () {
        type RomTableLookupFn =
            unsafe extern "C" fn(code: u32, mask: u32) -> *const core::ffi::c_void;

        const BOOTROM_WELL_KNOWN_PTR_SIZE: u16 = 2;
        const BOOTROM_TABLE_LOOKUP_OFFSET: u16 =
            BOOTROM_FUNC_TABLE_OFFSET + BOOTROM_WELL_KNOWN_PTR_SIZE;

        const RT_FLAG_FUNC_ARM_SEC: u32 = 0x0004;
        const RT_FLAG_FUNC_ARM_NONSEC: u32 = 0x0010;

        let lookup_addr = unsafe { Self::rom_read::<u16>(BOOTROM_TABLE_LOOKUP_OFFSET) };
        let lookup_addr = core::ptr::with_exposed_provenance::<()>(lookup_addr.into());
        let lookup_addr =
            unsafe { core::mem::transmute::<*const (), RomTableLookupFn>(lookup_addr) };

        if cortex_m::asm::tt(null_mut()) & (1 << 22) != 0 {
            unsafe { lookup_addr(code, RT_FLAG_FUNC_ARM_SEC) as _ }
        } else {
            unsafe { lookup_addr(code, RT_FLAG_FUNC_ARM_NONSEC) as _ }
        }
    }
}

#![no_std]
#![no_main]

use bridge::Bridge;
use panic_halt as _;

use rp235x_hal::binary_info::{
    EntryAddr, rp_cargo_bin_name, rp_cargo_homepage_url, rp_cargo_version,
    rp_program_build_attribute, rp_program_description,
};
use rp235x_hal::block::ImageDef;
use rp235x_hal::clocks::init_clocks_and_plls;
use rp235x_hal::pac::Peripherals;
use rp235x_hal::usb::UsbBus;
use rp235x_hal::{Timer, Watchdog};
use usb_device::LangID;
use usb_device::bus::UsbBusAllocator;
use usb_device::device::{StringDescriptors, UsbDeviceBuilder, UsbVidPid};

mod bridge;

#[unsafe(link_section = ".start_block")]
#[used]
pub static IMAGE_DEF: ImageDef = ImageDef::secure_exe();

const XTAL_FREQ_HZ: u32 = 12_000_000;

#[rp235x_hal::entry]
fn main() -> ! {
    let mut pac = Peripherals::take().unwrap();

    let mut watchdog = Watchdog::new(pac.WATCHDOG);

    let clocks = init_clocks_and_plls(
        XTAL_FREQ_HZ,
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
    .unwrap();

    //let timer = Timer::new_timer0(pac.TIMER0, &mut pac.RESETS, &clocks);

    let usb_bus = UsbBusAllocator::new(UsbBus::new(
        pac.USB,
        pac.USB_DPRAM,
        clocks.usb_clock,
        true,
        &mut pac.RESETS,
    ));

    let mut driver = Bridge::new(&usb_bus);

    let mut usb_dev = UsbDeviceBuilder::new(&usb_bus, UsbVidPid(0x0ED2, 0x64DD))
        .strings(&[StringDescriptors::new(LangID::EN_GB)
            .manufacturer("Kyoto Micro Computer Co., Ltd")
            .product("Partner-N64 USB interface")
            .serial_number("PARTNER-N64")])
        .unwrap()
        .max_packet_size_0(64)
        .unwrap()
        .device_class(0xFF)
        .build();

    loop {
        if usb_dev.poll(&mut [&mut driver]) {
            match driver.read() {
                Err(_) => {
                    // do nothing
                }
                Ok(_) => {
                    // do nothing
                }
            }
        }
    }
}

#[unsafe(link_section = ".bi_entries")]
#[used]
pub static PICOTOOL_ENTRIES: [EntryAddr; 5] = [
    rp_cargo_bin_name!(),
    rp_cargo_version!(),
    rp_program_description!(c"Partner-N64 USB interface"),
    rp_cargo_homepage_url!(),
    rp_program_build_attribute!(),
];

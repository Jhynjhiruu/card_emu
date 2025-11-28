#![no_std]
#![no_main]

use bridge::Bridge;
use panic_halt as _;

use pio::{pio_asm, pio_file};
use rp235x_hal::binary_info::{
    EntryAddr, rp_cargo_bin_name, rp_cargo_homepage_url, rp_cargo_version,
    rp_program_build_attribute, rp_program_description,
};
use rp235x_hal::block::ImageDef;
use rp235x_hal::clocks::init_clocks_and_plls;
use rp235x_hal::dma::{Byte, HalfWord};
use rp235x_hal::gpio::{DynPinId, FunctionPio0, Pin, PinGroup, Pins, PullUp};
use rp235x_hal::pac::Peripherals;
use rp235x_hal::pio::{PIOBuilder, PIOExt, PinDir, ShiftDirection};
use rp235x_hal::usb::UsbBus;
use rp235x_hal::{Sio, Timer, Watchdog};
use usb_device::LangID;
use usb_device::bus::UsbBusAllocator;
use usb_device::device::{StringDescriptors, UsbDeviceBuilder, UsbVidPid};

mod bridge;
mod rom;

#[unsafe(link_section = ".start_block")]
#[used]
pub static IMAGE_DEF: ImageDef = ImageDef::secure_exe();

const XTAL_FREQ_HZ: u32 = 12_000_000;

const ADDR_PIN_START: u8 = 0;
const ADDR_PIN_LEN: u8 = 8;

const DATA_PIN_START: u8 = ADDR_PIN_START + ADDR_PIN_LEN;
const DATA_PIN_LEN: u8 = 8;

const CTRL_PIN_START: u8 = DATA_PIN_START + DATA_PIN_LEN;
const DIR_PIN: u8 = CTRL_PIN_START;
const CLK_PIN: u8 = CTRL_PIN_START + 1;

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

    let sio = Sio::new(pac.SIO);

    let pins = Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    let addr: [Pin<DynPinId, FunctionPio0, PullUp>; 8] = [
        pins.gpio0.into_function().into_pull_type().into_dyn_pin(),
        pins.gpio1.into_function().into_pull_type().into_dyn_pin(),
        pins.gpio2.into_function().into_pull_type().into_dyn_pin(),
        pins.gpio3.into_function().into_pull_type().into_dyn_pin(),
        pins.gpio4.into_function().into_pull_type().into_dyn_pin(),
        pins.gpio5.into_function().into_pull_type().into_dyn_pin(),
        pins.gpio6.into_function().into_pull_type().into_dyn_pin(),
        pins.gpio7.into_function().into_pull_type().into_dyn_pin(),
    ];

    let data: [Pin<DynPinId, FunctionPio0, PullUp>; 8] = [
        pins.gpio8.into_function().into_pull_type().into_dyn_pin(),
        pins.gpio9.into_function().into_pull_type().into_dyn_pin(),
        pins.gpio10.into_function().into_pull_type().into_dyn_pin(),
        pins.gpio11.into_function().into_pull_type().into_dyn_pin(),
        pins.gpio12.into_function().into_pull_type().into_dyn_pin(),
        pins.gpio13.into_function().into_pull_type().into_dyn_pin(),
        pins.gpio14.into_function().into_pull_type().into_dyn_pin(),
        pins.gpio15.into_function().into_pull_type().into_dyn_pin(),
    ];

    let ctrl: [Pin<DynPinId, FunctionPio0, PullUp>; 2] = [
        pins.gpio16.into_function().into_pull_type().into_dyn_pin(),
        pins.gpio17.into_function().into_pull_type().into_dyn_pin(),
    ];

    let (mut pio0, sm0, sm1, _, _) = pac.PIO0.split(&mut pac.RESETS);

    // side-set 0 is DIR
    // side-set 1 is CLK
    let read = pio_asm!(
        "
        .side_set 2 opt
        ; .in 8 left auto 8
        ; .out 8 left auto 8
        ; .clock_div 1

        .wrap_target
            out pins,    8
            mov pindirs, null   side 0b00
            in  pins,    8
        .wrap
        "
    );
    let write = pio_asm!(
        "
        .side_set 2 opt
        ; .out 16 left auto 16
        ; .clock_div 1

        .wrap_target
            out pins,    16     side 0b01
            mov pindirs, ~null  side 0b11
            nop                 side 0b01
        .wrap
        "
    );
    let read_installed = pio0.install(&read.program).unwrap();
    let write_installed = pio0.install(&write.program).unwrap();

    let (mut read_sm, read_rx, read_tx) = PIOBuilder::from_installed_program(read_installed)
        .out_pins(addr[0].id().num, addr.len() as _)
        .out_shift_direction(ShiftDirection::Right)
        .in_pin_base(data[0].id().num)
        .in_count(data.len() as _)
        .in_shift_direction(ShiftDirection::Right)
        .side_set_pin_base(ctrl[0].id().num)
        .autopull(true)
        .pull_threshold(8)
        .autopush(true)
        .push_threshold(8)
        .clock_divisor_fixed_point(1, 0)
        .build(sm0);

    let (write_sm, _, write_tx) = PIOBuilder::from_installed_program(write_installed)
        .out_pins(addr[0].id().num, addr.len() as _)
        .out_shift_direction(ShiftDirection::Right)
        .side_set_pin_base(ctrl[0].id().num)
        .autopull(true)
        .pull_threshold(16)
        .clock_divisor_fixed_point(1, 0)
        .build(sm1);

    read_sm.set_pindirs([(DIR_PIN, PinDir::Output), (CLK_PIN, PinDir::Output)]);
    for i in ADDR_PIN_START..(ADDR_PIN_START + ADDR_PIN_LEN) {
        read_sm.set_pindirs([(i, PinDir::Output)]);
    }

    let read_sm = read_sm.start();
    let write_sm = write_sm.start();

    let usb_bus = UsbBusAllocator::new(UsbBus::new(
        pac.USB,
        pac.USB_DPRAM,
        clocks.usb_clock,
        true,
        &mut pac.RESETS,
    ));

    let mut driver = Bridge::new(
        &usb_bus,
        (
            read_sm,
            read_rx.transfer_size(Byte),
            read_tx.transfer_size(Byte),
        ),
        (write_sm, write_tx.transfer_size(HalfWord)),
    );

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

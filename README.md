# card_emu

A firmware for the Raspberry Pi Pico 2 that provides a USB interface to the Partner-N64 debugging cartridge, effectively replacing the ISA or PCI cards.

## Why Pico 2?

The debugging cartridge uses 5-volt logic levels.

## License

Licensed under either of

 * Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
 * MIT license
   ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

Based on [USBD-Blaster](https://github.com/sameer/usbd-blaster.git) by [Sameer Puri](https://github.com/sameer), the `rp-pico` `pico_usb_serial.rs` example from [rp-hal-boards](https://github.com/rp-rs/rp-hal-boards.git) by [rp-rs](https://github.com/rp-rs), and with `memory.x` taken from and `build.rs` modified from the `rp235x` example from [embassy](https://github.com/embassy-rs/embassy.git) by [embassy-rs](https://github.com/embassy-rs), all of which are dual-licensed under either the MIT license or the Apache License, Version 2.0.

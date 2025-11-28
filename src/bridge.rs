use rp235x_hal::dma::{Byte, HalfWord};
use rp235x_hal::pio::{PIO0SM0, Running, Rx, StateMachine, Tx, ValidStateMachine};
use usb_device::bus::{InterfaceNumber, UsbBus, UsbBusAllocator};
use usb_device::class::{ControlIn, ControlOut, UsbClass};
use usb_device::control::RequestType;
use usb_device::endpoint::{EndpointAddress, EndpointIn, EndpointOut, EndpointType};
use usb_device::{Result, UsbDirection, UsbError};

use crate::rom::ROM;

const BRIDGE_WRITE_SIZE: usize = 64;
const BRIDGE_READ_SIZE: usize = 32;

pub struct Bridge<'a, B: UsbBus, ReadSM, WriteSM>
where
    ReadSM: ValidStateMachine,
    WriteSM: ValidStateMachine,
{
    iface: InterfaceNumber,
    read_ep: EndpointOut<'a, B>,
    write_ep: EndpointIn<'a, B>,

    read_sm: StateMachine<ReadSM, Running>,
    write_sm: StateMachine<WriteSM, Running>,
    read_rx: Rx<ReadSM, Byte>,
    read_tx: Tx<ReadSM, Byte>,
    write_tx: Tx<WriteSM, HalfWord>,

    send_buffer: [u8; BRIDGE_WRITE_SIZE],
    send_len: usize,
    recv_buffer: [u8; BRIDGE_READ_SIZE],
    recv_len: usize,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy)]
enum ControlCommand {
    Write = 0x00,
    Read = 0x01,

    RebootToUSB = 0xFF,
}

impl TryFrom<u8> for ControlCommand {
    type Error = u8;

    fn try_from(value: u8) -> core::result::Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::Write),
            0x01 => Ok(Self::Read),

            0xFF => Ok(Self::RebootToUSB),

            e => Err(e),
        }
    }
}

impl<'a, B: UsbBus, ReadSM, WriteSM> UsbClass<B> for Bridge<'a, B, ReadSM, WriteSM>
where
    ReadSM: ValidStateMachine,
    WriteSM: ValidStateMachine,
{
    fn get_configuration_descriptors(
        &self,
        writer: &mut usb_device::descriptor::DescriptorWriter,
    ) -> Result<()> {
        writer.interface(self.iface, 0xFF, 0xFF, 0xFF)?;
        writer.endpoint(&self.write_ep)?;
        writer.endpoint(&self.read_ep)?;
        Ok(())
    }

    fn reset(&mut self) {
        self.send_len = 0;
        self.recv_len = 0;
    }

    fn control_in(&mut self, xfer: ControlIn<B>) {
        let req = xfer.request();

        if req.request_type == RequestType::Vendor {
            let cmd = ControlCommand::try_from(req.request);

            match cmd {
                Ok(ControlCommand::Read) => {
                    if self.read_tx.write_u16_replicated(req.value) == false {
                        xfer.reject().unwrap();
                        return;
                    }

                    if let Some(b) = self.read_rx.read() {
                        xfer.accept(|buf| {
                            buf[0] = b as u8;
                            Ok(1)
                        })
                        .unwrap();
                    } else {
                        xfer.reject().unwrap();
                    }
                }

                Ok(c) => {
                    todo!("unimplemented command: {c:?}");
                }

                Err(_) => {
                    xfer.reject().unwrap();
                }
            }
        }
    }

    fn control_out(&mut self, xfer: ControlOut<B>) {
        let req = xfer.request();
        if req.request_type == RequestType::Vendor {
            let cmd = ControlCommand::try_from(req.request);

            match cmd {
                Ok(ControlCommand::RebootToUSB) => {
                    unsafe { ROM::reset_usb_boot(None, false, false) };
                }

                Ok(ControlCommand::Write) => {
                    if self.write_tx.write_u16_replicated(req.value) {
                        xfer.accept().unwrap();
                    } else {
                        xfer.reject().unwrap();
                    }
                }

                Ok(c) => {
                    todo!("unimplemented command: {c:?}");
                }

                Err(_) => {
                    xfer.reject().unwrap();
                }
            }
        }
    }
}

impl<'a, B: UsbBus, ReadSM, WriteSM> Bridge<'a, B, ReadSM, WriteSM>
where
    ReadSM: ValidStateMachine,
    WriteSM: ValidStateMachine,
{
    pub fn new(
        alloc: &'a UsbBusAllocator<B>,
        read: (
            StateMachine<ReadSM, Running>,
            Rx<ReadSM, Byte>,
            Tx<ReadSM, Byte>,
        ),
        write: (StateMachine<WriteSM, Running>, Tx<WriteSM, HalfWord>),
    ) -> Self {
        Self {
            iface: alloc.interface(),
            write_ep: alloc
                .alloc(
                    Some(EndpointAddress::from_parts(0x01, UsbDirection::In)),
                    EndpointType::Bulk,
                    BRIDGE_WRITE_SIZE as _,
                    1,
                )
                .expect("alloc_ep failed"),
            read_ep: alloc
                .alloc(
                    Some(EndpointAddress::from_parts(0x02, UsbDirection::Out)),
                    EndpointType::Bulk,
                    BRIDGE_READ_SIZE as _,
                    1,
                )
                .expect("alloc_ep failed"),
            read_sm: read.0,
            write_sm: write.0,
            read_rx: read.1,
            read_tx: read.2,
            write_tx: write.1,
            send_buffer: [0; BRIDGE_WRITE_SIZE],
            send_len: 0,
            recv_buffer: [0; BRIDGE_READ_SIZE],
            recv_len: 0,
        }
    }

    pub fn read(&mut self) -> Result<usize> {
        if self.recv_len >= self.recv_buffer.len() {
            return Err(UsbError::WouldBlock);
        }
        let amount = self.read_ep.read(&mut self.recv_buffer[self.recv_len..])?;
        self.recv_len += amount;
        Ok(amount)
    }

    pub fn write(&mut self) -> Result<usize> {
        if self.send_len == 0 {
            return Err(UsbError::WouldBlock);
        }
        let res = self.write_ep.write(&self.send_buffer[..self.send_len + 2]);
        if res.is_ok() {
            let amount = *res.as_ref().unwrap();
            if amount > 2 {
                self.send_buffer
                    .copy_within((amount)..(self.send_len + 2), 2);
                let actual_amount = amount - 2;
                self.send_len -= actual_amount;
            }
        }
        res
    }

    pub fn handle(&mut self) -> Result<()> {
        Ok(())
    }
}

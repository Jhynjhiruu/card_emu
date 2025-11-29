use rp235x_hal::dma::{Byte, HalfWord};
use rp235x_hal::pio::{PIO0SM0, Running, Rx, StateMachine, Tx, ValidStateMachine};
use usb_device::bus::{InterfaceNumber, UsbBus, UsbBusAllocator};
use usb_device::class::{ControlIn, ControlOut, UsbClass};
use usb_device::control::RequestType;
use usb_device::endpoint::{EndpointAddress, EndpointIn, EndpointOut, EndpointType};
use usb_device::{Result, UsbDirection, UsbError};

use crate::rom::ROM;

// maximum size allowed for bulk endpoints
const BRIDGE_WRITE_SIZE: usize = 64;
const BRIDGE_READ_SIZE: usize = 64;

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

    WriteFromBuf = 0x10,
    ReadIntoBuf = 0x11,
    WriteBitsFromBuf = 0x12,

    GetRecvLen = 0x80,
    GetSendLen = 0x81,

    RebootToUSB = 0xFF,
}

impl TryFrom<u8> for ControlCommand {
    type Error = u8;

    fn try_from(value: u8) -> core::result::Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::Write),
            0x01 => Ok(Self::Read),

            0x10 => Ok(Self::WriteFromBuf),
            0x11 => Ok(Self::ReadIntoBuf),
            0x12 => Ok(Self::WriteBitsFromBuf),

            0x80 => Ok(Self::GetRecvLen),
            0x81 => Ok(Self::GetSendLen),

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

                    while self.read_rx.is_empty() {
                        // wait
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

                Ok(ControlCommand::GetRecvLen) => xfer
                    .accept(|buf| {
                        buf[0..size_of::<u32>()]
                            .copy_from_slice(&(self.recv_len as u32).to_be_bytes());
                        Ok(size_of::<u32>())
                    })
                    .unwrap(),

                Ok(ControlCommand::GetSendLen) => xfer
                    .accept(|buf| {
                        buf[0..size_of::<u32>()]
                            .copy_from_slice(&(self.send_len as u32).to_be_bytes());
                        Ok(size_of::<u32>())
                    })
                    .unwrap(),

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
                    while self.write_tx.is_full() {
                        // do nothing
                    }

                    if self.write_tx.write_u16_replicated(req.value) {
                        xfer.accept().unwrap();
                    } else {
                        xfer.reject().unwrap();
                    }

                    while !self.write_tx.is_empty() {
                        // do nothing
                    }
                }

                Ok(ControlCommand::WriteFromBuf) => {
                    let to_write = usize::from(req.index);

                    if to_write > self.recv_len {
                        xfer.reject().unwrap();
                        return;
                    }

                    for &b in &self.recv_buffer[..to_write] {
                        while self.write_tx.is_full() {
                            // do nothing
                        }

                        if self.write_tx.write_u16_replicated(req.value | u16::from(b)) == false {
                            xfer.reject().unwrap();
                            return;
                        }
                    }

                    self.recv_buffer.copy_within(to_write.., 0);
                    self.recv_len -= to_write;

                    while !self.write_tx.is_empty() {
                        // do nothing
                    }

                    xfer.accept().unwrap();
                }

                Ok(ControlCommand::WriteBitsFromBuf) => {
                    let to_write = usize::from(req.index);

                    if to_write > self.recv_len {
                        xfer.reject().unwrap();
                        return;
                    }

                    for &b in &self.recv_buffer[..to_write] {
                        for i in 0..u8::BITS {
                            while self.write_tx.is_full() {
                                // do nothing
                            }

                            if self
                                .write_tx
                                .write_u16_replicated(req.value | u16::from((b >> i) & 1))
                                == false
                            {
                                xfer.reject().unwrap();
                                return;
                            }
                        }
                    }

                    self.recv_buffer.copy_within(to_write.., 0);
                    self.recv_len -= to_write;

                    while !self.write_tx.is_empty() {
                        // do nothing
                    }

                    xfer.accept().unwrap();
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
        let res = self.write_ep.write(&self.send_buffer[..self.send_len]);
        if res.is_ok() {
            let amount = *res.as_ref().unwrap();
            if amount > 0 {
                self.send_buffer.copy_within((amount)..(self.send_len), 0);
                self.send_len -= amount;
            }
        }
        res
    }

    pub fn receive(&mut self, amount: usize) -> Result<()> {
        Ok(())
    }

    pub fn clear(&mut self, amount: usize) -> Result<()> {
        Ok(())
    }
}

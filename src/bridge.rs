use usb_device::bus::{InterfaceNumber, UsbBus, UsbBusAllocator};
use usb_device::class::{ControlIn, ControlOut, UsbClass};
use usb_device::control::RequestType;
use usb_device::endpoint::{EndpointAddress, EndpointIn, EndpointOut, EndpointType};
use usb_device::{Result, UsbDirection, UsbError};

const BRIDGE_WRITE_SIZE: usize = 64;
const BRIDGE_READ_SIZE: usize = 32;

pub struct Bridge<'a, B: UsbBus> {
    class: BridgeClass<'a, B>,
    send_buffer: [u8; BRIDGE_WRITE_SIZE],
    send_len: usize,
    recv_buffer: [u8; BRIDGE_READ_SIZE],
    recv_len: usize,
}

impl<'a, B: UsbBus> UsbClass<B> for Bridge<'a, B> {
    fn get_configuration_descriptors(
        &self,
        writer: &mut usb_device::descriptor::DescriptorWriter,
    ) -> Result<()> {
        self.class.get_configuration_descriptors(writer)
    }

    fn reset(&mut self) {
        self.class.reset();
        self.send_len = 0;
        self.recv_len = 0;
    }

    fn control_in(&mut self, xfer: ControlIn<B>) {
        self.class.control_in(xfer);
    }

    fn control_out(&mut self, xfer: ControlOut<B>) {
        let req = xfer.request();
        if req.request_type == RequestType::Vendor {
            match req.request {
                _ => {
                    xfer.reject().unwrap();
                }
            }
        }
    }
}

impl<'a, B: UsbBus> Bridge<'a, B> {
    pub fn new(alloc: &'a UsbBusAllocator<B>) -> Self {
        Self {
            class: BridgeClass::new(alloc, BRIDGE_WRITE_SIZE as u16, BRIDGE_READ_SIZE as u16),
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
        let amount = self.class.read(&mut self.recv_buffer[self.recv_len..])?;
        self.recv_len += amount;
        Ok(amount)
    }

    pub fn write(&mut self) -> Result<usize> {
        if self.send_len == 0 {
            return Err(UsbError::WouldBlock);
        }
        let res = self.class.write(&self.send_buffer[..self.send_len + 2]);
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

pub struct BridgeClass<'a, B: UsbBus> {
    iface: InterfaceNumber,
    pub read_ep: EndpointOut<'a, B>,
    pub write_ep: EndpointIn<'a, B>,
}

impl<'a, B: UsbBus> UsbClass<B> for BridgeClass<'a, B> {
    fn get_configuration_descriptors(
        &self,
        writer: &mut usb_device::descriptor::DescriptorWriter,
    ) -> usb_device::Result<()> {
        writer.interface(self.iface, 0xFF, 0xFF, 0xFF)?;
        writer.endpoint(&self.write_ep)?;
        writer.endpoint(&self.read_ep)?;
        Ok(())
    }

    fn reset(&mut self) {}

    fn control_in(&mut self, xfer: ControlIn<B>) {
        let req = xfer.request();

        if req.request_type == RequestType::Vendor {
            match req.request {
                _ => {
                    xfer.reject().unwrap();
                }
            }
        }
    }
}

impl<'a, B: UsbBus> BridgeClass<'a, B> {
    pub fn new(
        alloc: &'a UsbBusAllocator<B>,
        max_write_packet_size: u16,
        max_read_packet_size: u16,
    ) -> Self {
        Self {
            iface: alloc.interface(),
            write_ep: alloc
                .alloc(
                    Some(EndpointAddress::from_parts(0x01, UsbDirection::In)),
                    EndpointType::Bulk,
                    max_write_packet_size,
                    1,
                )
                .expect("alloc_ep failed"),
            read_ep: alloc
                .alloc(
                    Some(EndpointAddress::from_parts(0x02, UsbDirection::Out)),
                    EndpointType::Bulk,
                    max_read_packet_size,
                    1,
                )
                .expect("alloc_ep failed"),
        }
    }

    pub fn read(&mut self, data: &mut [u8]) -> Result<usize> {
        self.read_ep.read(data)
    }

    pub fn write(&mut self, data: &[u8]) -> Result<usize> {
        self.write_ep.write(data)
    }
}

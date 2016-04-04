use alloc::boxed::Box;

use collections::Vec;
use collections::string::ToString;

use common::random::rand;

use core::{cmp, mem, slice, str};

use fs::{KScheme, Resource, Url};

use network::common::{n16, n32, Checksum, Ipv4Addr, IP_ADDR, FromBytes, ToBytes};

use system::error::{Error, Result, ENOENT, EPIPE};

#[derive(Copy, Clone)]
#[repr(packed)]
pub struct TcpHeader {
    pub src: n16,
    pub dst: n16,
    pub sequence: n32,
    pub ack_num: n32,
    pub flags: n16,
    pub window_size: n16,
    pub checksum: Checksum,
    pub urgent_pointer: n16,
}

pub struct Tcp {
    pub header: TcpHeader,
    pub options: Vec<u8>,
    pub data: Vec<u8>,
}

pub const TCP_FIN: u16 = 1;
pub const TCP_SYN: u16 = 1 << 1;
pub const TCP_RST: u16 = 1 << 2;
pub const TCP_PSH: u16 = 1 << 3;
pub const TCP_ACK: u16 = 1 << 4;

impl FromBytes for Tcp {
    fn from_bytes(bytes: Vec<u8>) -> Option<Self> {
        if bytes.len() >= mem::size_of::<TcpHeader>() {
            unsafe {
                let header = *(bytes.as_ptr() as *const TcpHeader);
                let header_len = ((header.flags.get() & 0xF000) >> 10) as usize;

                return Some(Tcp {
                    header: header,
                    options: bytes[mem::size_of::<TcpHeader>()..header_len].to_vec(),
                    data: bytes[header_len..bytes.len()].to_vec(),
                });
            }
        }
        None
    }
}

impl ToBytes for Tcp {
    fn to_bytes(&self) -> Vec<u8> {
        unsafe {
            let header_ptr: *const TcpHeader = &self.header;
            let mut ret = Vec::from(slice::from_raw_parts(header_ptr as *const u8,
                                                          mem::size_of::<TcpHeader>()));
            ret.extend_from_slice(&self.options);
            ret.extend_from_slice(&self.data);
            ret
        }
    }
}

/// A TCP resource
pub struct TcpResource {
    ip: Box<Resource>,
    peer_addr: Ipv4Addr,
    peer_port: u16,
    host_port: u16,
    sequence: u32,
    acknowledge: u32,
}

impl Resource for TcpResource {
    fn dup(&self) -> Result<Box<Resource>> {
        match self.ip.dup() {
            Ok(ip) => {
                Ok(box TcpResource {
                    ip: ip,
                    peer_addr: self.peer_addr,
                    peer_port: self.peer_port,
                    host_port: self.host_port,
                    sequence: self.sequence,
                    acknowledge: self.acknowledge,
                })
            }
            Err(err) => Err(err),
        }
    }

    fn path(&self, buf: &mut [u8]) -> Result<usize> {
        let path_string = format!("udp:{}:{}/{}", self.peer_addr.to_string(), self.peer_port, self.host_port);
        let path = path_string.as_bytes();

        for (b, p) in buf.iter_mut().zip(path.iter()) {
            *b = *p;
        }

        Ok(cmp::min(buf.len(), path.len()))
    }

    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        loop {
            let mut bytes = [0; 8192];
            match self.ip.read(&mut bytes) {
                Ok(count) => {
                    if let Some(segment) = Tcp::from_bytes(bytes[.. count].to_vec()) {
                        if (segment.header.flags.get() & (TCP_PSH | TCP_SYN | TCP_ACK)) ==
                           (TCP_PSH | TCP_ACK) &&
                           segment.header.dst.get() == self.host_port &&
                           segment.header.src.get() == self.peer_port {
                            // Send ACK
                            self.sequence = segment.header.ack_num.get();
                            self.acknowledge = segment.header.sequence.get() +
                                               segment.data.len() as u32;
                            let mut tcp = Tcp {
                                        header: TcpHeader {
                                            src: n16::new(self.host_port),
                                            dst: n16::new(self.peer_port),
                                            sequence: n32::new(self.sequence),
                                            ack_num: n32::new(self.acknowledge),
                                            flags: n16::new(((mem::size_of::<TcpHeader>() << 10) & 0xF000) as u16 | TCP_ACK),
                                            window_size: n16::new(65535),
                                            checksum: Checksum {
                                                data: 0
                                            },
                                            urgent_pointer: n16::new(0)
                                        },
                                        options: Vec::new(),
                                        data: Vec::new()
                                    };

                            unsafe {
                                let proto = n16::new(0x06);
                                let segment_len = n16::new((mem::size_of::<TcpHeader>() + tcp.options.len() + tcp.data.len()) as u16);
                                tcp.header.checksum.data = Checksum::compile(
                                            Checksum::sum((&IP_ADDR as *const Ipv4Addr) as usize, mem::size_of::<Ipv4Addr>()) +
                                            Checksum::sum((&self.peer_addr as *const Ipv4Addr) as usize, mem::size_of::<Ipv4Addr>()) +
                                            Checksum::sum((&proto as *const n16) as usize, mem::size_of::<n16>()) +
                                            Checksum::sum((&segment_len as *const n16) as usize, mem::size_of::<n16>()) +
                                            Checksum::sum((&tcp.header as *const TcpHeader) as usize, mem::size_of::<TcpHeader>()) +
                                            Checksum::sum(tcp.options.as_ptr() as usize, tcp.options.len()) +
                                            Checksum::sum(tcp.data.as_ptr() as usize, tcp.data.len())
                                            );
                            }

                            let _ = self.ip.write(&tcp.to_bytes());

                            // TODO: Support broken packets (one packet in two buffers)
                            let mut i = 0;
                            while i < buf.len() && i < segment.data.len() {
                                buf[i] = segment.data[i];
                                i += 1;
                            }
                            return Ok(i);
                        }
                    }
                }
                Err(err) => return Err(err),
            }
        }
    }

    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let tcp_data = Vec::from(buf);

        let mut tcp = Tcp {
            header: TcpHeader {
                src: n16::new(self.host_port),
                dst: n16::new(self.peer_port),
                sequence: n32::new(self.sequence),
                ack_num: n32::new(self.acknowledge),
                flags: n16::new((((mem::size_of::<TcpHeader>()) << 10) & 0xF000) as u16 | TCP_PSH |
                                TCP_ACK),
                window_size: n16::new(65535),
                checksum: Checksum { data: 0 },
                urgent_pointer: n16::new(0),
            },
            options: Vec::new(),
            data: tcp_data,
        };

        unsafe {
            let proto = n16::new(0x06);
            let segment_len = n16::new((mem::size_of::<TcpHeader>() + tcp.data.len()) as u16);
            tcp.header.checksum.data =
                Checksum::compile(Checksum::sum((&IP_ADDR as *const Ipv4Addr) as usize,
                                                mem::size_of::<Ipv4Addr>()) +
                                  Checksum::sum((&self.peer_addr as *const Ipv4Addr) as usize,
                                                mem::size_of::<Ipv4Addr>()) +
                                  Checksum::sum((&proto as *const n16) as usize,
                                                mem::size_of::<n16>()) +
                                  Checksum::sum((&segment_len as *const n16) as usize,
                                                mem::size_of::<n16>()) +
                                  Checksum::sum((&tcp.header as *const TcpHeader) as usize,
                                                mem::size_of::<TcpHeader>()) +
                                  Checksum::sum(tcp.options.as_ptr() as usize, tcp.options.len()) +
                                  Checksum::sum(tcp.data.as_ptr() as usize, tcp.data.len()));
        }

        match self.ip.write(&tcp.to_bytes()) {
            Ok(size) => {
                loop {
                    // Wait for ACK
                    let mut bytes = [0; 8192];
                    match self.ip.read(&mut bytes) {
                        Ok(count) => {
                            if let Some(segment) = Tcp::from_bytes(bytes[.. count].to_vec()) {
                                if segment.header.dst.get() == self.host_port &&
                                   segment.header.src.get() == self.peer_port {
                                    return if (segment.header.flags.get() & (TCP_PSH | TCP_SYN | TCP_ACK)) == TCP_ACK {
                                        self.sequence = segment.header.ack_num.get();
                                        self.acknowledge = segment.header.sequence.get();
                                        Ok(size)
                                    } else {
                                        Err(Error::new(EPIPE))
                                    };
                                }
                            }
                        }
                        Err(err) => return Err(err),
                    }
                }
            }
            Err(err) => Err(err),
        }
    }

    fn sync(&mut self) -> Result<()> {
        self.ip.sync()
    }
}

impl TcpResource {
    /// Etablish client
    pub fn client_establish(&mut self) -> bool {
        // Send SYN
        let mut tcp = Tcp {
            header: TcpHeader {
                src: n16::new(self.host_port),
                dst: n16::new(self.peer_port),
                sequence: n32::new(self.sequence),
                ack_num: n32::new(self.acknowledge),
                flags: n16::new(((mem::size_of::<TcpHeader>() << 10) & 0xF000) as u16 | TCP_SYN),
                window_size: n16::new(65535),
                checksum: Checksum { data: 0 },
                urgent_pointer: n16::new(0),
            },
            options: Vec::new(),
            data: Vec::new(),
        };

        unsafe {
            let proto = n16::new(0x06);
            let segment_len = n16::new((mem::size_of::<TcpHeader>() + tcp.options.len() +
                                        tcp.data
                                           .len()) as u16);
            tcp.header.checksum.data =
                Checksum::compile(Checksum::sum((&IP_ADDR as *const Ipv4Addr) as usize,
                                                mem::size_of::<Ipv4Addr>()) +
                                  Checksum::sum((&self.peer_addr as *const Ipv4Addr) as usize,
                                                mem::size_of::<Ipv4Addr>()) +
                                  Checksum::sum((&proto as *const n16) as usize,
                                                mem::size_of::<n16>()) +
                                  Checksum::sum((&segment_len as *const n16) as usize,
                                                mem::size_of::<n16>()) +
                                  Checksum::sum((&tcp.header as *const TcpHeader) as usize,
                                                mem::size_of::<TcpHeader>()) +
                                  Checksum::sum(tcp.options.as_ptr() as usize, tcp.options.len()) +
                                  Checksum::sum(tcp.data.as_ptr() as usize, tcp.data.len()));
        }

        match self.ip.write(&tcp.to_bytes()) {
            Ok(_) => {
                loop {
                    // Wait for SYN-ACK
                    let mut bytes = [0; 8192];
                    match self.ip.read(&mut bytes) {
                        Ok(count) => {
                            if let Some(segment) = Tcp::from_bytes(bytes[.. count].to_vec()) {
                                if segment.header.dst.get() == self.host_port &&
                                   segment.header.src.get() == self.peer_port {
                                    return if (segment.header.flags.get() & (TCP_PSH | TCP_SYN | TCP_ACK)) == (TCP_SYN | TCP_ACK) {
                                        self.sequence = segment.header.ack_num.get();
                                        self.acknowledge = segment.header.sequence.get();

                                        self.acknowledge += 1;
                                        tcp = Tcp {
                                                header: TcpHeader {
                                                    src: n16::new(self.host_port),
                                                    dst: n16::new(self.peer_port),
                                                    sequence: n32::new(self.sequence),
                                                    ack_num: n32::new(self.acknowledge),
                                                    flags: n16::new(((mem::size_of::<TcpHeader>() << 10) & 0xF000) as u16 | TCP_ACK),
                                                    window_size: n16::new(65535),
                                                    checksum: Checksum {
                                                        data: 0
                                                    },
                                                    urgent_pointer: n16::new(0)
                                                },
                                                options: Vec::new(),
                                                data: Vec::new()
                                            };

                                        unsafe {
                                            let proto = n16::new(0x06);
                                            let segment_len = n16::new((mem::size_of::<TcpHeader>() + tcp.options.len() + tcp.data.len()) as u16);
                                            tcp.header.checksum.data = Checksum::compile(
                                                    Checksum::sum((&IP_ADDR as *const Ipv4Addr) as usize, mem::size_of::<Ipv4Addr>()) +
                                                    Checksum::sum((&self.peer_addr as *const Ipv4Addr) as usize, mem::size_of::<Ipv4Addr>()) +
                                                    Checksum::sum((&proto as *const n16) as usize, mem::size_of::<n16>()) +
                                                    Checksum::sum((&segment_len as *const n16) as usize, mem::size_of::<n16>()) +
                                                    Checksum::sum((&tcp.header as *const TcpHeader) as usize, mem::size_of::<TcpHeader>()) +
                                                    Checksum::sum(tcp.options.as_ptr() as usize, tcp.options.len()) +
                                                    Checksum::sum(tcp.data.as_ptr() as usize, tcp.data.len())
                                                    );
                                        }

                                        let _ = self.ip.write(&tcp.to_bytes());

                                        true
                                    } else {
                                        false
                                    };
                                }
                            }
                        }
                        Err(_) => return false,
                    }
                }
            }
            Err(_) => false,
        }
    }

    /// Try to establish a server connection
    pub fn server_establish(&mut self, _: Tcp) -> bool {
        // Send SYN-ACK
        self.acknowledge += 1;
        let mut tcp = Tcp {
            header: TcpHeader {
                src: n16::new(self.host_port),
                dst: n16::new(self.peer_port),
                sequence: n32::new(self.sequence),
                ack_num: n32::new(self.acknowledge),
                flags: n16::new(((mem::size_of::<TcpHeader>() << 10) & 0xF000) as u16 | TCP_SYN |
                                TCP_ACK),
                window_size: n16::new(65535),
                checksum: Checksum { data: 0 },
                urgent_pointer: n16::new(0),
            },
            options: Vec::new(),
            data: Vec::new(),
        };

        unsafe {
            let proto = n16::new(0x06);
            let segment_len = n16::new((mem::size_of::<TcpHeader>() + tcp.options.len() +
                                        tcp.data
                                           .len()) as u16);
            tcp.header.checksum.data =
                Checksum::compile(Checksum::sum((&IP_ADDR as *const Ipv4Addr) as usize,
                                                mem::size_of::<Ipv4Addr>()) +
                                  Checksum::sum((&self.peer_addr as *const Ipv4Addr) as usize,
                                                mem::size_of::<Ipv4Addr>()) +
                                  Checksum::sum((&proto as *const n16) as usize,
                                                mem::size_of::<n16>()) +
                                  Checksum::sum((&segment_len as *const n16) as usize,
                                                mem::size_of::<n16>()) +
                                  Checksum::sum((&tcp.header as *const TcpHeader) as usize,
                                                mem::size_of::<TcpHeader>()) +
                                  Checksum::sum(tcp.options.as_ptr() as usize, tcp.options.len()) +
                                  Checksum::sum(tcp.data.as_ptr() as usize, tcp.data.len()));
        }

        match self.ip.write(&tcp.to_bytes()) {
            Ok(_) => {
                loop {
                    // Wait for ACK
                    let mut bytes = [0; 8192];
                    match self.ip.read(&mut bytes) {
                        Ok(count ) => {
                            if let Some(segment) = Tcp::from_bytes(bytes[.. count].to_vec()) {
                                if segment.header.dst.get() == self.host_port &&
                                   segment.header.src.get() == self.peer_port {
                                    return if (segment.header.flags.get() &
                                               (TCP_PSH | TCP_SYN | TCP_ACK)) ==
                                              TCP_ACK {
                                        self.sequence = segment.header.ack_num.get();
                                        self.acknowledge = segment.header.sequence.get();
                                        true
                                    } else {
                                        false
                                    };
                                }
                            }
                        }
                        Err(_) => return false,
                    }
                }
            }
            Err(_) => false,
        }
    }
}

impl Drop for TcpResource {
    fn drop(&mut self) {
        // Send FIN-ACK
        let mut tcp = Tcp {
            header: TcpHeader {
                src: n16::new(self.host_port),
                dst: n16::new(self.peer_port),
                sequence: n32::new(self.sequence),
                ack_num: n32::new(self.acknowledge),
                flags: n16::new((((mem::size_of::<TcpHeader>()) << 10) & 0xF000) as u16 | TCP_FIN |
                                TCP_ACK),
                window_size: n16::new(65535),
                checksum: Checksum { data: 0 },
                urgent_pointer: n16::new(0),
            },
            options: Vec::new(),
            data: Vec::new(),
        };

        unsafe {
            let proto = n16::new(0x06);
            let segment_len = n16::new((mem::size_of::<TcpHeader>() + tcp.options.len() +
                                        tcp.data
                                           .len()) as u16);
            tcp.header.checksum.data =
                Checksum::compile(Checksum::sum((&IP_ADDR as *const Ipv4Addr) as usize,
                                                mem::size_of::<Ipv4Addr>()) +
                                  Checksum::sum((&self.peer_addr as *const Ipv4Addr) as usize,
                                                mem::size_of::<Ipv4Addr>()) +
                                  Checksum::sum((&proto as *const n16) as usize,
                                                mem::size_of::<n16>()) +
                                  Checksum::sum((&segment_len as *const n16) as usize,
                                                mem::size_of::<n16>()) +
                                  Checksum::sum((&tcp.header as *const TcpHeader) as usize,
                                                mem::size_of::<TcpHeader>()) +
                                  Checksum::sum(tcp.options.as_ptr() as usize, tcp.options.len()) +
                                  Checksum::sum(tcp.data.as_ptr() as usize, tcp.data.len()));
        }

        let _ = self.ip.write(&tcp.to_bytes());
    }
}

/// A TCP scheme
pub struct TcpScheme;

impl KScheme for TcpScheme {
    fn scheme(&self) -> &str {
        "tcp"
    }

    fn open(&mut self, url: Url, _: usize) -> Result<Box<Resource>> {
        let mut parts = url.reference().split('/');
        let remote = parts.next().unwrap_or("");
        let path = parts.next().unwrap_or("");

        let mut remote_parts = remote.split(':');
        let host = remote_parts.next().unwrap_or("");
        let port = remote_parts.next().unwrap_or("");

        if ! host.is_empty() && ! port.is_empty() {
            let peer_addr = Ipv4Addr::from_string(&host.to_string());
            let peer_port = port.parse::<u16>().unwrap_or(0);
            let host_port = (rand() % 32768 + 32768) as u16;

            match Url::from_str(&format!("ip:{}/6", peer_addr.to_string())).unwrap().open() {
                Ok(ip) => {
                    let mut ret = box TcpResource {
                        ip: ip,
                        peer_addr: peer_addr,
                        peer_port: peer_port,
                        host_port: host_port,
                        sequence: rand() as u32,
                        acknowledge: 0,
                    };

                    if ret.client_establish() {
                        return Ok(ret);
                    }
                }
                Err(err) => return Err(err),
            }
        } else if ! path.is_empty() {
            let host_port = path.parse::<u16>().unwrap_or(0);

            while let Ok(mut ip) = Url::from_str("ip:/6").unwrap().open() {
                let mut bytes = [0; 8192];
                match ip.read(&mut bytes) {
                    Ok(count) => {
                        if let Some(segment) = Tcp::from_bytes(bytes[.. count].to_vec()) {
                            if segment.header.dst.get() == host_port && (segment.header.flags.get() & (TCP_PSH | TCP_SYN | TCP_ACK)) == TCP_SYN {
                                let mut path = [0; 256];
                                if let Ok(path_count) = ip.path(&mut path) {
                                    let ip_reference = unsafe { str::from_utf8_unchecked(&path[.. path_count]) }.split(':').nth(1).unwrap_or("");
                                    let ip_remote = ip_reference.split('/').next().unwrap_or("");
                                    let peer_addr = ip_remote.split(':').next().unwrap_or("");

                                    let mut ret = box TcpResource {
                                        ip: ip,
                                        peer_addr: Ipv4Addr::from_string(&peer_addr.to_string()),
                                        peer_port: segment.header.src.get(),
                                        host_port: host_port,
                                        sequence: rand() as u32,
                                        acknowledge: segment.header.sequence.get(),
                                    };

                                    if ret.server_establish(segment) {
                                        return Ok(ret);
                                    }
                                }
                            }
                        }
                    }
                    Err(err) => return Err(err),
                }
            }
        }

        Err(Error::new(ENOENT))
    }
}
use std::fmt;
use std::io::{self, Read, Write};
#[cfg(not(target_os = "linux"))]
use std::net::{IpAddr, Ipv4Addr};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use ureq::config::Config;
use ureq::unversioned::resolver::DefaultResolver;
#[cfg(feature = "tls")]
use ureq::unversioned::transport::RustlsConnector;
use ureq::unversioned::transport::{
    Buffers, ConnectionDetails, Connector, LazyBuffers, NextTimeout, Transport,
};
use ureq::{Agent, ResponseExt};

#[cfg(not(target_os = "linux"))]
use crate::error::Error;
use crate::error::Result;

#[derive(Debug, Clone)]
pub struct HttpClient {
    agent: Agent,
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub final_url: String,
    pub body: String,
}

impl HttpClient {
    pub fn new(user_agent: Option<String>, iface: Option<String>) -> Result<Self> {
        let mut builder = Config::builder()
            .http_status_as_error(false)
            .max_redirects(5)
            .save_redirect_history(true)
            .timeout_global(Some(Duration::from_secs(30)))
            .accept("*/*")
            .accept_encoding("");

        builder = builder.user_agent(user_agent.unwrap_or_default());
        let config = builder.build();

        let agent = if let Some(iface) = iface.filter(|value| !value.trim().is_empty()) {
            let bind = BindTarget::for_interface(&iface)?;
            let connector = ().chain(BoundTcpConnector { bind });
            #[cfg(feature = "tls")]
            let connector = connector.chain(RustlsConnector::default());
            Agent::with_parts(config, connector, DefaultResolver::default())
        } else {
            config.into()
        };

        Ok(Self { agent })
    }

    pub fn get(&self, url: &str) -> Result<HttpResponse> {
        let mut response = self.agent.get(url).call()?;
        let final_url = response.get_uri().to_string();
        let body = response.body_mut().read_to_string()?;
        Ok(HttpResponse { final_url, body })
    }
}

#[derive(Debug, Clone)]
enum BindTarget {
    #[cfg(target_os = "linux")]
    Device(String),
    #[cfg(not(target_os = "linux"))]
    LocalIp(IpAddr),
}

impl BindTarget {
    fn for_interface(iface: &str) -> Result<Self> {
        #[cfg(target_os = "linux")]
        {
            return Ok(Self::Device(iface.to_owned()));
        }

        #[cfg(not(target_os = "linux"))]
        {
            let ip = interface_ipv4(iface)?;
            Ok(Self::LocalIp(IpAddr::V4(ip)))
        }
    }

    fn is_addr_compatible(&self, _addr: SocketAddr) -> bool {
        match self {
            #[cfg(target_os = "linux")]
            Self::Device(_) => true,
            #[cfg(not(target_os = "linux"))]
            Self::LocalIp(IpAddr::V4(_)) => _addr.is_ipv4(),
            #[cfg(not(target_os = "linux"))]
            Self::LocalIp(IpAddr::V6(_)) => _addr.is_ipv6(),
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn interface_ipv4(iface: &str) -> Result<Ipv4Addr> {
    if_addrs::get_if_addrs()?
        .into_iter()
        .find_map(|item| {
            if item.name == iface && !item.is_loopback() {
                match item.ip() {
                    IpAddr::V4(ip) => Some(ip),
                    IpAddr::V6(_) => None,
                }
            } else {
                None
            }
        })
        .ok_or_else(|| Error::msg(format!("interface `{iface}` has no usable IPv4 address")))
}

#[derive(Clone)]
struct BoundTcpConnector {
    bind: BindTarget,
}

impl<In: Transport> Connector<In> for BoundTcpConnector {
    type Out = BoundTcpTransport;

    fn connect(
        &self,
        details: &ConnectionDetails,
        chained: Option<In>,
    ) -> std::result::Result<Option<Self::Out>, ureq::Error> {
        if chained.is_some() {
            return Err(ureq::Error::ConnectionFailed);
        }

        for addr in details.addrs.iter().copied() {
            if !self.bind.is_addr_compatible(addr) {
                continue;
            }
            match connect_bound(addr, details, &self.bind) {
                Ok(stream) => {
                    let buffers = LazyBuffers::new(
                        details.config.input_buffer_size(),
                        details.config.output_buffer_size(),
                    );
                    return Ok(Some(BoundTcpTransport::new(stream, buffers)));
                }
                Err(err) if err.kind() == io::ErrorKind::ConnectionRefused => continue,
                Err(err) if err.kind() == io::ErrorKind::TimedOut => {
                    return Err(ureq::Error::Timeout(details.timeout.reason));
                }
                Err(err) => return Err(ureq::Error::Io(err)),
            }
        }

        Err(ureq::Error::ConnectionFailed)
    }
}

impl fmt::Debug for BoundTcpConnector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BoundTcpConnector")
            .field("bind", &self.bind)
            .finish()
    }
}

fn connect_bound(
    addr: SocketAddr,
    details: &ConnectionDetails,
    bind: &BindTarget,
) -> io::Result<TcpStream> {
    let socket = Socket::new(Domain::for_address(addr), Type::STREAM, Some(Protocol::TCP))?;

    match bind {
        #[cfg(target_os = "linux")]
        BindTarget::Device(name) => socket.bind_device(Some(name.as_bytes()))?,
        #[cfg(not(target_os = "linux"))]
        BindTarget::LocalIp(ip) => socket.bind(&SockAddr::from(SocketAddr::new(*ip, 0)))?,
    }

    let sock_addr = SockAddr::from(addr);
    if let Some(timeout) = details.timeout.not_zero() {
        socket.connect_timeout(&sock_addr, *timeout)?;
    } else {
        socket.connect(&sock_addr)?;
    }

    let stream: TcpStream = socket.into();
    if details.config.no_delay() {
        stream.set_nodelay(true)?;
    }
    Ok(stream)
}

struct BoundTcpTransport {
    stream: TcpStream,
    buffers: LazyBuffers,
    timeout_write: Option<ureq::unversioned::transport::time::Duration>,
    timeout_read: Option<ureq::unversioned::transport::time::Duration>,
}

impl BoundTcpTransport {
    fn new(stream: TcpStream, buffers: LazyBuffers) -> Self {
        Self {
            stream,
            buffers,
            timeout_write: None,
            timeout_read: None,
        }
    }
}

impl Transport for BoundTcpTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        &mut self.buffers
    }

    fn transmit_output(
        &mut self,
        amount: usize,
        timeout: NextTimeout,
    ) -> std::result::Result<(), ureq::Error> {
        maybe_update_timeout(
            timeout,
            &mut self.timeout_write,
            &self.stream,
            TcpStream::set_write_timeout,
        )?;

        let output = &self.buffers.output()[..amount];
        self.stream.write_all(output).map_err(|err| {
            if err.kind() == io::ErrorKind::TimedOut {
                ureq::Error::Timeout(timeout.reason)
            } else {
                ureq::Error::Io(err)
            }
        })
    }

    fn await_input(&mut self, timeout: NextTimeout) -> std::result::Result<bool, ureq::Error> {
        maybe_update_timeout(
            timeout,
            &mut self.timeout_read,
            &self.stream,
            TcpStream::set_read_timeout,
        )?;

        let input = self.buffers.input_append_buf();
        let amount = self.stream.read(input).map_err(|err| {
            if err.kind() == io::ErrorKind::TimedOut {
                ureq::Error::Timeout(timeout.reason)
            } else {
                ureq::Error::Io(err)
            }
        })?;
        self.buffers.input_appended(amount);

        Ok(amount > 0)
    }

    fn is_open(&mut self) -> bool {
        probe_tcp_stream(&mut self.stream).unwrap_or(false)
    }
}

impl fmt::Debug for BoundTcpTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BoundTcpTransport")
            .field("addr", &self.stream.peer_addr().ok())
            .finish()
    }
}

fn maybe_update_timeout(
    timeout: NextTimeout,
    previous: &mut Option<ureq::unversioned::transport::time::Duration>,
    stream: &TcpStream,
    f: impl Fn(&TcpStream, Option<Duration>) -> io::Result<()>,
) -> std::result::Result<(), ureq::Error> {
    let next = timeout.not_zero();
    if next != *previous {
        f(stream, next.map(|timeout| *timeout)).map_err(ureq::Error::Io)?;
        *previous = next;
    }
    Ok(())
}

fn probe_tcp_stream(stream: &mut TcpStream) -> std::result::Result<bool, ureq::Error> {
    stream.set_nonblocking(true).map_err(ureq::Error::Io)?;
    let mut buf = [0_u8];
    let result = match stream.read(&mut buf) {
        Err(err) if err.kind() == io::ErrorKind::WouldBlock => Ok(true),
        Ok(_) => Ok(false),
        Err(_) => Ok(false),
    };
    stream.set_nonblocking(false).map_err(ureq::Error::Io)?;
    result
}

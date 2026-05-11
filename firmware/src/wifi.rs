//! Self-contained one-shot Wi-Fi + NTP fetch.
//!
//! [`try_fetch_time`] connects to the configured Wi-Fi network, performs a
//! single SNTP transaction against a public NTP server, and tears everything
//! down before returning. Both success and failure paths drop every network
//! resource so that the rest of the firmware runs with the radio powered off
//! and `~30 KB` more RAM available for the matrix buffers.
//!
//! The Wi-Fi plumbing is intentionally a faithful mirror of the one in
//! [cpa/ulanzi-tc001](https://github.com/cpa/ulanzi-tc001) — that skeleton
//! was the only known-good ESP32 + esp-hal 1.0 + blocking-network-stack
//! configuration at the time this firmware was written.

use blocking_network_stack::Stack;
use esp_hal::{
    peripherals::WIFI,
    rng::Rng,
    time::{Duration, Instant},
};
use esp_println::println;
use esp_radio::wifi::{ClientConfig, Config as WifiConfig, ModeConfig, PowerSaveMode, WifiDevice};
use smoltcp::{
    iface::{Config as IfaceConfig, Interface, SocketSet, SocketStorage},
    socket::dns::DnsQuery,
    socket::udp::PacketMetadata,
    wire::{DhcpOption, DnsQueryType, EthernetAddress, HardwareAddress, IpAddress, Ipv4Address},
};

use crate::time::ntp::{NTP_PACKET_LEN, NtpRequest, parse_ntp_response};

const NTP_HOST: &str = "pool.ntp.org";
const NTP_PORT: u16 = 123;
const FALLBACK_DNS: Ipv4Address = Ipv4Address::new(1, 1, 1, 1);
const DNS_QUERY_SLOTS: usize = 2;
const UDP_PACKET_SLOTS: usize = 4;
const UDP_RX_BUF: usize = 256;
const UDP_TX_BUF: usize = 96;

/// Attempt to fetch the current unix timestamp via SNTP.
///
/// `timeout_ms` is the total wall-clock budget for *everything* (radio init,
/// DHCP, DNS, send, recv). After that budget is exhausted the function bails
/// out and returns `None`.
pub fn try_fetch_time<'a>(
    wifi_peripheral: WIFI<'a>,
    ssid: &str,
    password: &str,
    timeout_ms: u64,
) -> Option<i64> {
    if ssid.is_empty() {
        println!("wifi: no SSID configured, skipping NTP");
        return None;
    }

    let deadline = Instant::now();
    let budget = Duration::from_millis(timeout_ms);
    let expired = || deadline.elapsed() > budget;

    println!("wifi: bringing up radio");
    let radio = esp_radio::init().ok()?;
    let (mut controller, interfaces) =
        esp_radio::wifi::new(&radio, wifi_peripheral, WifiConfig::default()).ok()?;
    let mut device = interfaces.sta;
    let iface = make_interface(&mut device);

    let mut socket_storage: [SocketStorage; 4] = Default::default();
    let mut socket_set = SocketSet::new(&mut socket_storage[..]);
    let mut dhcp_socket = smoltcp::socket::dhcpv4::Socket::new();
    dhcp_socket.set_outgoing_options(&[DhcpOption {
        kind: 12,
        data: b"ulanzi-pomo",
    }]);
    socket_set.add(dhcp_socket);

    let rng = Rng::new();
    let now_fn = || Instant::now().duration_since_epoch().as_millis();
    let stack = Stack::new(iface, device, socket_set, now_fn, rng.random());
    let mut dns_storage: [Option<DnsQuery>; DNS_QUERY_SLOTS] = core::array::from_fn(|_| None);
    stack.configure_dns(&[IpAddress::Ipv4(FALLBACK_DNS)], &mut dns_storage);

    controller.set_power_saving(PowerSaveMode::None).ok()?;
    let cfg = ModeConfig::Client(
        ClientConfig::default()
            .with_ssid(ssid.into())
            .with_password(password.into()),
    );
    if controller.set_config(&cfg).is_err() {
        println!("wifi: set_config failed");
        return None;
    }
    if controller.start().is_err() {
        println!("wifi: start failed");
        return None;
    }
    if controller.connect().is_err() {
        println!("wifi: connect call failed");
        return None;
    }
    println!("wifi: associating with {}", ssid);

    while !matches!(controller.is_connected(), Ok(true)) {
        if expired() {
            println!("wifi: timeout waiting for association");
            let _ = controller.disconnect();
            let _ = controller.stop();
            return None;
        }
        stack.work();
    }

    loop {
        if expired() {
            println!("wifi: timeout waiting for DHCP");
            let _ = controller.disconnect();
            let _ = controller.stop();
            return None;
        }
        stack.work();
        if stack.is_iface_up() {
            break;
        }
    }
    println!("wifi: link up, ip = {:?}", stack.get_ip_info());

    let server_ip = match stack.dns_query(NTP_HOST, DnsQueryType::A) {
        Ok(addrs) => addrs
            .into_iter()
            .find(|addr| matches!(addr, IpAddress::Ipv4(_))),
        Err(err) => {
            println!("wifi: DNS query failed: {:?}", err);
            None
        }
    };
    let Some(server_ip) = server_ip else {
        let _ = controller.disconnect();
        let _ = controller.stop();
        return None;
    };

    // UDP scratch space — all stack-allocated and dropped with the socket.
    let mut rx_meta = [PacketMetadata::EMPTY; UDP_PACKET_SLOTS];
    let mut rx_buf = [0u8; UDP_RX_BUF];
    let mut tx_meta = [PacketMetadata::EMPTY; UDP_PACKET_SLOTS];
    let mut tx_buf = [0u8; UDP_TX_BUF];
    let mut udp =
        stack.get_udp_socket(&mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);

    // Bind to a fixed high port; NTP servers don't care about the source
    // port and a non-zero value avoids any smoltcp implementations that
    // treat 0 as "unspecified" (and therefore "do not accept replies").
    if udp.bind(47_123u16).is_err() {
        println!("wifi: UDP bind failed");
        drop(udp);
        let _ = controller.disconnect();
        let _ = controller.stop();
        return None;
    }

    let request = NtpRequest::to_bytes();
    if udp.send(server_ip, NTP_PORT, &request).is_err() {
        println!("wifi: NTP send failed");
        drop(udp);
        let _ = controller.disconnect();
        let _ = controller.stop();
        return None;
    }

    let mut response = [0u8; NTP_PACKET_LEN];
    let result = loop {
        if expired() {
            println!("wifi: timeout waiting for NTP response");
            break None;
        }
        match udp.receive(&mut response) {
            Ok((len, _, _)) if len >= NTP_PACKET_LEN => break parse_ntp_response(&response),
            Ok(_) => continue,
            Err(_) => {
                // No datagram available yet — keep servicing the stack.
                continue;
            }
        }
    };

    drop(udp);
    let _ = controller.disconnect();
    let _ = controller.stop();

    match result {
        Some(unix) => println!("ntp: ok, unix = {}", unix),
        None => println!("ntp: failed"),
    }

    result
}

fn make_interface(device: &mut WifiDevice<'_>) -> Interface {
    let mac = EthernetAddress::from_bytes(&device.mac_address());
    let cfg = IfaceConfig::new(HardwareAddress::Ethernet(mac));
    let smol_now = smoltcp::time::Instant::from_micros(
        Instant::now().duration_since_epoch().as_micros() as i64,
    );
    Interface::new(cfg, device, smol_now)
}

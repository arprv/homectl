//! # homectl
//!
//! homectl is a library for controlling various smart home devices.
#![recursion_limit="128"]
#![feature(custom_attribute)]
#![feature(clamp)]
pub mod prot {
//! This module contains traits defining capabilities smart home devices can
//! possess as well as concrete smart home device implementations.

    use std::net::IpAddr;
    use std::io::Result;
    use color_processing::Color;

    /// A smart home device.
    ///
    /// All smart home devices must implement this trait.
    pub trait SmartDevice {
        /// Attempts to construct a smart home device from IP address.
        fn from_address(addr: &IpAddr) -> Result<Option<Self>>
            where Self: std::marker::Sized;

        /// Attempts to find devices on LAN.
        fn discover() -> Result<Option<Vec<Self>>>
            where Self: std::marker::Sized;

        /// Attempts to update internal state.
        ///
        /// All `_set_` methods should call `refresh()` before returning. Those
        /// relying on device's state should first call it as well.
        fn refresh(&mut self) -> Result<()>;

        /// Attempts to turn the device on.
        fn set_on(&mut self, on: bool) -> Result<()>;

        /// Checks whether the device is on or not.
        fn is_on(&self) -> bool;

        /// Returns the address of the device.
        fn address(&self) -> IpAddr;

        /// Returns port used to communicate with the device.
        fn port(&self) -> u16;

        /// Returns name of the device.
        ///
        /// This can be any string, but should be unique enough.
        fn name(&self) -> String;
    }

    /// Smart home device that has RGB capability.
    pub trait Rgb: SmartDevice {
        /// Attempts to set color and brightness.
        ///
        /// `brightness` is clamped to [0, 1] and corresponds to value in HSV
        /// representation.
        fn rgb_set(&mut self, color: &Color, brightness: f32) -> Result<()>;

        /// Attempts to set color to the exact value of `color`.
        fn rgb_set_exact(&mut self, color: &Color) -> Result<()>;

        /// Attempts to set color, first dimming it to the previously set
        /// brightness.
        fn rgb_set_color(&mut self, color: &Color) -> Result<()>;

        /// Attempts to set brightness.
        fn rgb_set_brightness(&mut self, brightness: f32) -> Result<()>;

        /// Gets color.
        ///
        /// This method simply returns internally stored state. `refresh()`
        /// should first be called to assure the values returned by getters are
        /// accurate.
        fn rgb_color(&self) -> Color;

        /// Gets color.
        ///
        /// This method simply returns internally stored state. `refresh()`
        /// should first be called to assure the values returned by getters are
        /// accurate.
        fn rgb_brightness(&self) -> f32;

        /// Gets color.
        ///
        /// This method simply returns internally stored state. `refresh()`
        /// should first be called to assure the values returned by getters are
        /// accurate.
        fn rgb_exact(&self) -> Color;
        //fn rgb_temperature(&self) -> u16;
    }

    /// Smart home device that has brightness adjust capability.
    pub trait Mono: SmartDevice {
        /// Attempts to set brightness.
        ///
        /// `brightness` is clamped to [0, 1].
        fn mono_set(&mut self, brightness: f32) -> Result<()>;

        /// Gets brightness.
        ///
        /// This method simply returns internally stored state. `refresh()`
        /// should first be called to assure the values returned by getters are
        /// accurate.
        fn mono(&self) -> f32;
    }

    /// Smart home Device that has Correlated Color Temperature adjust
    /// capability.
    pub trait Cct: SmartDevice {
        /// Attempts to set color temperature and brightness.
        fn cct_set(&mut self, kelvin: u16, brightness: f32) -> Result<()>;

        /// Attempts to set color temperature keeping previously set brightness.
        fn cct_set_temperature(&mut self, kelvin: u16) -> Result<()>;

        /// Attempts to set brightness keeping previously set color temperature.
        fn cct_set_brightness(&mut self, brightness: f32) -> Result<()>;

        /// Gets temperature.
        ///
        /// This method simply returns internally stored state. `refresh()`
        /// should first be called to assure the values returned by getters are
        /// accurate.
        fn cct_temperature(&self) -> u16;

        /// Gets brightness.
        ///
        /// This method simply returns internally stored state. `refresh()`
        /// should first be called to assure the values returned by getters are
        /// accurate.
        fn cct_brightness(&self) -> f32;
    }

    pub mod led_net {
    //! Implementation of the LEDNET protocol
    //!
    //! # Note
    //! The protocol was reverse-engineered from and tested only on
    //! HF-LPB100-ZJ200 RGBWW model

        use super::SmartDevice;
        use super::Rgb;
        use super::Cct;
        use std::net::{TcpStream, UdpSocket, Ipv4Addr, IpAddr, SocketAddr};
        use std::io::Write;
        use std::io::Read;
        use std::time::Duration;
        use color_processing::Color;
        use std::io::Error;
        use std::io::ErrorKind;
        use std::io::Result;

        // TODO: move into impl LedNet?
        // TODO: enum for models
        const SUPPORTED: [&str; 1] = ["HF-LPB100-ZJ200",];
        const DISCO_PORT: u16 = 48899;
        const DISCO_MSG: &[u8] = b"HF-A11ASSISTHREAD";
        const PORT: u16 = 5577;

        /// Takes a comma separated list of values and returns it with its
        /// checksum appended.
        macro_rules! fin_cmd {
            ( $( $value:expr ),* ) => {
                [ $( $value ),* , 0u8 $( .wrapping_add($value) )* ]
            }
        }

        #[derive(Debug)]
        pub struct LedNet {
            addr: SocketAddr,
            model: &'static str,

            is_on: bool,
            rgb_color_bytes: (u8, u8, u8),
            cct_bytes: (u8, u8),

            rgb_brightness: f32,
            cct_temperature: u16,
            cct_brightness: f32,
        }

        mod op {
            pub const SET_POWER: u8 = 0x71;
            pub const SET_COLOR: u8 = 0x31;
            pub const GET_STATE: u8 = 0x81;
        }

        mod word {
            pub const TERMINATOR: u8    = 0x0f;
            pub const ON: u8            = 0x23;
            pub const OFF: u8           = 0x24;
            pub const WRITE_COLORS: u8  = 0xf0;
            pub const WRITE_WHITES: u8  = 0x0f;
            pub const WRITE_BOTH: u8    = WRITE_COLORS & WRITE_WHITES;
        }

        mod temp {
            const MIN_TEMP: u16 = 2800;
            const MAX_TEMP: u16 = 6500;
            const TEMP_RANGE: u16 = MAX_TEMP - MIN_TEMP;

            /// Converts device internal reprsentation of color temperature to
            /// Kelvin.
            pub fn to_kelvin(warm: u8, cold: u8) -> u16 {
                if warm != 0 || cold != 0 {
                    let warm = warm as f32;
                    let cold = cold as f32;

                    let wc = warm + cold;
                    let leftover = 0xff as f32 - wc;
                    let warm = warm + (leftover * (warm / wc));
                    let k = (0xff as f32 - warm)
                          * (TEMP_RANGE as f32 / 0xff as f32)
                          + MIN_TEMP as f32;
                    k as u16
                } else {
                    0
                }
            }

            /// Converts color temperature in Kelvin to the two-byte device
            /// internal representation.
            pub fn to_warm_cold(kelvin: u16) -> (u8, u8) {
                let temp = (kelvin.clamp(MIN_TEMP, MAX_TEMP) - MIN_TEMP) as f32;
                let warm = 0xff as f32  * (1.0 - temp / TEMP_RANGE as f32);
                let cold = 0xff as f32 * (temp / TEMP_RANGE as f32);

                (warm.ceil() as u8, cold.ceil() as u8)
            }
        }

        impl std::fmt::Display for LedNet {
            fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(
                    f,
                    "{name} -- Address: {addr} Power: {power} \
                    RGB: [{rgb} @ {rgb_b}%] CCT: [{white_t}K @ {white_b}%]",
                    name    = self.name(),
                    addr    = self.addr,
                    power   = if self.is_on { "ON" } else { "OFF" },
                    rgb     = self.rgb_color().to_rgb_string(),
                    rgb_b   = (100.0 * self.rgb_brightness) as u8,
                    white_t = self.cct_temperature,
                    white_b = (100.0 * self.cct_brightness) as u8
                )
            }
        }

        impl SmartDevice for LedNet {
            fn from_address(addr: &IpAddr) -> Result<Option<Self>> {
                let socket = UdpSocket::bind(
                    SocketAddr::from((Ipv4Addr::UNSPECIFIED, DISCO_PORT))
                )?;
                LedNet::disco_send(
                    &socket,
                    &SocketAddr::new(*addr, DISCO_PORT)
                )?;
                let timeout = Some(Duration::from_millis(2500));
                if let Some(dev) = LedNet::disco_recv(&socket, &timeout)? {
                    Ok(Some(dev))
                } else {
                    Ok(None)
                }
            }

            fn discover() -> Result<Option<Vec<LedNet>>> {
                let socket = UdpSocket::bind(
                    SocketAddr::from((Ipv4Addr::UNSPECIFIED, DISCO_PORT))
                )?;
                socket.set_broadcast(true)?;

                // Get a broadcast address for each interface
                let mut bcast_addrs = Vec::new();
                for iface in pnet_datalink::interfaces() {
                    if iface.is_up() && iface.is_broadcast() {
                        for ip in iface.ips {
                            if ip.is_ipv4() {
                                bcast_addrs.push(SocketAddr::from(
                                    (ip.broadcast(), DISCO_PORT)
                                ));
                            }
                        }
                    }
                }

                // Send the discovery message to each
                for bcast_addr in bcast_addrs {
                    LedNet::disco_send(&socket, &bcast_addr)?;
                }

                let mut devs = Vec::new();

                // If we block for more than two seconds assume no more
                // responses will come
                let timeout = Some(Duration::from_millis(2000));
                while let Ok(maybe_dev) = LedNet::disco_recv(&socket, &timeout){
                    if let Some(dev) = maybe_dev {
                        devs.push(dev);
                    }
                }

                if !devs.is_empty() {
                    Ok(Some(devs))
                } else {
                    Ok(None)
                }
            }

            // TODO: get timers, etc
            fn refresh(&mut self) -> Result<()> {
                const STATE_RESP_LEN: usize = 14;
                // TODO: What are the last two bytes?
                const GET_STATE_MSG: &[u8] = &fin_cmd![
                    op::GET_STATE, 0x8a, 0x8b
                ];

                // Tell the device we want its state
                let mut stream = TcpStream::connect(self.addr)?;
                stream.write_all(GET_STATE_MSG)?;

                // Try to read it in
                let timeout = Some(Duration::from_millis(2000));
                let state = LedNet::read_response(
                    &mut stream,
                    STATE_RESP_LEN,
                    &timeout
                )?;

                // Make sure the checksum is okay
                let checksum = state[..STATE_RESP_LEN - 1].iter()
                    .fold(0u8, |acc, b| acc.wrapping_add(*b));
                if state[STATE_RESP_LEN - 1] != checksum {
                    return Err(Error::new(
                        ErrorKind::Other,
                        "Invalid checksum of state query response".to_owned()
                    ));
                }

                // Compute brightnesses 
                let (_, _, rgb_b, _) = Color::new_rgb(
                    state[6],
                    state[7],
                    state[8]
                ).get_hsva();

                let cct_b = 1.0 -
                    (0xff as i32 - (state[9] as i32 + state[11] as i32)
                ) as f32 / 0xff as f32;

                // Update internal state
                self.is_on              = state[2] == word::ON;
                self.rgb_color_bytes    = (state[6], state[7], state[8]);
                self.cct_bytes          = (state[9], state[11]);
                self.rgb_brightness     = rgb_b as f32;
                self.cct_temperature    = temp::to_kelvin(state[9], state[11]);
                self.cct_brightness     = cct_b;
                Ok(())
            }

            fn set_on(&mut self, on: bool) -> Result<()> {
                const ON_COMMAND: &[u8] = &fin_cmd![
                    op::SET_POWER, word::ON, word::TERMINATOR
                ];
                const ON_RESPONSE: &[u8] = &fin_cmd![
                    word::TERMINATOR, op::SET_POWER, word::ON
                ];

                const OFF_COMMAND: &[u8] = &fin_cmd![
                    op::SET_POWER, word::OFF, word::TERMINATOR
                ];
                const OFF_RESPONSE: &[u8] = &fin_cmd![
                    word::TERMINATOR, op::SET_POWER, word::OFF
                ];

                if on {
                    self.write_command(ON_COMMAND, ON_RESPONSE)?;
                } else {
                    self.write_command(OFF_COMMAND, OFF_RESPONSE)?;
                }

                self.refresh()?;

                Ok(())
            }

            fn is_on(&self) -> bool {
                self.is_on
            }

            fn address(&self) -> IpAddr {
                self.addr.ip()
            }

            fn port(&self) -> u16 {
                self.addr.port()
            }

            fn name(&self) -> String {
                "LEDNET:".to_owned() + self.model
            }
        }

        impl Rgb for LedNet {
            fn rgb_set_exact(&mut self, color: &Color) -> Result<()> {
                let command = fin_cmd![
                    op::SET_COLOR,
                    color.red,
                    color.green,
                    color.blue,
                    0u8,
                    0u8,
                    word::WRITE_COLORS,
                    word::TERMINATOR
                ];
                self.write_command(&command, &[])?;
                self.refresh()?;
                Ok(())
            }

            fn rgb_set(
                &mut self,
                color: &Color,
                brightness: f32
            ) -> Result<()> {
                let (hue, sat, _, _) = color.get_hsva();
                self.rgb_set_exact(&Color::new_hsv(hue, sat, brightness.into()))
            }

            fn rgb_set_color(&mut self, color: &Color) -> Result<()> {
                self.refresh()?;
                self.rgb_set(color, self.rgb_brightness)
            }

            fn rgb_set_brightness(&mut self, brightness: f32) -> Result<()> {
                self.refresh()?;
                self.rgb_set(&self.rgb_color(), brightness)
            }

            fn rgb_color(&self) -> Color {
                let (hue, sat, _, _) = self.rgb_exact().get_hsva();
                Color::new_hsv(hue, sat, 1.0)
            }

            fn rgb_brightness(&self) -> f32 {
                self.rgb_brightness
            }

            fn rgb_exact(&self) -> Color {
                Color::new_rgb(
                    self.rgb_color_bytes.0,
                    self.rgb_color_bytes.1,
                    self.rgb_color_bytes.2
                )
            }
        }

        impl Cct for LedNet {
            fn cct_set(&mut self, kelvin: u16, brightness: f32) -> Result<()> {
                let (warm, cold) = temp::to_warm_cold(kelvin);
                let command = fin_cmd![
                    op::SET_COLOR,
                    0u8,
                    0u8,
                    0u8,
                    (warm as f32 * brightness.clamp(0.0, 1.0)) as u8,
                    (cold as f32 * brightness.clamp(0.0, 1.0)) as u8,
                    word::WRITE_WHITES,
                    word::TERMINATOR
                ];
                self.write_command(&command, &[])?;
                self.refresh()?;
                Ok(())
            }

            fn cct_set_temperature(&mut self, kelvin: u16) -> Result<()> {
                self.refresh()?;
                self.cct_set(kelvin, self.cct_brightness)
            }

            fn cct_set_brightness(&mut self, brightness: f32) -> Result<()> {
                self.refresh()?;
                self.cct_set(self.cct_temperature, brightness)
            }

            fn cct_temperature(&self) -> u16 {
                self.cct_temperature
            }

            fn cct_brightness(&self) -> f32 {
                self.cct_brightness
            }

        }

        impl LedNet {
        //! LEDNET specific functionality.

            /// Attempts to set WW and CW channels directly.
            pub fn set_ww_cw(&mut self, ww: u8, cw: u8) -> Result<()> {
                let command = fin_cmd![
                    op::SET_COLOR,
                    0u8,
                    0u8,
                    0u8,
                    ww,
                    cw,
                    word::WRITE_WHITES,
                    word::TERMINATOR
                ];
                self.write_command(&command, &[])?;
                self.refresh()?;
                Ok(())
            }

            /// Attempts to set RGB and CCT outputs simultaneously.
            pub fn set_rgb_cct(
                &mut self,
                color: Color,
                kelvin: u16
            ) -> Result<()> {
                let (warm, cold) = temp::to_warm_cold(kelvin);
                let command = fin_cmd![
                    op::SET_COLOR,
                    color.red,
                    color.green,
                    color.blue,
                    warm,
                    cold,
                    word::WRITE_BOTH,
                    word::TERMINATOR
                ];
                self.write_command(&command, &[])?;
                self.refresh()?;
                Ok(())
            }

            fn write_command(
                &self,
                command: &[u8],
                expected: &[u8]
            ) -> Result<()> {
                let mut stream = TcpStream::connect(self.addr)?;
                stream.write_all(&command)?;

                let timeout = Some(Duration::from_millis(2000));
                let response = LedNet::read_response(
                    &mut stream,
                    expected.len(),
                    &timeout
                )?;

                if response != expected {
                    Err(Error::new(ErrorKind::Other, "Incorrect response"))
                } else {
                    Ok(())
                }
            }

            fn read_response(
                mut stream: &TcpStream,
                len: usize,
                timeout: &Option<Duration>
            ) -> Result<Vec<u8>> {
                let old_timeout = stream.read_timeout();
                stream.set_read_timeout(*timeout)?;

                let mut response = vec![0u8; len];
                let ret = stream.read_exact(&mut response).map(|_| response);

                stream.set_read_timeout(old_timeout.unwrap_or(None))?;
                ret
            }

            fn disco_send(socket: &UdpSocket, addr: &SocketAddr) -> Result<()> {
                let sent = socket.send_to(DISCO_MSG, addr)?;

                if sent != DISCO_MSG.len() {
                    Err(Error::new(
                        ErrorKind::Other, 
                        "Could not write full discovery message"
                    ))
                } else {
                    Ok(())
                }

            }

            fn disco_recv(
                socket: &UdpSocket,
                timeout: &Option<Duration>
            ) -> Result<Option<LedNet>> {
                // Save the old timeout so we can reset it when we are done
                let old_timeout = socket.read_timeout();
                socket.set_read_timeout(*timeout)?;

                let mut buf = [0u8; 128];
                let (len, mut addr) = socket.recv_from(&mut buf)?;

                let mut maybe_dev: Option<LedNet> = None;

                // Expected format:
                // "192.168.1.212,F0FE6B5A6D68,HF-LPB100-ZJ200"
                //  \___________/ \__________/ \_____________/
                //        IP          MAC         Model ID
                if let Ok(response) = std::str::from_utf8(&buf[..len]) {
                    let mut fields = response.split(',');
                    if let Some(m_id) = fields.nth(2) {
                        let m_id = SUPPORTED.iter().find(|&&e| e == m_id);
                        if let Some(model) = m_id {
                            addr.set_port(PORT);
                            // There really should be better syntax to
                            // partially default initialize...
                            let mut dev = LedNet {
                                // TODO: Should we read the address from the
                                // reply instead?
                                addr,
                                model,
                                is_on: Default::default(),
                                rgb_color_bytes: Default::default(),
                                cct_bytes: Default::default(),
                                rgb_brightness: Default::default(),
                                cct_temperature: Default::default(),
                                cct_brightness: Default::default(),
                            };
                            dev.refresh()?;
                            maybe_dev = Some(dev);
                        }
                    }
                }

                socket.set_read_timeout(old_timeout.unwrap_or(None))?;
                Ok(maybe_dev)
            }
        }
    }

}

pub mod mult {
//! This module is an assortment of traits and enums that provide a unified
//! interface to control various smart home devices without the need to
//! explicitly handle each one.
//!
//! # Example
//!
//! ```
//! use mult::{Command, Device};
//!
//! if let Ok(Some(mut devs)) = Device::discover() {
//!     for dev in devs {
//!         dev.exec(&Command::On)?;
//!     }
//! }
//! ```

    use crate::prot::{SmartDevice, Rgb, Cct, Mono};
    use crate::prot::led_net::LedNet;
    
    use std::io;
    use std::error;
    use std::fmt;
    use std::net::IpAddr;
    use color_processing::Color;

    use homectl_macros::Dev;

    type Result = std::result::Result<Option<Response>, Error>;
    type Brightness = f32;
    type Kelvin = u16;

    /// Represents a smart home device.
    #[derive(Debug, Dev)]
    pub enum Device {
        #[homectl(cmd = "RgbCommands", cmd = "CctCommands")]
        LedNet(LedNet),
    }

    #[derive(Debug)]
    pub enum Error {
        CommandNotSupported,
        Io(io::Error),
    }

    impl error::Error for Error {}

    impl fmt::Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            use Error::*;
            match self {
                CommandNotSupported => write!(f, "Command not supported"),
                Io(e)               => write!(f, "I/O error: {}", e.to_string())
            }
        }
    }

    impl From<io::Error> for Error {
        fn from(err: io::Error) -> Error {
            Error::Io(err)
        }
    }

    /// Possible responses from various getters.
    pub enum Response {
        Color(Color),
        Brightness(Brightness),
        Temperature(Kelvin),
        IsOn(bool),
        Address(IpAddr),
        Port(u16),
    }

    impl fmt::Display for Response {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> fmt::Result {
            match self {
                Response::Color(c)       => write!(f, "{}", c.to_rgb_string()),
                Response::Brightness(b)  => write!(f, "{}", (100.0 * b) as u8),
                Response::Temperature(t) => write!(f, "{}", t),
                Response::IsOn(o)        => write!(f, "{}", o),
                Response::Address(a)     => write!(f, "{}", a),
                Response::Port(p)        => write!(f, "{}", p),
            }
        }
    }

    /// Supported commands.
    pub enum Command {
        On,
        Off,

        GetAddress,
        GetPort,
        IsOn,

        RgbSet(Color, Brightness),
        RgbSetExact(Color),
        RgbSetColor(Color),
        RgbSetBrightness(Brightness),

        RgbGetColor,
        RgbGetBrightness,
        RgbGetExact,

        CctSet(Kelvin, Brightness),
        CctSetTemperature(Kelvin),
        CctSetBrightness(Brightness),

        CctGetTemperature,
        CctGetBrightness,

        MonoSet(Brightness),

        MonoGet
    }

    trait SmartDeviceCommands {
        fn exec(&mut self, command: &Command) -> Result;
    }

    trait RgbCommands {
        fn exec(&mut self, command: &Command) -> Result;
    }

    trait CctCommands {
        fn exec(&mut self, command: &Command) -> Result;
    }

    trait MonoCommands {
        fn exec(&mut self, command: &Command) -> Result;
    }

    impl<T> SmartDeviceCommands for T where T: SmartDevice {
        fn exec(&mut self, command: &Command) -> Result {
            match command {
                Command::On => {
                    self.set_on(true)?;
                    Ok(None)
                },
                Command::Off => {
                    self.set_on(false)?;
                    Ok(None)
                },
                Command::GetAddress => {
                    Ok(Some(Response::Address(self.address())))
                },
                Command::GetPort => {
                    Ok(Some(Response::Port(self.port())))
                },
                Command::IsOn => {
                    Ok(Some(Response::IsOn(self.is_on())))
                },

                _ => Err(Error::CommandNotSupported)
            }
        }
    }

    impl<T> RgbCommands for T where T: Rgb {
        fn exec(&mut self, command: &Command) -> Result {
            match command {
                Command::RgbSet(c, b) => {
                    self.rgb_set(c, *b)?;
                    Ok(None)
                },
                Command::RgbSetExact(c) => {
                    self.rgb_set_exact(c)?;
                    Ok(None)
                },
                Command::RgbSetColor(c) => {
                    self.rgb_set_color(c)?;
                    Ok(None)
                },
                Command::RgbSetBrightness(b) => {
                    self.rgb_set_brightness(*b)?;
                    Ok(None)
                },
                Command::RgbGetColor => {
                    Ok(Some(Response::Color(self.rgb_color())))
                },
                Command::RgbGetBrightness => {
                    Ok(Some(Response::Brightness(self.rgb_brightness())))
                },
                Command::RgbGetExact => {
                    Ok(Some(Response::Color(self.rgb_exact())))
                },
                _ => Err(Error::CommandNotSupported)
            }
        }
    }

    impl<T> CctCommands for T where T: Cct {
        fn exec(&mut self, command: &Command) -> Result {
            match command {
                Command::CctSet(k, b) => {
                    self.cct_set(*k, *b)?;
                    Ok(None)
                },
                Command::CctSetTemperature(k) => {
                    self.cct_set_temperature(*k)?;
                    Ok(None)
                },
                Command::CctSetBrightness(b) => {
                    self.cct_set_brightness(*b)?;
                    Ok(None)
                },
                Command::CctGetTemperature => {
                    Ok(Some(Response::Temperature(self.cct_temperature())))
                },
                Command::CctGetBrightness => {
                    Ok(Some(Response::Brightness(self.cct_brightness())))
                },
                _ => Err(Error::CommandNotSupported)
            }
        }
    }

    impl<T> MonoCommands for T where T: Mono {
        fn exec(&mut self, command: &Command) -> Result {
            match command {
                Command::MonoSet(b) => {
                    self.mono_set(*b)?;
                    Ok(None)
                },
                Command::MonoGet => {
                    Ok(Some(Response::Brightness(self.mono())))
                },
                _ => Err(Error::CommandNotSupported)
            }
        }
    }
}


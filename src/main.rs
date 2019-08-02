#![feature(clamp)]

use std::{process, net::IpAddr};
use color_processing::Color;
use structopt::StructOpt;
use homectl::mult::{Commandable, Command, Device};

#[derive(StructOpt)]
#[structopt(
    about = "Control your smart home devices",
    raw(setting = "structopt::clap::AppSettings::InferSubcommands")
)]
struct HomeCtl {
    #[structopt(
        name = "address",
        value_name = "IP",
        help = "Address of the device",
        required_unless = "discover",
        overrides_with = "discover",
        parse(try_from_str)
    )]
    addr: Vec<IpAddr>,

    #[structopt(
        name = "discover",
        short = "d",
        long = "discover",
        help = "Tries to discover devices then applies command to all"
    )]
    discover: bool,

    #[structopt(subcommand)]
    cmd: ArgCmd,
}

#[derive(StructOpt)]
enum ArgCmd {
    #[structopt(name = "on", about = "Turns the device(s) on")]
    On ,
    #[structopt(name = "off", about = "Turns the device(s) off")]
    Off,
    #[structopt(
        name = "set",
        about = "Sets various device parameters",
        raw(setting = "structopt::clap::AppSettings::InferSubcommands")
    )]
    Set(Set),
    #[structopt(
        name = "get",
        about = "Gets various device parameters",
        raw(setting = "structopt::clap::AppSettings::InferSubcommands")
    )]
    Get(Get),

    #[structopt(
        name = "status",
        about = "Prints general device information",
    )]
    Status,
}

#[derive(StructOpt)]
enum Set {
    #[structopt(
        name = "rgb",
        raw(setting = "structopt::clap::AppSettings::InferSubcommands")
    )]
    SetRgb(SetRgb),

    #[structopt(
        name = "cct",
        raw(setting = "structopt::clap::AppSettings::InferSubcommands")
    )]
    SetCct(SetCct),

    #[structopt(
        name = "mono",
        raw(setting = "structopt::clap::AppSettings::InferSubcommands")
    )]
    SetMono {
        #[structopt(name = "brightness")]
        brightness: u8
    }
}

#[derive(StructOpt)]
enum SetRgb {
    #[structopt(name = "full")]
    Full {
        color: Color,
        brightness: u8
    },

    #[structopt(name = "color")]
    Color {
        color: Color
    },

    #[structopt(name = "brightness")]
    Brightness {
        brightness: u8
    },

    #[structopt(name = "exact")]
    Exact {
        color: Color
    }
}

#[derive(StructOpt)]
enum SetCct {
    #[structopt(name = "full")]
    Full {
        temperature: u16,
        brightness: u8
    },

    #[structopt(name = "temperature")]
    Temperature {
        temperature: u16
    },

    #[structopt(name = "brightness")]
    Brightness {
        brightness: u8
    },
}

#[derive(StructOpt)]
enum Get {
    #[structopt(
        name = "rgb",
        raw(setting = "structopt::clap::AppSettings::InferSubcommands")
    )]
    GetRgb(GetRgb),

    #[structopt(
        name = "cct",
        raw(setting = "structopt::clap::AppSettings::InferSubcommands")
    )]
    GetCct(GetCct),

    #[structopt(
        name = "mono",
    )]
    GetMono,

    #[structopt(name = "on")]
    GetOn,

    #[structopt(name = "address")]
    GetAddress,

    #[structopt(name = "port")]
    GetPort,
}

#[derive(StructOpt)]
enum GetRgb {
    #[structopt(name = "color")]
    Color,
    #[structopt(name = "brightness")]
    Brightness,
    #[structopt(name = "exact")]
    Exact
}

#[derive(StructOpt)]
enum GetCct {
    #[structopt(name = "temperature")]
    Temperature,
    #[structopt(name = "brightness")]
    Brightness
}

enum CommandType {
    Device(Command),
    Meta(ArgCmd)
}

impl From<ArgCmd> for CommandType {

    /// Converts ArgCmd to Command::Device if it can be executed by Device
    /// directly. Otherwise returns the ArgCmd instance wrapped in
    /// Commandtype::Meta.
    fn from(cmd: ArgCmd) -> CommandType {
        let normalize_brightness = |b| (b as f32).clamp(0.0, 100.0) / 100.0;
        match cmd {
            ArgCmd::On => CommandType::Device(Command::On),
            ArgCmd::Off => CommandType::Device(Command::Off),
            ArgCmd::Set(set) => {
                CommandType::Device(match set {
                    Set::SetRgb(set_rgb) => {
                        match set_rgb {
                            SetRgb::Full {color, brightness} => {
                                Command::RgbSet(
                                    color,
                                    normalize_brightness(brightness)
                                )
                            },
                            SetRgb::Color {color} => {
                                Command::RgbSetColor(color)
                            },
                            SetRgb::Brightness {brightness} => {
                                Command::RgbSetBrightness(
                                    normalize_brightness(brightness)
                                )
                            },
                            SetRgb::Exact {color} => {
                                Command::RgbSetExact(color)
                            },
                        }
                    },
                    Set::SetCct(set_cct) => {
                        match set_cct {
                            SetCct::Full {temperature, brightness} => {
                                Command::CctSet(
                                    temperature,
                                    normalize_brightness(brightness)
                                )

                            },
                            SetCct::Temperature {temperature} => {
                                Command::CctSetTemperature(
                                    temperature
                                )
                            },
                            SetCct::Brightness {brightness} => {
                                Command::CctSetBrightness(
                                    normalize_brightness(brightness)
                                )
                            },
                        }
                    }
                    Set::SetMono {brightness} => {
                        Command::MonoSet(
                            normalize_brightness(brightness)
                        )
                    },
                })
            },
            ArgCmd::Get(get) => {
                CommandType::Device(match get {
                    Get::GetOn => Command::IsOn,
                    Get::GetAddress => Command::GetAddress,
                    Get::GetPort => Command::GetPort,
                    Get::GetRgb(get_rgb) => {
                        match get_rgb {
                            GetRgb::Color => Command::RgbGetColor,
                            GetRgb::Brightness => Command::RgbGetBrightness,
                            GetRgb::Exact => Command::RgbGetExact,
                        }
                    },
                    Get::GetCct(get_cct) => {
                        match get_cct {
                            GetCct::Temperature => Command::CctGetTemperature,
                            GetCct::Brightness => Command::CctGetBrightness,
                        }
                    },
                    Get::GetMono => Command::MonoGet,
                })
            },

            ArgCmd::Status => CommandType::Meta(ArgCmd::Status),
        }
    }
}

fn main() {
    const FAILURE: i32 = 1;

    let opt = HomeCtl::from_args();

    let mut devs = Vec::new();

    // Discover devices
    if opt.discover {
        match Device::discover() {
            Ok(maybe_devs) => {
                if let Some(mut ds) = maybe_devs {
                    devs.append(&mut ds);
                } else {
                    println!("No devices found.");
                }
            },
            Err(e) => {
                eprintln!("Could not discover devices: {}", e);
                process::exit(FAILURE);
            }
        }
    // Connect directly
    } else {
        for addr in opt.addr {
            match Device::from_address(&addr) {
                Ok(maybe_dev) => {
                    if let Some(dev) = maybe_dev {
                        devs.push(dev);
                    } else {
                        println!("{}: Device not supported", addr);
                    }
                },
                Err(e) => {
                    eprintln!("Could not connect to {}: {}", addr, e);
                    process::exit(FAILURE);
                }
            }
        }

    }

    // Keep track of whether all commands succeeded so we can exit with an
    // appropriate value
    let mut all_succeeded = true;

    match opt.cmd.into() {
        CommandType::Device(cmd) => {
            for mut dev in devs {
                match dev.exec(&cmd) {
                    Ok(maybe_rv) => {
                        if let Some(rv) = maybe_rv {
                            println!("{}: {}", dev.description(), rv);
                        }
                    }
                    Err(_) => {
                        eprintln!(
                            "{}: Command not supported",
                            dev.description()
                        );
                        all_succeeded = false;
                    }
                }
            }
        },
        CommandType::Meta(cmd) => {
            match cmd {
                ArgCmd::Status => devs.iter().for_each(|d| println!("{}", d)),
                _ => unreachable!(), // Consider it a bug
            }
        }
    }

    if !all_succeeded {
        process::exit(FAILURE);
    }
}


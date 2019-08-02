# homectl
A smart home device client written in rust. This project is at it's very early stages.

## Supported Devices
### LEDNET HF-LPB100-ZJ200 (LED controller)
#### Missing features:
* Initial setup
* Timers
* "functions"

## Usage
```
USAGE:
    homectl [FLAGS] <IP>... <SUBCOMMAND>

FLAGS:
    -d, --discover    Tries to discover devices then applies command to all
    -h, --help        Prints help information
    -V, --version     Prints version information

ARGS:
    <IP>...    Address of the device

SUBCOMMANDS:
    get       Gets various device parameters
    help      Prints this message or the help of the given subcommand(s)
    off       Turns the device(s) off
    on        Turns the device(s) on
    set       Sets various device parameters
    status    Prints general device information
```
Print various device info
```
$ ./homectl -d stat
LEDNET:HF-LPB100-ZJ200 -- Address: 192.168.1.212:5577 Power: ON RGB: [rgb(255, 135, 30) @ 100%] CCT: [2800K @ 100%]
```
Get a specific device parameter
```
$ ./homectl 192.168.1.212 get rgb brightness
LEDNET:HF-LPB100-ZJ200 @ 192.168.1.212: 100
```
Commands can be abbreviated
```
$ ./homectl 192.168.1.212 set c b 80
$ ./homectl -d stat
LEDNET:HF-LPB100-ZJ200 -- Address: 192.168.1.212:5577 Power: ON RGB: [rgb(255, 135, 30) @ 100%] CCT: [2800K @ 80%]
```
Colors can be specified in several ways, for example:
```
$ ./homectl -d set rgb exact green
$ ./homectl -d set rgb exact "rgb(127, 255, 64)"
$ ./homectl -d set rgb exact "cmyk(100%, 0%, 0%, 0%)"
```
See `color_processing` documentation for more info.

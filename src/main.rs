mod command;
mod eps;
mod sb;

use crate::command::{Command as SBCommand, CommandType};
use crate::sb::*;
use cubeos_service::{Logger, Power};
use gpio::GpioOut;
use log::{error, info};
use serial::*;
use simplelog::*;
use std::env;
use std::io::Write;
use std::thread;
use std::time::Duration;

fn main() {
    let file_path = std::env::args().nth(1).unwrap_or_else(|| "./sbtest02.json".to_string());
    let startup_command = std::fs::read_to_string(file_path)
        .unwrap_or_else(|_| "Upsbtest02.json".to_string());
    // let _ = Logger::init();
    CombinedLogger::init(vec![
        TermLogger::new(LevelFilter::Info, Config::default(), TerminalMode::Mixed, ColorChoice::Auto)
    ]).unwrap();

    // TODO: power on payload (not implemented by ws yet)
    let mut power_3v3 = gpio::sysfs::SysFsGpioOutput::open(117).unwrap();
    power_3v3.set_value(1).unwrap();
    thread::sleep(Duration::from_secs(5));
    let mut power_5v = Power::new(Some(5)).unwrap();
    power_5v.initialize_payload().unwrap();

    info!("Starting up");

    let uart_path = "/dev/ttyS1".to_string();
    let uart_setting = serial::PortSettings {
        baud_rate: Baud115200,
        char_size: Bits8,
        parity: ParityNone,
        stop_bits: Stop1,
        flow_control: FlowNone,
    };
    let uart_timeout = Duration::from_secs(1);

    let mut sb = Sb::new(uart_path, uart_setting, uart_timeout, startup_command);

    match sb.app_logic() {
        Ok(()) => {
            info!("Operation complete");
        }
        Err(e) => {
            error!("Error: {:?}", e);
        }
    }
    let _ = sb
        .port
        .write(&SBCommand::simple_command(CommandType::PowerDown).to_bytes());

    // give payload time to shutdown
    thread::sleep(SHUTDOWN_ALLOWANCE);

    power_3v3.set_value(0).unwrap();
    power_5v.shutdown().unwrap();
}

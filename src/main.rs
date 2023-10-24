mod eps;

use crate::eps::Eps;
use cubeos_service::{Power, Config, Logger, Result};
use serial::*;
use std::thread;
use std::time::{Duration, Instant};
use chrono::prelude::*;
use gpio::GpioOut;
use gpio::sysfs::SysFsGpioOutput;
use isis_eps_api::PIUHkSel;
use ws_api::{Command as SBCommand, CommandType};
use log::{error, info, warn};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::fs::File;

const POWER_UP_ALLOWANCE: Duration = Duration::from_secs(60 * 5);
const ACKNOWLEDGE_MESSAGE_TIMEOUT: Duration = Duration::from_secs(60 * 1);
const ACKNOWLEDGE_MESSAGE_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(1);
const SHUTDOWN_ALLOWANCE: Duration = Duration::from_secs(30);

fn main() {
    let _ = Logger::init();
    // TODO: power on payload (not implemented by ws yet)    
    let mut power_3v3 = gpio::sysfs::SysFsGpioOutput::open(117).unwrap();
    power_3v3.set_value(1).unwrap();
    thread::sleep(Duration::from_secs(5));
    let mut power_5v = Power::new(Some(5)).unwrap();
    power_5v.initialize_payload().unwrap();

    match app_logic() {
        Ok(()) => {
            info!("Operation complete");
        }
        Err(e) => {
            error!("Error: {:?}", e);
        }
    }

    // give payload time to shutdown
    thread::sleep(SHUTDOWN_ALLOWANCE);

    power_3v3.set_value(0).unwrap();
    power_5v.shutdown().unwrap();
}

fn app_logic() -> Result<()> {
    let session_duration = Duration::from_secs(60 * 15);  // 15 minutes
    let session_start_time = Instant::now();

    let uart_path = "/dev/ttyS1".to_string();
    let uart_setting = serial::PortSettings {
        baud_rate: Baud115200,
        char_size: Bits8,
        parity: ParityNone,
        stop_bits: Stop1,
        flow_control: FlowNone,
    };
    let uart_timeout = Duration::from_secs(1);

    let mut port = serial::open(&uart_path).unwrap();
    port.configure(&uart_setting).unwrap();
    port.set_timeout(uart_timeout).unwrap();

    // wait for init
    receive_loop(&mut port, CommandType::Initialised, POWER_UP_ALLOWANCE)?;
    println!("Initialised");
    info!("Initialised");

    // send time
    let sb_command = SBCommand::time(Utc::now());
    println!("UTC: {:?}", Utc::now());
    println!("Send Time: {:?}", sb_command.data);
    info!("Send Time: {:?}", sb_command.data);
    port.write(&sb_command.data)?;

    // wait for time ack
    receive_loop(&mut port, CommandType::TimeAcknowledge, ACKNOWLEDGE_MESSAGE_TIMEOUT)?;
    println!("Time Acknowledged");
    info!("Time Acknowledged");

    thread::sleep(Duration::from_millis(100));

    // send startup command
    let startup_command = "patch01.json".as_bytes().to_vec();
    let sb_command = SBCommand::startup_command(startup_command);
    println!("Send Startup Command: {:?}", sb_command.data);
    info!("Send Startup Command: {:?}", sb_command.data);
    port.write(&sb_command.data)?;

    // wait for startup command ack
    receive_loop(&mut port, CommandType::StartupCommandAcknowledge, ACKNOWLEDGE_MESSAGE_TIMEOUT)?;
    println!("Startup Command Acknowledged");
    info!("Startup Command Acknowledged");

    // wait for session to finish
    let mut batt_low = false;
    let mut time_remaining = session_duration - session_start_time.elapsed();
    loop {
        match ftp(&mut port) {
            Ok(()) => break,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::TimedOut {
                    println!("FTP timed out");
                    continue;
                } else {
                    error!("Error: {:?}", e);
                    break;
                }
            }
        }
        time_remaining = session_duration - session_start_time.elapsed();
        batt_low = match Eps::piu_hk(PIUHkSel::PIUEngHK) {
            Ok(hk) => if hk.vip_dist_input.volt < 13500 {
                warn!("Battery low");
                true
            } else {
                false
            },
            Err(e) => {
                error!("Error: {:?}", e);
                false
            }
        };
        if time_remaining < SHUTDOWN_ALLOWANCE || batt_low {
            break;            
        }
    }
    Ok(())
}

fn receive_loop(port: &mut dyn Read, cmd_type: CommandType, timeout: Duration) -> std::io::Result<()> {
    let start_time = Instant::now();
    let mut data: Vec<u8> = Vec::new();
    loop {
        if start_time.elapsed() > timeout {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!(
                    "Timed out waiting for {:?}",
                    cmd_type
                ),
            ).into());
        }
        let mut buffer = [0u8; 1];
        if let Ok(response) = port.read(&mut buffer) {
            let byte = buffer[0];
            data.push(byte);
            if byte == 0 && data.len() > 2 {
                if cmd_type == CommandType::Initialised {
                    if let Some(cmd) = SBCommand::from_bytes(data[data.len()-3..].to_vec()) {
                        if cmd.command_type == cmd_type {
                            return Ok(());
                        } else {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                format!(
                                    "Received {:?} instead of {:?}",
                                    cmd, cmd_type
                                ),
                            ).into());
                        } 
                    } else {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!(
                                "Received {:?} instead of {:?}",
                                data, cmd_type
                            ),
                        ).into());
                    }
                } else {
                    if let Some(cmd) = SBCommand::from_bytes(data.clone()) {
                        if cmd.command_type == cmd_type {
                            return Ok(());
                        } else {
                            error!("Received {:?} instead of {:?}", cmd, cmd_type);
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                format!(
                                    "Received {:?} instead of {:?}",
                                    cmd, cmd_type
                                ),
                            ).into());
                        } 
                    } else {
                        error!("Received {:?} instead of {:?}", data, cmd_type);
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!(
                                "Received {:?} instead of {:?}",
                                data, cmd_type
                            ),
                        ).into());
                    }    
                }                            
            }
        }
    }
}

trait Port: Read + Write {}
impl Port for serial::SystemPort {}

fn ftp(port: &mut dyn Port) -> std::io::Result<()> {
    let mut buffer = [0; 1024];
    let mut file_name = String::new();

    // Receive file name
    loop {
        let bytes_read = port.read(&mut buffer)?;
        file_name.push_str(std::str::from_utf8(&buffer[..bytes_read]).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?);
        if bytes_read < buffer.len() {
            break;
        }
    }

    // Remove trailing null bytes and any directory path
    file_name = file_name.trim_end_matches(char::from(0)).rsplit('/').next().unwrap().to_string();

    // Send READY_RECEIVE_FILE message
    port.write_all(b"READY_RECEIVE_FILE")?;

    // Receive file data
    let mut file_data = Vec::new();
    loop {
        let bytes_read = port.read(&mut buffer)?;
        file_data.extend_from_slice(&buffer[..bytes_read]);
        if bytes_read < buffer.len() {
            break;
        }
    }

    // Send RECEIVED_FILE_DATA message
    port.write_all(b"RECEIVED_FILE_DATA")?;

    // Compute file hash
    let file_hash = Sha256::digest(&file_data);

    // Send SEND_FILE_HASH message
    port.write_all(b"SEND_FILE_HASH")?;

    // Receive file hash
    let mut hash_buffer = [0; 32];
    port.read_exact(&mut hash_buffer)?;

    // Check file hash
    if hash_buffer != file_hash.as_slice() {
        port.write_all(b"RECEIVE_FILE_ERROR_RETRY")?;
        return Err(std::io::Error::new(std::io::ErrorKind::Other, "File hash does not match"));
    }

    // Send RECEIVE_FILE_SUCCESS message
    port.write_all(b"RECEIVE_FILE_SUCCESS")?;

    // Write file data to disk
    let mut file = File::create(&file_name)?;
    file.write_all(&file_data)?;

    Ok(())
}
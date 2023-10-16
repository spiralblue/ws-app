mod eps;

use std::thread;
use std::time::{Duration, Instant};
use chrono::prelude::*;
use ws_api::{Command as SBCommand, CommandType};
use cubeos_service::*;
use log::{error, info, warn};
use gpio::GpioOut;
use gpio::sysfs::SysFsGpioOutput;
use std::str::FromStr;
use crate::eps::Eps;
use isis_eps_api::PIUHkSel;

// const POWER_UP_ALLOWANCE: Duration = Duration::from_secs(60 * 5);
// const ACKNOWLEDGE_MESSAGE_TIMEOUT: Duration = Duration::from_secs(60 * 1);
// const ACKNOWLEDGE_MESSAGE_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(1);
const SHUTDOWN_ALLOWANCE: Duration = Duration::from_secs(45);

app_macro! {
    spiral_blue:SpiralBlue{ 
        mutation: Initialised => fn initialised(&self) -> Result<()>;
        mutation: Time => fn time(&self) -> Result<()>;
        mutation: StartupCommand => fn startup_command(&self, cmd: Vec<u8>) -> Result<()>;
        mutation: Shutdown => fn shutdown(&self, time_remaining_s: u16) -> Result<()>;
        mutation: Ftp => fn ftp(&self) -> Result<()>;
    }
}

fn app_logic() -> Result<()> {
    let session_start_time = Instant::now();
    // TODO: get time remaining in session from service
    let session_duration = Duration::from_secs(60 * 15);  // 15 minutes
    
    // wait for payload to be ready
    match SpiralBlue::initialised() {
        Ok(()) => {
            info!("Initialised");
        }
        Err(e) => {
            error!("Error: {:?}", e);
        }
    }

    // send payload the current time
    match SpiralBlue::time() {
        Ok(()) => {
            info!("Time updated successfully");
        }
        Err(e) => {
            error!("Error: {:?}", e);
        }
    }

    // send payload the startup command
    let startup_command = "patch01.json".as_bytes().to_vec();
    match SpiralBlue::startup_command(startup_command) {
        Ok(()) => {
            info!("Starting operation");
        }
        Err(e) => {
            error!("Error: {:?}", e);
        }
    }

    // wait for session to finish
    let mut batt_low = false;
    let mut time_remaining = session_duration - session_start_time.elapsed();
    loop {
        match SpiralBlue::ftp() {
            Ok(()) => break,
            Err(e) => {
                if std::io::Error::from(e.clone()).kind() == std::io::ErrorKind::TimedOut {
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

    match SpiralBlue::shutdown(time_remaining.as_secs() as u16) {
        Ok(()) => {
            info!("Shutdown acknowledged");
        }
        Err(e) => {
            error!("Error: {:?}", e);
        }
    }

    // give payload time to shutdown
    thread::sleep(SHUTDOWN_ALLOWANCE);
    Ok(())
}

fn main() -> Result<()> {
    
    // TODO: power on payload (not implemented by ws yet)    
    let mut power_3v3 = gpio::sysfs::SysFsGpioOutput::open(115).unwrap();
    power_3v3.set_value(1).unwrap();

    let app = App::new(app_logic, Some(5), "./spero-service").run()?;

    power_3v3.set_value(0).unwrap();
    Ok(())
}

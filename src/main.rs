use std::thread;
use std::time::{Duration, Instant};
use chrono::prelude::*;
use ws_api::{Command, CommandType};

const POWER_UP_ALLOWANCE: Duration = Duration::from_secs(60 * 5);
const ACKNOWLEDGE_MESSAGE_TIMEOUT: Duration = Duration::from_secs(60 * 1);
const ACKNOWLEDGE_MESSAGE_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(1);
const SHUTDOWN_ALLOWANCE: Duration = Duration::from_secs(30);


fn send_message(command: Command) {
    // TODO: use service to send message
    println!("Sent a message.");
}

fn receive_message() -> Command {
    // TODO: use service to receive message
    return Command::simple_command(CommandType::Initialised);
}

fn wait_for_message(
    message_type: CommandType,
    queue: &mut Vec<Command>,
    timeout: Duration,
) -> Option<Command> {
    let start_time = Instant::now();
    if let Some(index) = queue.iter().position(|x| x.command_type == message_type) {
        return Some(queue.remove(index));
    }
    loop {
        if start_time.elapsed() > timeout {
            return None;
        }
        let message = receive_message();
        if message.command_type == message_type {
            return Some(message);
        } else {
            queue.push(message);
        }
    }
}

fn send_message_with_acknowledgment(
    message_func: impl Fn() -> Command,
    expected_acknowledgment_type: CommandType,
    queue: &mut Vec<Command>,
    timeout: Duration,
) -> Command {
    let start_time = Instant::now();
    loop {
        if start_time.elapsed() > timeout {
            panic!("Did not receive acknowledgment in time");
        }
        let message = message_func();
        send_message(message);
        if let Some(acknowledgment) = wait_for_message(
            expected_acknowledgment_type,
            queue,
            ACKNOWLEDGE_MESSAGE_ATTEMPT_TIMEOUT,
        ) {
            return acknowledgment;
        }
    }
}


fn main() {
    let mut message_queue = vec![];


    let session_start_time = Instant::now();
    // TODO: get time remaining in session
    let session_duration = Duration::from_secs(60 * 15);  // 15 minutes

    // TODO: power on payload

    // wait for payload to be ready
    let initialised = wait_for_message(
        CommandType::Initialised, &mut message_queue, POWER_UP_ALLOWANCE,
    );
    if initialised.is_some() {
        send_message(
            Command::simple_command(CommandType::InitialisedAcknowledge)
        );
    } else {
        panic!("Payload did not initialise in time");
    }

    // send payload the current time
    let time_func = || {
        let current_time = Utc::now();
        Command::time(current_time)
    };
    send_message_with_acknowledgment(
        time_func,
        CommandType::TimeAcknowledge,
        &mut message_queue,
        ACKNOWLEDGE_MESSAGE_TIMEOUT,
    );

    // send payload the startup command
    // TODO: get startup command somehow
    let startup_command = "patch01.json".as_bytes().to_vec();
    send_message_with_acknowledgment(
        || Command::startup_command(startup_command.clone()),
        CommandType::StartupCommandAcknowledge,
        &mut message_queue,
        ACKNOWLEDGE_MESSAGE_TIMEOUT,
    );

    // wait for session to finish
    let time_remaining = session_duration - session_start_time.elapsed();
    let session_finished = wait_for_message(
        CommandType::PowerDown, &mut message_queue, time_remaining,
    );
    if session_finished.is_some() {
        send_message(Command::simple_command(CommandType::PowerDownAcknowledge));
    } else {
        // terminate session
        send_message_with_acknowledgment(
            || Command::simple_command(CommandType::PowerDown),
            CommandType::PowerDownAcknowledge,
            &mut message_queue,
            SHUTDOWN_ALLOWANCE,
        );
    }

    // give payload time to shutdown
    thread::sleep(SHUTDOWN_ALLOWANCE)

    // TODO: power off payload
}

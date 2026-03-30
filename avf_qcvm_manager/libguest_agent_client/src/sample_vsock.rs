/*
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries.
 * SPDX-License-Identifier: BSD-3-Clause-Clear */
use anyhow::Result;
use std::thread;
use guest_agent_client::vsock::VsockClient;
use guest_agent_client::GuestAgentClient;

fn main() -> Result<()> {
    let port = 3301; //oemvm port
    let timeout = 10;
    let start_userspace_timer=4;

    let thread = thread::spawn(move || {
        match VsockClient::connect_userspace(0, start_userspace_timer, timeout, guest_agent_client::ServiceId::VsockPort(port)) {
            Ok(service) => {
                println!("Vsock connection established.");
                //Note: we can update state to userspace ready here.
                service
            }
            Err(e) => {
                eprintln!("Failed to connect: {:?}", e);
                panic!("Vsock connection failed.");
            }
        }
    });

    let service = thread.join().unwrap();
    service.shutdown()?;

    Ok(())
}

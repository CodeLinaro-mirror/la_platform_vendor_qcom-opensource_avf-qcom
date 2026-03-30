/*
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries.
 * SPDX-License-Identifier: BSD-3-Clause-Clear */
use anyhow::{Context, Result};
use log::info;
use nix::sys::select::{select, FdSet};
use nix::sys::time::TimeVal;
use serde::{Deserialize, Serialize};
use std::io::{BufWriter, Write};
use std::os::fd::BorrowedFd;
use std::os::unix::io::AsRawFd;
use std::time::Duration;
use vsock::{VsockListener, VsockStream, VMADDR_CID_HOST};
use crate::{GuestAgentClient, ServiceId, ServiceId::VsockPort};
use std::thread;

const WRITE_BUFFER_CAPACITY: usize = 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Request {
    Shutdown,
}

pub struct VsockClient {
    pub vsock_stream: VsockStream,
}


impl VsockClient {
    fn write_request(&self, request: Request) -> Result<()> {
        let mut buffer = BufWriter::with_capacity(WRITE_BUFFER_CAPACITY, &self.vsock_stream);
        ciborium::into_writer(&request, &mut buffer)?;
        buffer.flush().context("Failed to flush the buffer")?;
        info!("Sent request to the service VM.");
        Ok(())
    }
}

impl GuestAgentClient for VsockClient {
    /// Connect to the userspace of the VM using Vsock
    /// The connect call is a blocking call. Once it has connected
    /// we can safely assume the userspace is up.
    /// Return a handle to the vsock service.
    fn connect_userspace(_retry:u32, vm_userspace_start_timer: u32, timeout: u32, service_id: ServiceId) -> Result<Self> {
        let mut port = 0;
        if vm_userspace_start_timer > 0
        {
            thread::sleep(Duration::from_millis(vm_userspace_start_timer.into()));
        }
        if let VsockPort(vsock_port) = service_id {
            port = vsock_port;
        }
        let vsock_listener = VsockListener::bind_with_cid_port(VMADDR_CID_HOST, port)
            .context("Failed to bind vsock")?;

        let fd = vsock_listener.as_raw_fd();
        let borrowed_fd = unsafe { BorrowedFd::borrow_raw(fd) };
        let mut fds = FdSet::new();
        fds.insert(borrowed_fd);

        let mut timeout_tv = TimeVal::new(timeout as i64, 0);
        let result = select(fd + 1, Some(&mut fds), None, None, Some(&mut timeout_tv))
            .context("Select failed")?;

        if result > 0 {
            let (vsock_stream, _peer_addr) = vsock_listener
                .accept()
                .context("Failed to accept connection")?;

            info!("Accepted vsock connection on port {}!", port);
            vsock_stream.set_read_timeout(Some(Duration::from_secs(timeout.into())))?;
            vsock_stream.set_write_timeout(Some(Duration::from_secs(timeout.into())))?;

            Ok(Self { vsock_stream })
        } else {
            Err(anyhow::anyhow!("Accept timeout"))
        }
    }
    // this will send a shutdown request to vm.
    // Since vsock guest agent running in VM does not record any running task, it will
    // directly shutdown. this is different with mink.
    fn shutdown(&self) -> Result<()> {
        let request = Request::Shutdown;
        self.write_request(request)?;
        Ok(())
    }
}
